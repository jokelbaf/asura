use std::net::SocketAddr;

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

const RELAY: &str = "wineserver";
const WINDOWS: &str = ".exe";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Client {
    pub process: u32,
    pub name: String,
    pub command_line: String,
    pub target: Option<u64>,
}

impl Client {
    pub fn is_windows(&self) -> bool {
        self.name == RELAY || self.name.to_ascii_lowercase().ends_with(WINDOWS)
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Tls {
    pub server_name: Option<String>,
    pub offered_protocols: Vec<String>,
    pub protocol: Option<String>,
    pub version: Option<String>,
    pub cipher: Option<String>,
    pub certificates: Vec<Vec<u8>>,
    pub decrypted: bool,
    pub reason: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Connection {
    pub id: u64,
    pub client: Client,
    pub server: SocketAddr,
    pub tls: Option<Tls>,
    pub opened: OffsetDateTime,
}
