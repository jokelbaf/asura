use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use http::{Request, Response};
use hyper::body::Incoming;
use hyper::client::conn::{http1, http2};
use hyper_util::rt::{TokioExecutor, TokioIo};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio::time::timeout;

use super::{Body, BoxError, Wire};
use crate::context::Context;

const PATIENCE: Duration = Duration::from_secs(15);

pub trait Io: AsyncRead + AsyncWrite + Unpin + Send + 'static {}

impl<T: AsyncRead + AsyncWrite + Unpin + Send + 'static> Io for T {}

#[derive(Clone)]
pub enum Security {
    Plain,
    Tls {
        name: Option<String>,
        protocols: Vec<Vec<u8>>,
    },
}

pub enum Sender {
    Http1(http1::SendRequest<Body>),
    Http2(http2::SendRequest<Body>),
}

impl Sender {
    pub async fn handshake(io: impl Io, wire: Wire) -> Result<Self, BoxError> {
        let io = TokioIo::new(io);

        Ok(match wire {
            Wire::Http1 => {
                let (sender, connection) = http1::Builder::new()
                    .preserve_header_case(true)
                    .handshake(io)
                    .await?;
                tokio::spawn(connection.with_upgrades());
                Self::Http1(sender)
            }
            Wire::Http2 => {
                let (sender, connection) = http2::Builder::new(TokioExecutor::new())
                    .handshake(io)
                    .await?;
                tokio::spawn(connection);
                Self::Http2(sender)
            }
        })
    }

    pub async fn send(&mut self, request: Request<Body>) -> Result<Response<Incoming>, BoxError> {
        match self {
            Self::Http1(sender) => {
                sender.ready().await?;
                Ok(sender.send_request(request).await?)
            }
            Self::Http2(sender) => {
                sender.ready().await?;
                Ok(sender.send_request(request).await?)
            }
        }
    }

    fn is_closed(&self) -> bool {
        match self {
            Self::Http1(sender) => sender.is_closed(),
            Self::Http2(sender) => sender.is_closed(),
        }
    }
}

#[derive(Default)]
struct Slot {
    fresh: Option<Box<dyn Io>>,
    sender: Option<Sender>,
}

pub struct Link {
    context: Arc<Context>,
    destination: SocketAddr,
    security: Security,
    wire: Wire,
    slot: Mutex<Slot>,
}

impl Link {
    pub fn new(
        context: Arc<Context>,
        destination: SocketAddr,
        security: Security,
        wire: Wire,
        stream: impl Io,
    ) -> Self {
        Self {
            context,
            destination,
            security,
            wire,
            slot: Mutex::new(Slot {
                fresh: Some(Box::new(stream)),
                sender: None,
            }),
        }
    }

    pub fn wire(&self) -> Wire {
        self.wire
    }

    pub async fn send(&self, request: Request<Body>) -> Result<Response<Incoming>, BoxError> {
        let mut slot = self.slot.lock().await;

        match self.ready(&mut slot).await? {
            Sender::Http1(sender) => {
                sender.ready().await?;
                let response = sender.send_request(request);
                drop(slot);
                Ok(response.await?)
            }
            Sender::Http2(sender) => {
                let mut sender = sender.clone();
                drop(slot);
                sender.ready().await?;
                Ok(sender.send_request(request).await?)
            }
        }
    }

    async fn ready<'a>(&self, slot: &'a mut Slot) -> Result<&'a mut Sender, BoxError> {
        let sender = match slot.sender.take() {
            Some(sender) if !sender.is_closed() => sender,
            _ => match slot.fresh.take() {
                Some(stream) => Sender::handshake(stream, self.wire).await?,
                None => self.connect().await?,
            },
        };

        Ok(slot.sender.insert(sender))
    }

    async fn connect(&self) -> Result<Sender, BoxError> {
        let stream = timeout(PATIENCE, TcpStream::connect(self.destination)).await??;
        let _ = stream.set_nodelay(true);

        match &self.security {
            Security::Plain => Sender::handshake(stream, self.wire).await,
            Security::Tls { name, protocols } => {
                let stream = self
                    .context
                    .upstream
                    .connect(name.as_deref(), protocols.clone(), self.destination, stream)
                    .await?;

                Sender::handshake(stream, self.wire).await
            }
        }
    }
}
