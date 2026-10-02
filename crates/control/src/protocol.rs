use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub enum Request {
    Register {
        name: String,
        process: u32,
        command: Vec<String>,
    },
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Reply {
    Registered { capturing: bool },
    Failed(String),
}
