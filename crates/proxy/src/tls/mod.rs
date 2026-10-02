mod hello;
mod policy;
mod upstream;
mod witness;

use std::io;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use authority::{Identity, Name};
use rustls::server::{ClientHello, ResolvesServerCert};
use rustls::sign::CertifiedKey;
use rustls::{AlertDescription, ServerConfig};
use session::{Scheme, Tls};
use tokio::net::TcpStream;
use tokio_rustls::TlsAcceptor;
use x509_parser::extensions::GeneralName;
use x509_parser::prelude::{FromDer, X509Certificate};

use crate::active::{self, Security, Wire};
use crate::context::{Context, Flow};
use crate::observe::{Http1, Http2, Observer, Tunnel};
use crate::relay::relay;
use crate::rewind::Rewind;

const REJECTION_WINDOW: Duration = Duration::from_secs(2);

pub use policy::{Policy, Rejection};
pub use upstream::Upstream;

use hello::Hello;
use witness::Witness;

pub async fn intercept(
    context: Arc<Context>,
    flow: Flow,
    client: Rewind<TcpStream>,
    upstream: TcpStream,
) {
    let (hello, client) = match hello::read(client).await {
        Ok(read) => read,
        Err(client) => {
            let tls = undecrypted(None, "unrecognised TLS client hello");
            return passthrough(&context, &flow, tls, client, upstream).await;
        }
    };

    let key = hello.key(flow.server);

    if let Some(reason) = context.policy.bypass(&key, &flow.client) {
        let tls = undecrypted(Some(&hello), &reason);
        return passthrough(&context, &flow, tls, client, upstream).await;
    }

    let connected = context
        .upstream
        .connect(
            hello.server_name.as_deref(),
            hello.protocols.clone(),
            flow.server,
            upstream,
        )
        .await;

    let upstream = match connected {
        Ok(upstream) => upstream,
        Err(error) => {
            let tls = undecrypted(
                Some(&hello),
                &format!("upstream TLS handshake failed: {error}"),
            );
            return match TcpStream::connect(flow.server).await {
                Ok(fresh) => passthrough(&context, &flow, tls, client, fresh).await,
                Err(error) => {
                    context.failure(&flow, Some(tls), format!("reconnecting failed: {error}"))
                }
            };
        }
    };

    let (_, state) = upstream.get_ref();
    let protocol = state.alpn_protocol().map(<[u8]>::to_vec);
    let certificates: Vec<Vec<u8>> = state
        .peer_certificates()
        .map(|chain| {
            chain
                .iter()
                .map(|certificate| certificate.to_vec())
                .collect()
        })
        .unwrap_or_default();

    let identity = identity(&hello, &certificates, flow.server.ip());
    let certified = match context.authority.issue(&identity) {
        Ok(certified) => certified,
        Err(error) => {
            let tls = undecrypted(Some(&hello), "certificate issuance failed");
            return context.failure(&flow, Some(tls), error.to_string());
        }
    };

    let acceptor = match configure(&context, certified, protocol.clone()) {
        Ok(config) => TlsAcceptor::from(config),
        Err(error) => {
            let tls = undecrypted(Some(&hello), "server configuration failed");
            return context.failure(&flow, Some(tls), error.to_string());
        }
    };

    let client = match acceptor.accept(client).await {
        Ok(client) => client,
        Err(error) if !refused(&error) => {
            let reason = format!("client closed the connection during the TLS handshake ({error})");
            let tls = undecrypted(Some(&hello), &reason);
            return context.failure(&flow, Some(tls), reason);
        }
        Err(error) => {
            let reason = format!("client rejected the interception certificate ({error})");
            context.policy.reject(key, &flow.client, &reason);

            let tls = undecrypted(Some(&hello), &reason);
            return context.failure(
                &flow,
                Some(tls),
                format!("{reason}; later connections pass through"),
            );
        }
    };

    let (_, state) = client.get_ref();
    let tls = Tls {
        server_name: hello.server_name.clone(),
        offered_protocols: hello.offered(),
        protocol: protocol
            .as_deref()
            .map(|protocol| String::from_utf8_lossy(protocol).into_owned()),
        version: state
            .protocol_version()
            .map(|version| format!("{version:?}")),
        cipher: state
            .negotiated_cipher_suite()
            .map(|suite| format!("{:?}", suite.suite())),
        certificates,
        decrypted: true,
        reason: None,
    };

    let client = Witness::new(client, context.policy.clone(), flow.client.clone());
    let scope = context.scope(&flow, Some(tls), Scheme::Https);
    let wire = match protocol.as_deref() {
        Some(b"h2") => Some(Wire::Http2),
        Some(b"http/1.1" | b"http/1.0") | None => Some(Wire::Http1),
        Some(_) => None,
    };

    let traffic = match wire {
        Some(wire)
            if context
                .tamper
                .applies(Scheme::Https, hello.server_name.as_deref()) =>
        {
            let security = Security::Tls {
                name: hello.server_name.clone(),
                protocols: protocol.into_iter().collect(),
            };

            active::intercept(
                context.clone(),
                scope,
                client,
                upstream,
                flow.server,
                security,
                wire,
            )
            .await
        }
        _ => {
            let observer: Box<dyn Observer> = match wire {
                Some(Wire::Http2) => Box::new(Http2::new(scope)),
                Some(Wire::Http1) => Box::new(Http1::new(scope)),
                None => Box::new(Tunnel::new(&scope)),
            };

            relay(client, upstream, observer).await
        }
    };

    if traffic.is_silent()
        && traffic.elapsed < REJECTION_WINDOW
        && !context.policy.has_vouched(&flow.client)
    {
        let reason = String::from(
            "client closed the connection right after the handshake (certificate not trusted)",
        );
        context.policy.reject(key, &flow.client, &reason);

        let tls = undecrypted(Some(&hello), &reason);
        context.failure(
            &flow,
            Some(tls),
            format!("{reason}; later connections pass through"),
        );
    }
}

