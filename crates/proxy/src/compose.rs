use std::io;
use std::net::{IpAddr, SocketAddr, ToSocketAddrs};
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use http::header::HOST;
use http::{HeaderMap, HeaderValue, Method, Request, Uri};
use http_body_util::{BodyExt, Full};
use session::{Client, Connection, Direction, Entry, Headers, Mark, Scheme, Tls};
use time::OffsetDateTime;
use tokio::net::TcpStream;
use tokio::time::timeout;

use crate::active::{self, Body, BoxError, Sender, Tap, Wire};
use crate::context::Context;

const PATIENCE: Duration = Duration::from_secs(15);
const PROTOCOLS: [&str; 2] = ["h2", "http/1.1"];
const NAME: &str = "Asura";
const COMMAND: &str = "Composer";

pub struct Outgoing {
    pub method: String,
    pub url: String,
    pub headers: Headers,
    pub body: Vec<u8>,
    pub address: Option<SocketAddr>,
}

struct Target {
    scheme: Scheme,
    method: Method,
    authority: String,
    host: String,
    port: u16,
    path: String,
}

struct Connected {
    sender: Sender,
    tls: Option<Tls>,
    wire: Wire,
}

#[derive(Clone)]
pub struct Composer {
    context: Arc<Context>,
}

impl Composer {
    pub(crate) fn new(context: Arc<Context>) -> Self {
        Self { context }
    }

    pub fn send(&self, outgoing: Outgoing) -> io::Result<Arc<Entry>> {
        let target = parse(&outgoing)?;

        let address = match outgoing.address {
            Some(address) => address,
            None => (target.host.as_str(), target.port)
                .to_socket_addrs()?
                .next()
                .ok_or_else(|| invalid("the host name did not resolve to an address"))?,
        };

        let opened = OffsetDateTime::now_utc();
        let connected = self
            .context
            .runtime
            .block_on(timeout(PATIENCE, self.connect(&target, address)))
            .unwrap_or_else(|_| Err("connecting timed out".into()));

        let connected = match connected {
            Ok(connected) => connected,
            Err(error) => {
                return Ok(self.failure(&target, &outgoing, address, opened, error.to_string()));
            }
        };

        let request = build(&target, &outgoing, connected.wire).map_err(invalid)?;
        let entry = self
            .context
            .store
            .insert(self.connection(address, connected.tls, opened));
        let recorded = record(&target, request.version(), request.headers());
        let time = OffsetDateTime::now_utc();

        entry.update(|record, spool| {
            let mut recorded = recorded;
            recorded.body = active::filled(&outgoing.body, spool);
            record.request = Some(recorded);
            record.timing.request_complete = Some(time);
            record.mark(Mark::Composed);
        });

        self.context
            .runtime
            .spawn(exchange(entry.clone(), connected.sender, request));
        Ok(entry)
    }

    async fn connect(&self, target: &Target, address: SocketAddr) -> Result<Connected, BoxError> {
        let stream = TcpStream::connect(address).await?;
        let _ = stream.set_nodelay(true);

        if target.scheme == Scheme::Http {
            return Ok(Connected {
                sender: Sender::handshake(stream, Wire::Http1).await?,
                tls: None,
                wire: Wire::Http1,
            });
        }

        let name = target
            .host
            .parse::<IpAddr>()
            .is_err()
            .then(|| target.host.clone());
        let protocols = PROTOCOLS
            .iter()
            .map(|protocol| protocol.as_bytes().to_vec())
            .collect();
        let stream = self
            .context
            .upstream
            .connect(name.as_deref(), protocols, address, stream)
            .await?;

        let (_, state) = stream.get_ref();
        let protocol = state
            .alpn_protocol()
            .map(|protocol| String::from_utf8_lossy(protocol).into_owned());
        let wire = if protocol.as_deref() == Some("h2") {
            Wire::Http2
        } else {
            Wire::Http1
        };

        let tls = Tls {
            server_name: name,
            offered_protocols: PROTOCOLS.map(str::to_owned).to_vec(),
            protocol,
            version: state
                .protocol_version()
                .map(|version| format!("{version:?}")),
            cipher: state
                .negotiated_cipher_suite()
                .map(|suite| format!("{:?}", suite.suite())),
            certificates: state
                .peer_certificates()
                .map(|chain| {
                    chain
                        .iter()
                        .map(|certificate| certificate.to_vec())
                        .collect()
                })
                .unwrap_or_default(),
            decrypted: true,
            reason: None,
        };

        Ok(Connected {
            sender: Sender::handshake(stream, wire).await?,
            tls: Some(tls),
            wire,
        })
    }

