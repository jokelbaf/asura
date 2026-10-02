use std::io::Write;
use std::process::Stdio;

use anyhow::{Context, ensure};

pub fn install(der: &[u8]) -> anyhow::Result<()> {
    let mut helper = ::capture::helper(&["trust", "install"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .context("starting the privileged helper")?;

    helper
        .stdin
        .take()
        .context("opening the helper input")?
        .write_all(der)?;

    let output = helper.wait_with_output()?;
    ensure!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(())
}

pub fn remove() -> anyhow::Result<()> {
    let output = ::capture::helper(&["trust", "remove"])
        .stdin(Stdio::null())
        .output()
        .context("starting the privileged helper")?;

    ensure!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(())
}
