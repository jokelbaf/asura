use std::net::SocketAddr;

#[derive(Clone, Debug)]
pub struct Route {
    pub destination: SocketAddr,
    pub process: u32,
    pub command: String,
    pub target: Option<u64>,
}

pub trait Resolve: Send + Sync {
    fn resolve(&self, peer: SocketAddr) -> Option<Route>;

    fn unreachable(&self, destination: SocketAddr);
}