    fn connection(
        &self,
        server: SocketAddr,
        tls: Option<Tls>,
        opened: OffsetDateTime,
    ) -> Arc<Connection> {
        Arc::new(Connection {
            id: self.context.store.connection_id(),
            client: Client {
                process: 0,
                name: String::from(NAME),
                command_line: String::from(COMMAND),
                target: None,
            },
            server,
            tls,
            opened,
        })
    }

    fn failure(
        &self,
        target: &Target,
        outgoing: &Outgoing,
        address: SocketAddr,
        opened: OffsetDateTime,
        reason: String,
    ) -> Arc<Entry> {
        let tls = (target.scheme == Scheme::Https).then(|| Tls {
            server_name: Some(target.host.clone()),
            offered_protocols: PROTOCOLS.map(str::to_owned).to_vec(),
            decrypted: false,
            reason: Some(reason.clone()),
            ..Tls::default()
        });

        let entry = self
            .context
            .store
            .insert(self.connection(address, tls, opened));
        let recorded = record(
            target,
            http::Version::HTTP_11,
            &active::framed(&outgoing.headers, outgoing.body.len(), false),
        );

        entry.update(|record, spool| {
            let mut recorded = recorded;
            recorded.body = active::filled(&outgoing.body, spool);
            record.request = Some(recorded);
            record.mark(Mark::Composed);
            record.fail(reason);
        });

        entry
    }
}

async fn exchange(entry: Arc<Entry>, mut sender: Sender, request: Request<Body>) {
    let response = match sender.send(request).await {
        Ok(response) => response,
        Err(error) => return entry.update(|record, _| record.fail(error.to_string())),
    };

    let time = OffsetDateTime::now_utc();
    let (parts, body) = response.into_parts();
    let recorded = active::recorded_response(&parts);

    entry.update(|record, _| {
        record.response = Some(recorded);
        record.timing.response_started = Some(time);
    });

    let mut body = Tap::recording(body, entry, Direction::Inbound, true);

    while let Some(frame) = body.frame().await {
        if frame.is_err() {
            return;
        }
    }
}

fn parse(outgoing: &Outgoing) -> io::Result<Target> {
    let url = outgoing.url.trim().split('#').next().unwrap_or_default();
    let uri: Uri = url.parse().map_err(invalid)?;

    let scheme = match uri.scheme_str().map(str::to_ascii_lowercase).as_deref() {
        Some("https") => Scheme::Https,
        Some("http") => Scheme::Http,
        _ => return Err(invalid("the URL must start with http:// or https://")),
    };

    let authority = uri
        .authority()
        .ok_or_else(|| invalid("the URL has no host"))?;
    let default = if scheme == Scheme::Https { 443 } else { 80 };

    Ok(Target {
        scheme,
        method: Method::from_bytes(outgoing.method.trim().as_bytes()).map_err(invalid)?,
        authority: authority.to_string(),
        host: authority
            .host()
            .trim_start_matches('[')
            .trim_end_matches(']')
            .to_owned(),
        port: authority.port_u16().unwrap_or(default),
        path: uri
            .path_and_query()
            .map_or("/", |path| path.as_str())
            .to_owned(),
    })
}

fn build(target: &Target, outgoing: &Outgoing, wire: Wire) -> Result<Request<Body>, BoxError> {
    let mut headers = active::framed(&outgoing.headers, outgoing.body.len(), false);

    let (uri, version) = match wire {
        Wire::Http1 => {
            if !headers.contains_key(HOST) {
                headers.insert(HOST, HeaderValue::from_str(&target.authority)?);
            }

            (target.path.parse::<Uri>()?, http::Version::HTTP_11)
        }
        Wire::Http2 => {
            headers.remove(HOST);

            let absolute = format!(
                "{}://{}{}",
                target.scheme.as_str(),
                target.authority,
                target.path
            );
            (absolute.parse::<Uri>()?, http::Version::HTTP_2)
        }
    };

    let body = Full::new(Bytes::copy_from_slice(&outgoing.body))
        .map_err(|never| match never {})
        .boxed_unsync();

    let mut request = Request::new(body);
    *request.method_mut() = target.method.clone();
    *request.uri_mut() = uri;
    *request.version_mut() = version;
    *request.headers_mut() = headers;
    Ok(request)
}

fn record(target: &Target, version: http::Version, headers: &HeaderMap) -> session::Request {
    session::Request {
        method: target.method.to_string(),
        scheme: target.scheme,
        authority: target.authority.clone(),
        target: target.path.clone(),
        version: active::recorded_version(version),
        headers: active::recorded_headers(headers),
        trailers: Headers::default(),
        body: session::Body::default(),
    }
}

fn invalid(error: impl ToString) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, error.to_string())
}
