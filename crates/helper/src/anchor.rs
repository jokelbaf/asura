use std::io::{self, Read};

use anyhow::{Context, anyhow, ensure};
use trust::System;
use x509_parser::prelude::{FromDer, X509Certificate};

const LIMIT: u64 = 64 * 1024;

pub fn install() -> anyhow::Result<()> {
    let mut der = Vec::new();
    io::stdin().take(LIMIT).read_to_end(&mut der)?;

    let (rest, certificate) = X509Certificate::from_der(&der)
        .map_err(|_| anyhow!("the input is not a DER certificate"))?;
    ensure!(
        rest.is_empty() && certificate.is_ca(),
        "the input is not a certificate authority"
    );

    store()?.install(&der)?;
    Ok(())
}

pub fn remove() -> anyhow::Result<()> {
    store()?.uninstall()?;
    Ok(())
}

fn store() -> anyhow::Result<&'static System> {
    trust::system().context("no supported system trust store was found")
}
