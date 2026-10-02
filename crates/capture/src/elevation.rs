use std::env;
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

use rustix::process::geteuid;

use crate::{Bundle, Error};

const HELPER: &str = "asura-helper";
const INSTALLED: &str = "/usr/lib/asura/asura-helper";

pub fn acquire() -> Result<Bundle, Error> {
    let (channel, remote) = UnixStream::pair()?;
    let helper = spawn(remote)?;

    let bundle = Bundle::receive(&channel);
    let output = helper.wait_with_output()?;

    match bundle {
        Ok(bundle) if output.status.success() => Ok(bundle),
        _ => Err(Error::Helper(
            String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        )),
    }
}

pub fn helper(arguments: &[&str]) -> Command {
    let helper = locate();

    let mut command = if geteuid().is_root() {
        Command::new(helper)
    } else {
        let mut command = Command::new("pkexec");
        command.arg(helper);
        command
    };

    command.args(arguments);
    command
}

fn spawn(channel: UnixStream) -> Result<Child, Error> {
    Ok(helper(&["capture"])
        .stdin(OwnedFd::from(channel))
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?)
}

fn locate() -> PathBuf {
    env::current_exe()
        .ok()
        .and_then(|executable| Some(executable.parent()?.join(HELPER)))
        .filter(|candidate| candidate.exists())
        .unwrap_or_else(|| PathBuf::from(INSTALLED))
}
