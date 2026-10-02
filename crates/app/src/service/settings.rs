use std::fs;
use std::io;
use std::path::PathBuf;

use directories::BaseDirs;
use proxy::{Breakpoint, Responder};
use serde::{Deserialize, Serialize};

const DIRECTORY: &str = "asura";
const FILE: &str = "settings.json";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub block_quic: bool,
    pub exclusions: Vec<String>,
    pub undecrypted: Vec<String>,
    pub prefixes: Vec<PathBuf>,
    pub responders: Vec<Responder>,
    pub breakpoints: Vec<Breakpoint>,
    pub window: Geometry,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Geometry {
    pub width: i32,
    pub height: i32,
    pub maximized: bool,
    pub split: i32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            block_quic: true,
            exclusions: Vec::new(),
            undecrypted: Vec::new(),
            prefixes: Vec::new(),
            responders: Vec::new(),
            breakpoints: Vec::new(),
            window: Geometry::default(),
        }
    }
}

impl Default for Geometry {
    fn default() -> Self {
        Self {
            width: 1360,
            height: 820,
            maximized: false,
            split: 720,
        }
    }
}

impl Settings {
    pub fn load() -> Self {
        location()
            .and_then(|path| fs::read(path).ok())
            .and_then(|contents| serde_json::from_slice(&contents).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> io::Result<()> {
        let path = location()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no configuration directory"))?;
        let temporary = path.with_extension("tmp");

        if let Some(directory) = path.parent() {
            fs::create_dir_all(directory)?;
        }

        fs::write(&temporary, serde_json::to_vec_pretty(self)?)?;
        fs::rename(temporary, path)
    }
}

fn location() -> Option<PathBuf> {
    BaseDirs::new().map(|directories| directories.config_dir().join(DIRECTORY).join(FILE))
}
