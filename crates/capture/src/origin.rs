use std::net::SocketAddr;

use crate::endpoint::to_socket;

#[derive(Clone, Debug)]
pub struct Origin {
    pub destination: SocketAddr,
    pub target: Option<u64>,
    pub process: u32,
    pub command: String,
}

impl From<common::Origin> for Origin {
    fn from(origin: common::Origin) -> Self {
        let length = origin
            .command
            .iter()
            .position(|&byte| byte == 0)
            .unwrap_or(origin.command.len());

        Self {
            destination: to_socket(&origin.destination),
            target: (origin.target != 0).then_some(origin.target),
            process: origin.process,
            command: String::from_utf8_lossy(&origin.command[..length]).into_owned(),
        }
    }
}
