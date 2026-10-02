use std::io;
use std::net::{Ipv4Addr, Ipv6Addr};
use std::sync::Arc;
use std::time::Duration;

use authority::Authority;
use session::Store;
use tokio::net::TcpListener;
use tokio::runtime::Handle;
use tokio::task::JoinSet;

use crate::context::Context;
use crate::process::Processes;
use crate::tls::{Policy, Upstream};
use crate::{Composer, Resolve, Tamper, connection};

const ATTEMPTS: usize = 16;
const BACKOFF: Duration = Duration::from_millis(100);

pub struct Engine {
    listeners: Vec<TcpListener>,
    port: u16,
    context: Arc<Context>,
}

impl Engine {
    pub async fn bind(
        resolver: Arc<dyn Resolve>,
        authority: Arc<Authority>,
        store: Arc<Store>,
    ) -> io::Result<Self> {
        let (listeners, port) = listen().await?;
        let upstream = Upstream::new(authority.provider().clone()).map_err(io::Error::other)?;

        Ok(Self {
            listeners,
            port,
            context: Arc::new(Context {
                resolver,
                authority,
                store,
                policy: Arc::default(),
                tamper: Arc::default(),
                upstream,
                processes: Processes::default(),
                runtime: Handle::current(),
            }),
        })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn policy(&self) -> Arc<Policy> {
        self.context.policy.clone()
    }

    pub fn tamper(&self) -> Arc<Tamper> {
        self.context.tamper.clone()
    }

    pub fn composer(&self) -> Composer {
        Composer::new(self.context.clone())
    }

    pub async fn run(self) {
        let mut tasks = JoinSet::new();

        for listener in self.listeners {
            tasks.spawn(accept(listener, self.context.clone()));
        }

        tasks.join_all().await;
    }
}

async fn listen() -> io::Result<(Vec<TcpListener>, u16)> {
    for _ in 0..ATTEMPTS {
        let primary = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
        let port = primary.local_addr()?.port();

        match TcpListener::bind((Ipv6Addr::LOCALHOST, port)).await {
            Ok(secondary) => return Ok((vec![primary, secondary], port)),
            Err(error) if error.kind() == io::ErrorKind::AddrInUse => continue,
            Err(_) => return Ok((vec![primary], port)),
        }
    }

    Err(io::Error::new(
        io::ErrorKind::AddrInUse,
        "no free loopback port pair",
    ))
}

async fn accept(listener: TcpListener, context: Arc<Context>) {
    loop {
        match listener.accept().await {
            Ok((stream, peer)) => {
                tokio::spawn(connection::handle(context.clone(), stream, peer));
            }
            Err(_) => tokio::time::sleep(BACKOFF).await,
        }
    }
}
