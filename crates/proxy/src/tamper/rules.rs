use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Responder {
    pub enabled: bool,
    pub pattern: String,
    pub method: String,
    pub action: Action,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    File(PathBuf),
    Mock {
        status: u16,
        content_type: String,
        body: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Breakpoint {
    pub enabled: bool,
    pub pattern: String,
    pub method: String,
    pub request: bool,
    pub response: bool,
}
