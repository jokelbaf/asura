use std::io::ErrorKind;
use std::net::SocketAddr;
use std::sync::Arc;

use session::{Client, Scheme};
use time::OffsetDateTime;
use tokio::net::TcpStream;

use crate::Route;
use crate::active::{self, Security, Wire};
use crate::classify::{Protocol, classify};
use crate::context::{Context, Flow};
use crate::observe::{Http1, Http2, Observer, Tunnel};
use crate::process;
use crate::relay::relay;
use crate::rewind::Rewind;
use crate::tls;

pub async fn handle(context: Arc<Context>, mut client: TcpStream, peer: SocketAddr) {
    let Some(route) = context.resolver.resolve(peer) else {
        return;
    };

    let Ok(local) = client.local_addr() else {
        return;
    };

    let opened = OffsetDateTime::now_utc();

    let upstream = match TcpStream::connect(route.destination).await {
        Ok(upstream) => upstream,
        Err(error) => {
            let mut reason = format!("connecting to {} failed: {error}", route.destination);

            if matches!(
                error.kind(),
                ErrorKind::NetworkUnreachable
                    | ErrorKind::HostUnreachable
                    | ErrorKind::ConnectionRefused
            ) {
                context.resolver.unreachable(route.destination);
                reason.push_str("; later connections to this address bypass the proxy");
            }

            let flow = Flow {
                client: identify(&context, route.clone(), peer, local).await,
                server: route.destination,
                opened,
            };

            context.failure(&flow, None, reason);
            let _ = client.set_zero_linger();
            return;
        }
    };

    let _ = client.set_nodelay(true);
    let _ = upstream.set_nodelay(true);

    let Ok((protocol, prefix)) = classify(&mut client).await else {
        return;
    };

    let flow = Flow {
        client: identify(&context, route.clone(), peer, local).await,
        server: route.destination,
        opened,
    };
    let client = Rewind::new(prefix, client);

    if let Protocol::Tls = protocol {
        return tls::intercept(context, flow, client, upstream).await;
    }

    let scope = context.scope(&flow, None, Scheme::Http);
    let wire = match protocol {
        Protocol::Http1 => Some(Wire::Http1),
        Protocol::Http2 => Some(Wire::Http2),
        Protocol::Tls | Protocol::Unknown => None,
    };

    if let Some(wire) = wire
        && context.tamper.applies(Scheme::Http, None)
    {
        active::intercept(
            context.clone(),
            scope,
            client,
            upstream,
            route.destination,
            Security::Plain,
            wire,
        )
        .await;
        return;
    }

    let observer: Box<dyn Observer> = match protocol {
        Protocol::Http1 => Box::new(Http1::new(scope)),
        Protocol::Http2 => Box::new(Http2::new(scope)),
        Protocol::Tls | Protocol::Unknown => Box::new(Tunnel::new(&scope)),
    };

    relay(client, upstream, observer).await;
}

async fn identify(
    context: &Arc<Context>,
    route: Route,
    peer: SocketAddr,
    local: SocketAddr,
) -> Client {
    let shared = context.clone();
    let fallback = process::fallback(&route);

    tokio::task::spawn_blocking(move || shared.processes.describe(&route, peer, local))
        .await
        .unwrap_or(fallback)
}