async fn passthrough(
    context: &Context,
    flow: &Flow,
    tls: Tls,
    client: Rewind<TcpStream>,
    upstream: TcpStream,
) {
    let scope = context.scope(flow, Some(tls), Scheme::Https);
    relay(client, upstream, Box::new(Tunnel::new(&scope))).await;
}

fn refused(error: &io::Error) -> bool {
    matches!(
        error.get_ref().and_then(|inner| inner.downcast_ref::<rustls::Error>()),
        Some(rustls::Error::AlertReceived(alert)) if *alert != AlertDescription::CloseNotify
    )
}

fn undecrypted(hello: Option<&Hello>, reason: &str) -> Tls {
    Tls {
        server_name: hello.and_then(|hello| hello.server_name.clone()),
        offered_protocols: hello.map(Hello::offered).unwrap_or_default(),
        decrypted: false,
        reason: Some(reason.to_owned()),
        ..Tls::default()
    }
}

fn configure(
    context: &Context,
    certified: Arc<CertifiedKey>,
    protocol: Option<Vec<u8>>,
) -> Result<Arc<ServerConfig>, rustls::Error> {
    let mut config = ServerConfig::builder_with_provider(context.authority.provider().clone())
        .with_safe_default_protocol_versions()?
        .with_no_client_auth()
        .with_cert_resolver(Arc::new(Fixed(certified)));

    config.alpn_protocols = protocol.into_iter().collect();
    Ok(Arc::new(config))
}

fn identity(hello: &Hello, certificates: &[Vec<u8>], address: IpAddr) -> Identity {
    let requested = hello.server_name.iter().map(|name| Name::Dns(name.clone()));
    let mirrored = certificates
        .first()
        .map(|leaf| mirror(leaf))
        .unwrap_or_default();
    let fallback = hello.server_name.is_none().then_some(Name::Ip(address));

    Identity::new(requested.chain(mirrored).chain(fallback))
}

fn mirror(der: &[u8]) -> Vec<Name> {
    let Ok((_, certificate)) = X509Certificate::from_der(der) else {
        return Vec::new();
    };

    let alternatives: Vec<Name> = certificate
        .subject_alternative_name()
        .ok()
        .flatten()
        .map(|extension| {
            extension
                .value
                .general_names
                .iter()
                .filter_map(|name| match name {
                    GeneralName::DNSName(name) => Some(Name::Dns((*name).to_owned())),
                    GeneralName::IPAddress(bytes) => ip(bytes).map(Name::Ip),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default();

    if !alternatives.is_empty() {
        return alternatives;
    }

    certificate
        .subject()
        .iter_common_name()
        .filter_map(|name| name.as_str().ok())
        .map(|name| Name::Dns(name.to_owned()))
        .collect()
}

fn ip(bytes: &[u8]) -> Option<IpAddr> {
    match bytes.len() {
        4 => Some(IpAddr::from(<[u8; 4]>::try_from(bytes).ok()?)),
        16 => Some(IpAddr::from(<[u8; 16]>::try_from(bytes).ok()?)),
        _ => None,
    }
}

#[derive(Debug)]
struct Fixed(Arc<CertifiedKey>);

impl ResolvesServerCert for Fixed {
    fn resolve(&self, _: ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
        Some(self.0.clone())
    }
}
