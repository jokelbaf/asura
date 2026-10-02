use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use crate::certificate::Certificate;
use crate::{Error, registry};

const SYSTEM_REGISTRY: &str = "system.reg";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    Wine,
    Steam { app: u32 },
    Lutris,
    Heroic,
    Bottles,
    Custom,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Installed,
    Missing,
    Uninitialised,
}

#[derive(Clone, Debug)]
pub struct Prefix {
    pub path: PathBuf,
    pub name: String,
    pub source: Source,
}

impl Prefix {
    pub fn new(path: PathBuf, name: impl Into<String>, source: Source) -> Self {
        Self {
            path,
            name: name.into(),
            source,
        }
    }

    pub fn status(&self, der: &[u8]) -> Status {
        match fs::read_to_string(self.registry()) {
            Ok(contents) if registry::contains(&contents, &Certificate::new(der)) => {
                Status::Installed
            }
            Ok(_) => Status::Missing,
            Err(_) => Status::Uninitialised,
        }
    }

    pub fn install(&self, der: &[u8]) -> Result<bool, Error> {
        let certificate = Certificate::new(der);
        let contents = self.load()?;

        if registry::contains(&contents, &certificate) {
            return Ok(false);
        }

        registry::write(&self.registry(), &registry::insert(&contents, &certificate))?;
        Ok(true)
    }

    pub fn uninstall(&self, der: &[u8]) -> Result<bool, Error> {
        let certificate = Certificate::new(der);
        let contents = self.load()?;

        if !registry::contains(&contents, &certificate) {
            return Ok(false);
        }

        registry::write(&self.registry(), &registry::remove(&contents, &certificate))?;
        Ok(true)
    }

    pub fn in_use(&self) -> bool {
        self.wineserver().is_some()
    }

    pub fn wineserver(&self) -> Option<u32> {
        let server = self.server_directory()?;

        fs::read_dir("/proc")
            .into_iter()
            .flatten()
            .flatten()
            .find(|process| {
                fs::read_link(process.path().join("cwd")).is_ok_and(|directory| directory == server)
            })
            .and_then(|process| process.file_name().to_str()?.parse().ok())
    }

    pub fn is_initialised(&self) -> bool {
        self.registry().is_file()
    }

    fn load(&self) -> Result<String, Error> {
        let contents = fs::read_to_string(self.registry())
            .map_err(|_| Error::Uninitialised(self.path.clone()))?;

        if self.in_use() {
            return Err(Error::InUse(self.path.clone()));
        }

        Ok(contents)
    }

    fn registry(&self) -> PathBuf {
        self.path.join(SYSTEM_REGISTRY)
    }

    fn server_directory(&self) -> Option<PathBuf> {
        let metadata = fs::metadata(&self.path).ok()?;
        let user = fs::metadata("/proc/self").ok()?.uid();

        Some(
            Path::new("/tmp")
                .join(format!(".wine-{user}"))
                .join(format!("server-{:x}-{:x}", metadata.dev(), metadata.ino())),
        )
    }
}
