mod buffer;
mod convert;
mod link;
mod respond;
mod tap;

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;

use bytes::Bytes;
use http::header::{CONNECTION, CONTENT_LENGTH, HOST, TRANSFER_ENCODING, UPGRADE};
use http::{
    HeaderMap, HeaderName, HeaderValue, Method, Request, Response, StatusCode, Uri, request,
    response,
};
use http_body_util::combinators::UnsyncBoxBody;
use http_body_util::{BodyExt, Empty, Full};
use hyper::body::Incoming;
use hyper::ext::ReasonPhrase;
use hyper::server::conn::{http1, http2};
use hyper::service::service_fn;
use hyper::upgrade::OnUpgrade;
use hyper_util::rt::{TokioExecutor, TokioIo};
use session::{Direction, Entry, Headers, Mark, Scheme, Stage};
use time::OffsetDateTime;

use crate::classify::{Protocol, classify};
use crate::context::Context;
use crate::observe::{Observer, Scope, Tunnel, WebSocket};
use crate::relay::{Traffic, relay};
use crate::rewind::Rewind;
use crate::tamper::{Action, Plan, RequestDraft, Verdict};

pub use convert::{
    filled, headers as recorded_headers, response as recorded_response, version as recorded_version,
};
pub use link::{Io, Link, Security, Sender};
pub use tap::Tap;

pub type BoxError = Box<dyn std::error::Error + Send + Sync>;
pub type Body = UnsyncBoxBody<Bytes, BoxError>;

const HOLD_LIMIT: usize = 64 << 20;
const ABORTED: &str = "aborted at a breakpoint";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wire {
    Http1,
    Http2,
}

pub async fn intercept(
    context: Arc<Context>,
    scope: Scope,
    mut client: impl Io,
    upstream: impl Io,
    destination: SocketAddr,
    security: Security,
    wire: Wire,
) -> Traffic {
    let started = Instant::now();
    let (protocol, prefix) = classify(&mut client)
        .await
        .unwrap_or((Protocol::Unknown, Bytes::new()));
    let sent = prefix.len() as u64;
    let client = Rewind::new(prefix, client);

    let served = match protocol {
        Protocol::Http1 => Wire::Http1,
        Protocol::Http2 => Wire::Http2,
        Protocol::Tls | Protocol::Unknown => {
            return relay(client, upstream, Box::new(Tunnel::new(&scope))).await;
        }
    };

    let link = Link::new(context.clone(), destination, security, wire, upstream);
    serve(context, scope, client, link, served).await;

    Traffic {
        sent,
        received: 0,
        elapsed: started.elapsed(),
    }
}

async fn serve(context: Arc<Context>, scope: Scope, client: impl Io, link: Link, wire: Wire) {
    let exchanger = Arc::new(Exchanger {
        context,
        scope,
        link,
    });
    let service = service_fn(move |request| exchanger.clone().handle(request));
    let io = TokioIo::new(client);

    let _ = match wire {
        Wire::Http1 => {
            http1::Builder::new()
                .preserve_header_case(true)
                .header_read_timeout(None)
                .serve_connection(io, service)
                .with_upgrades()
                .await
        }
        Wire::Http2 => {
            http2::Builder::new(TokioExecutor::new())
                .serve_connection(io, service)
                .await
        }
    };
}

struct Exchanger {
    context: Arc<Context>,
    scope: Scope,
    link: Link,
}

struct Holds {
    request: bool,
    response: bool,
}

