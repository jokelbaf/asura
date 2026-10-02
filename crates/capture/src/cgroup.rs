use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use crate::Error;

const ROOT: &str = "/sys/fs/cgroup";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cgroup {
    pub id: u64,
    pub path: PathBuf,
}

impl Cgroup {
    pub fn of(process: u32) -> Result<Self, Error> {
        let membership = fs::read_to_string(format!("/proc/{process}/cgroup"))?;
        let relative = membership
            .lines()
            .find_map(|line| line.strip_prefix("0::"))
            .ok_or(Error::Membership(process))?;

        let path = Path::new(ROOT).join(relative.trim_start_matches('/'));

        Ok(Self {
            id: fs::metadata(&path)?.ino(),
            path,
        })
    }

    pub fn exists(&self) -> bool {
        fs::metadata(&self.path).is_ok_and(|metadata| metadata.ino() == self.id)
    }
}
