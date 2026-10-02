use std::net::SocketAddr;
use std::sync::Arc;

use authority::Authority;
use session::{Client, Connection, Scheme, Store, Tls};
use time::OffsetDateTime;
use tokio::runtime::Handle;

use crate::Resolve;
use crate::observe::Scope;
use crate::process::Processes;
use crate::tamper::Tamper;
use crate::tls::{Policy, Upstream};

pub struct Flow {
    pub client: Client,
    pub server: SocketAddr,
    pub opened: OffsetDateTime,
}

pub struct Context {
    pub resolver: Arc<dyn Resolve>,
    pub authority: Arc<Authority>,
    pub store: Arc<Store>,
    pub policy: Arc<Policy>,
    pub tamper: Arc<Tamper>,
    pub upstream: Upstream,
    pub processes: Processes,
    pub runtime: Handle,
}

impl Context {
    pub fn connection(&self, flow: &Flow, tls: Option<Tls>) -> Arc<Connection> {
        Arc::new(Connection {
            id: self.store.connection_id(),
            client: flow.client.clone(),
            server: flow.server,
            tls,
            opened: flow.opened,
        })
    }

    pub fn scope(&self, flow: &Flow, tls: Option<Tls>, scheme: Scheme) -> Scope {
        Scope {
            store: self.store.clone(),
            connection: self.connection(flow, tls),
            scheme,
        }
    }

    pub fn failure(&self, flow: &Flow, tls: Option<Tls>, reason: String) {
        let entry = self.store.open(self.connection(flow, tls));

        entry.update(|record, _| {
            record.tunnel = Some(Default::default());
            record.fail(reason);
        });
    }
}