impl Exchanger {
    async fn handle(
        self: Arc<Self>,
        mut request: Request<Incoming>,
    ) -> Result<Response<Body>, BoxError> {
        let entry = self.scope.store.open(self.scope.connection.clone());
        let pending = Pending(Some(entry.clone()));

        let upgrade = (request.method() == Method::CONNECT
            || token(request.headers(), CONNECTION, "upgrade"))
        .then(|| hyper::upgrade::on(&mut request));

        let (parts, body) = request.into_parts();
        let recorded = convert::request(&self.scope, &parts);
        let (url, authority) = (recorded.url(), recorded.authority.clone());
        entry.update(|record, _| record.request = Some(recorded));

        let plan = if entry.is_listed() {
            self.context.tamper.plan(parts.method.as_str(), &url)
        } else {
            Plan::default()
        };
        let body = Tap::recording(body, entry.clone(), Direction::Outbound, false).boxed_unsync();

        let holds = Holds {
            request: plan.request,
            response: plan.response,
        };

        let outcome = match plan.responder {
            Some((pattern, action)) => {
                self.mock(&entry, body, parts.version, pattern, &action)
                    .await
            }
            None => {
                self.forward(&entry, parts, body, holds, &authority, upgrade)
                    .await
            }
        };

        pending.settle(&outcome);
        outcome
    }

    async fn forward(
        &self,
        entry: &Arc<Entry>,
        parts: request::Parts,
        body: Body,
        holds: Holds,
        authority: &str,
        upgrade: Option<OnUpgrade>,
    ) -> Result<Response<Body>, BoxError> {
        let connect = parts.method == Method::CONNECT;

        let (mut request, break_response) = if holds.request {
            self.hold_request(entry, parts, body).await?
        } else {
            (Request::from_parts(parts, body), false)
        };

        normalize(&mut request, self.link.wire(), self.scope.scheme, authority);

        let response = self.link.send(request).await?;
        let time = OffsetDateTime::now_utc();
        let switching = response.status() == StatusCode::SWITCHING_PROTOCOLS
            || (connect && response.status().is_success());

        if switching && let Some(upgrade) = upgrade {
            return Ok(self.switch(entry, response, upgrade, time));
        }

        let (parts, body) = response.into_parts();
        let recorded = convert::response(&parts);

        entry.update(|record, _| {
            record.response = Some(recorded);
            record.timing.response_started = Some(time);
        });

        let hold = holds.response || break_response;
        let body = Tap::recording(body, entry.clone(), Direction::Inbound, !hold).boxed_unsync();

        if hold {
            return self.hold_response(entry, parts, body).await;
        }

        Ok(Response::from_parts(parts, body))
    }

    async fn hold_request(
        &self,
        entry: &Arc<Entry>,
        parts: request::Parts,
        body: Body,
    ) -> Result<(Request<Body>, bool), BoxError> {
        let buffered = buffer::collect(body, HOLD_LIMIT).await?;

        if !buffered.is_complete() {
            return Ok((Request::from_parts(parts, buffered.into_body()), false));
        }

        match self.context.tamper.pause(entry, Stage::Request).await {
            Some(Verdict::Abort) => Err(abort(entry)),
            Some(Verdict::Request {
                draft: Some(draft),
                break_response,
            }) => {
                let request = match edited(parts, &draft, buffered.trailers) {
                    Ok(request) => request,
                    Err(error) => {
                        resume(entry);
                        return Err(error);
                    }
                };

                let headers = convert::headers(request.headers());

                entry.update(|record, spool| {
                    record.paused = None;

                    if let Some(recorded) = &mut record.request {
                        recorded.method = draft.method.trim().to_owned();
                        recorded.target = draft.target.trim().to_owned();
                        recorded.headers = headers;
                        recorded.body = filled(&draft.body, spool);
                    }

                    record.mark(Mark::RequestEdited);
                });

                Ok((request, break_response))
            }
            verdict => {
                resume(entry);
                let break_response = matches!(
                    verdict,
                    Some(Verdict::Request {
                        break_response: true,
                        ..
                    })
                );
                Ok((
                    Request::from_parts(parts, buffered.into_body()),
                    break_response,
                ))
            }
        }
    }

