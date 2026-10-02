use std::fs::{self, Permissions};
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;

use crate::Error;

const ANCHOR: &str = "asura.crt";
const BEGIN: &str = "-----BEGIN CERTIFICATE-----";
const END: &str = "-----END CERTIFICATE-----";
const LINE: usize = 64;

pub struct System {
    anchors: &'static str,
    bundle: &'static str,
    refresh: &'static str,
}

const STORES: [System; 4] = [
    System {
        anchors: "/etc/ca-certificates/trust-source/anchors",
        bundle: "/etc/ssl/certs/ca-certificates.crt",
        refresh: "update-ca-trust",
    },
    System {
        anchors: "/etc/pki/ca-trust/source/anchors",
        bundle: "/etc/pki/ca-trust/extracted/pem/tls-ca-bundle.pem",
        refresh: "update-ca-trust",
    },
    System {
        anchors: "/etc/pki/trust/anchors",
        bundle: "/etc/ssl/ca-bundle.pem",
        refresh: "update-ca-certificates",
    },
    System {
        anchors: "/usr/local/share/ca-certificates",
        bundle: "/etc/ssl/certs/ca-certificates.crt",
        refresh: "update-ca-certificates",
    },
];

pub fn system() -> Option<&'static System> {
    STORES
        .iter()
        .find(|store| Path::new(store.anchors).is_dir())
}

impl System {
    pub fn trusts(&self, der: &[u8]) -> bool {
        let encoded = STANDARD.encode(der);

        fs::read_to_string(self.bundle)
            .is_ok_and(|bundle| blocks(&bundle).any(|block| block == encoded))
    }

    pub fn installed(&self) -> Option<SystemTime> {
        fs::metadata(self.anchor())
            .and_then(|metadata| metadata.modified())
            .ok()
    }

    pub fn install(&self, der: &[u8]) -> Result<(), Error> {
        let anchor = self.anchor();
        let staging = anchor.with_extension("partial");

        fs::write(&staging, pem(der))?;
        fs::set_permissions(&staging, Permissions::from_mode(0o644))?;
        fs::rename(&staging, &anchor)?;

        self.refresh()
    }

    pub fn uninstall(&self) -> Result<(), Error> {
        match fs::remove_file(self.anchor()) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error.into()),
            _ => self.refresh(),
        }
    }

    fn refresh(&self) -> Result<(), Error> {
        let output = Command::new(self.refresh).output()?;

        if output.status.success() {
            return Ok(());
        }

        Err(Error::Refresh {
            command: self.refresh,
            output: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        })
    }

    fn anchor(&self) -> PathBuf {
        Path::new(self.anchors).join(ANCHOR)
    }
}

pub fn pem(der: &[u8]) -> String {
    let encoded = STANDARD.encode(der);
    let mut pem = format!("{BEGIN}\n");

    for line in encoded.as_bytes().chunks(LINE) {
        pem.push_str(&String::from_utf8_lossy(line));
        pem.push('\n');
    }

    pem.push_str(END);
    pem.push('\n');
    pem
}

fn blocks(bundle: &str) -> impl Iterator<Item = String> + '_ {
    bundle.split(BEGIN).skip(1).filter_map(|block| {
        let (body, _) = block.split_once(END)?;
        Some(body.split_whitespace().collect())
    })
}
