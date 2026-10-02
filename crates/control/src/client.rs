use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

use crate::{Reply, Request, socket};

const PATIENCE: Duration = Duration::from_secs(120);

pub fn reachable() -> bool {
    UnixStream::connect(socket()).is_ok()
}

pub fn send(request: &Request) -> io::Result<Reply> {
    let mut stream = UnixStream::connect(socket())?;
    stream.set_read_timeout(Some(PATIENCE))?;

    let mut message = serde_json::to_vec(request)?;
    message.push(b'\n');
    stream.write_all(&message)?;

    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line)?;
    Ok(serde_json::from_str(&line)?)
}