    async fn hold_response(
        &self,
        entry: &Arc<Entry>,
        mut parts: response::Parts,
        body: Body,
    ) -> Result<Response<Body>, BoxError> {
        let buffered = buffer::collect(body, HOLD_LIMIT).await?;
        let completing = |body: Body| Tap::completing(body, entry.clone()).boxed_unsync();

        if !buffered.is_complete() {
            return Ok(Response::from_parts(
                parts,
                completing(buffered.into_body()),
            ));
        }

        match self.context.tamper.pause(entry, Stage::Response).await {
            Some(Verdict::Abort) => Err(abort(entry)),
            Some(Verdict::Response { draft: Some(draft) }) => {
                let status = match StatusCode::from_u16(draft.status) {
                    Ok(status) => status,
                    Err(error) => {
                        resume(entry);
                        return Err(error.into());
                    }
                };

                let previous = parts
                    .extensions
                    .remove::<ReasonPhrase>()
                    .filter(|_| parts.status == status);

                parts.status = status;
                parts.headers = framed(
                    &draft.headers,
                    draft.body.len(),
                    buffered.trailers.is_some(),
                );
                phrase(&mut parts, previous.as_ref().map(ReasonPhrase::as_bytes));

                let mut recorded = convert::response(&parts);

                entry.update(|record, spool| {
                    record.paused = None;
                    recorded.trailers = record
                        .response
                        .take()
                        .map(|response| response.trailers)
                        .unwrap_or_default();
                    recorded.body = filled(&draft.body, spool);
                    record.response = Some(recorded);
                    record.mark(Mark::ResponseEdited);
                });

                let body = buffer::replay(Bytes::from(draft.body), buffered.trailers, None);
                Ok(Response::from_parts(parts, completing(body)))
            }
            _ => {
                resume(entry);
                Ok(Response::from_parts(
                    parts,
                    completing(buffered.into_body()),
                ))
            }
        }
    }

    async fn mock(
        &self,
        entry: &Arc<Entry>,
        body: Body,
        version: http::Version,
        pattern: String,
        action: &Action,
    ) -> Result<Response<Body>, BoxError> {
        let _ = buffer::collect(body, HOLD_LIMIT).await;
        let mock = respond::mock(action).await;

        let mut response = Response::new(
            Full::new(mock.body.clone())
                .map_err(|never| match never {})
                .boxed_unsync(),
        );
        *response.status_mut() = mock.status;
        *response.version_mut() = version;
        *response.headers_mut() = framed(&mock.headers, mock.body.len(), false);

        let (mut parts, body) = response.into_parts();
        phrase(&mut parts, mock.reason.as_deref().map(str::as_bytes));

        let mut recorded = convert::response(&parts);
        let time = OffsetDateTime::now_utc();

        entry.update(|record, spool| {
            recorded.body = filled(&mock.body, spool);
            record.response = Some(recorded);
            record.timing.response_started = Some(time);
            record.mark(Mark::Mocked(pattern));
        });

        Ok(Response::from_parts(
            parts,
            Tap::completing(body, entry.clone()).boxed_unsync(),
        ))
    }

    fn switch(
        &self,
        entry: &Arc<Entry>,
        mut response: Response<Incoming>,
        client: OnUpgrade,
        time: OffsetDateTime,
    ) -> Response<Body> {
        let upstream = hyper::upgrade::on(&mut response);
        let (parts, _) = response.into_parts();
        let websocket = parts.status == StatusCode::SWITCHING_PROTOCOLS
            && token(&parts.headers, UPGRADE, "websocket");

        let mut recorded = convert::response(&parts);
        recorded.body.finish();
        let extensions: Headers = recorded.headers.clone();

        entry.update(|record, _| {
            record.response = Some(recorded);
            record.timing.response_started = Some(time);
            record.timing.response_complete = Some(time);
        });

        let entry = entry.clone();

        tokio::spawn(async move {
            let (Ok(client), Ok(upstream)) = tokio::join!(client, upstream) else {
                return entry.update(|record, _| record.fail("the connection upgrade failed"));
            };

            let observer: Box<dyn Observer> = if websocket {
                Box::new(WebSocket::new(entry, &extensions))
            } else {
                Box::new(Tunnel::attach(entry))
            };

            relay(TokioIo::new(client), TokioIo::new(upstream), observer).await;
        });

        Response::from_parts(
            parts,
            Empty::new().map_err(|never| match never {}).boxed_unsync(),
        )
    }
}

