use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;

use proxy::ResponseDraft;
use session::Entry;

const DIRECTORY: &str = "responses";

pub fn save(entry: &Entry) -> io::Result<PathBuf> {
    let record = entry.read();
    let response = record
        .response
        .as_ref()
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "the session has no response"))?;

    let draft = ResponseDraft::capture(response);
    let directory = ::control::data().join(DIRECTORY);
    fs::create_dir_all(&directory)?;

    let stamp = time::OffsetDateTime::now_utc().unix_timestamp_nanos();
    let path = directory.join(format!("{stamp}-{}.http", entry.id()));
    let mut file = fs::File::create(&path)?;

    write!(file, "HTTP/1.1 {} {}\r\n", draft.status, response.reason)?;

    for (name, value) in draft.headers.iter() {
        file.write_all(name.as_bytes())?;
        file.write_all(b": ")?;
        file.write_all(value)?;
        file.write_all(b"\r\n")?;
    }

    write!(file, "content-length: {}\r\n\r\n", draft.body.len())?;
    file.write_all(&draft.body)?;
    file.flush()?;

    Ok(path)
}
