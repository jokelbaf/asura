mod anchor;
mod link;
mod loader;

use std::env;
use std::io;
use std::os::fd::AsFd;
use std::os::unix::net::UnixStream;
use std::process::ExitCode;

use anyhow::bail;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> anyhow::Result<()> {
    let arguments: Vec<String> = env::args().skip(1).collect();
    let arguments: Vec<&str> = arguments.iter().map(String::as_str).collect();

    match arguments.as_slice() {
        ["capture"] => capture(),
        ["trust", "install"] => anchor::install(),
        ["trust", "remove"] => anchor::remove(),
        _ => bail!("usage: asura-helper capture | trust install | trust remove"),
    }
}

fn capture() -> anyhow::Result<()> {
    let channel = UnixStream::from(io::stdin().as_fd().try_clone_to_owned()?);

    loader::load()?.send(&channel)?;
    Ok(())
}