struct Pending(Option<Arc<Entry>>);

impl Pending {
    fn settle(mut self, outcome: &Result<Response<Body>, BoxError>) {
        let Some(entry) = self.0.take() else {
            return;
        };

        if let Err(error) = outcome {
            entry.update(|record, _| record.fail(error.to_string()));
        }
    }
}

impl Drop for Pending {
    fn drop(&mut self) {
        if let Some(entry) = self.0.take() {
            entry.update(|record, _| {
                record.fail("the client closed the connection before the response arrived")
            });
        }
    }
}

pub fn framed(headers: &Headers, length: usize, trailers: bool) -> HeaderMap {
    let mut map = convert::map(headers);
    map.remove(CONTENT_LENGTH);
    map.remove(TRANSFER_ENCODING);

    if length > 0 && !trailers {
        map.insert(CONTENT_LENGTH, HeaderValue::from(length));
    }

    map
}

pub fn normalize(request: &mut Request<Body>, wire: Wire, scheme: Scheme, authority: &str) {
    match wire {
        Wire::Http2 => {
            if request.uri().authority().is_none() {
                let authority = request
                    .headers()
                    .get(HOST)
                    .and_then(|host| host.to_str().ok())
                    .unwrap_or(authority)
                    .to_owned();
                let path = request
                    .uri()
                    .path_and_query()
                    .map_or("/", |path| path.as_str())
                    .to_owned();

                if let Ok(uri) = Uri::builder()
                    .scheme(scheme.as_str())
                    .authority(authority)
                    .path_and_query(path)
                    .build()
                {
                    *request.uri_mut() = uri;
                }
            }

            request.headers_mut().remove(HOST);
            *request.version_mut() = http::Version::HTTP_2;
        }
        Wire::Http1 if request.version() == http::Version::HTTP_2 => {
            let uri = request.uri().clone();

            if let Some(authority) = uri.authority()
                && !request.headers().contains_key(HOST)
                && let Ok(value) = HeaderValue::from_str(authority.as_str())
            {
                request.headers_mut().insert(HOST, value);
            }

            if request.method() != Method::CONNECT
                && let Some(path) = uri.path_and_query()
                && let Ok(origin) = path.as_str().parse()
            {
                *request.uri_mut() = origin;
            }

            *request.version_mut() = http::Version::HTTP_11;
        }
        Wire::Http1 => {}
    }
}

fn phrase(parts: &mut response::Parts, custom: Option<&[u8]>) {
    let reason = custom
        .filter(|reason| !reason.is_empty())
        .and_then(|reason| ReasonPhrase::try_from(reason).ok())
        .or_else(|| {
            parts
                .status
                .canonical_reason()
                .is_none()
                .then(|| ReasonPhrase::from_static(b""))
        });

    if let Some(reason) = reason {
        parts.extensions.insert(reason);
    }
}

fn edited(
    mut parts: request::Parts,
    draft: &RequestDraft,
    trailers: Option<HeaderMap>,
) -> Result<Request<Body>, BoxError> {
    parts.method = Method::from_bytes(draft.method.trim().as_bytes())?;
    parts.uri = draft.target.trim().parse::<Uri>()?;
    parts.headers = framed(&draft.headers, draft.body.len(), trailers.is_some());

    let body = buffer::replay(Bytes::copy_from_slice(&draft.body), trailers, None);
    Ok(Request::from_parts(parts, body))
}

fn abort(entry: &Entry) -> BoxError {
    resume(entry);
    ABORTED.into()
}

fn resume(entry: &Entry) {
    entry.update(|record, _| record.paused = None);
}

fn token(headers: &HeaderMap, name: HeaderName, wanted: &str) -> bool {
    headers
        .get_all(name)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .any(|item| item.trim().eq_ignore_ascii_case(wanted))
}
