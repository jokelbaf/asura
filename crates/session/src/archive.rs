use std::collections::{HashMap, HashSet};
use std::io::{self, Read, Write};
use std::path::Path;
use std::sync::Arc;

use serde::Serialize;

use crate::{Body, Connection, Entry, Record, State, Store};

const MAGIC: &[u8; 7] = b"ASURA\0\0";
const VERSION: u8 = 2;
const LEVEL: i32 = 3;
const CHUNK: usize = 64 << 10;

const CONNECTION: u8 = 1;
const EXCHANGE: u8 = 2;

const COMPLETE: u8 = 1;
const DECODED: u8 = 1 << 1;

pub fn save(entries: &[Arc<Entry>], writer: impl Write) -> io::Result<()> {
    let mut encoder = zstd::Encoder::new(writer, LEVEL)?;
    let mut written = HashSet::new();

    encoder.write_all(MAGIC)?;
    encoder.write_all(&[VERSION])?;

    for entry in entries {
        let connection = entry.connection();

        if written.insert(connection.id) {
            item(&mut encoder, CONNECTION, &**connection)?;
        }

        let record = entry.read();
        item(&mut encoder, EXCHANGE, &(connection.id, &*record))?;

        for body in bodies(&record) {
            blob(&mut encoder, body)?;
        }
    }

    encoder.finish()?.flush()
}

pub fn load(store: &Store, reader: impl Read) -> io::Result<usize> {
    let mut decoder = zstd::Decoder::new(reader)?;
    let mut magic = [0u8; MAGIC.len() + 1];
    decoder.read_exact(&mut magic)?;

    if &magic[..MAGIC.len()] != MAGIC {
        return Err(invalid("not an Asura session file"));
    }

    if magic[MAGIC.len()] != VERSION {
        return Err(invalid(
            "the session file was written by an incompatible version of Asura",
        ));
    }

    let mut connections = HashMap::new();
    let mut loaded = 0;

    while let Some(kind) = next(&mut decoder)? {
        let payload = payload(&mut decoder)?;

        match kind {
            CONNECTION => {
                let mut connection: Connection = postcard::from_bytes(&payload).map_err(invalid)?;
                let original = connection.id;

                connection.id = store.connection_id();
                connections.insert(original, Arc::new(connection));
            }
            EXCHANGE => {
                let (id, mut record): (u64, Record) =
                    postcard::from_bytes(&payload).map_err(invalid)?;

                for body in bodies_mut(&mut record) {
                    fill(body, &mut decoder, store.spool())?;
                }

                if record.state == State::Open {
                    record.fail("still in progress when the session was saved");
                }

                let connection = connections
                    .get(&id)
                    .cloned()
                    .ok_or_else(|| invalid("unknown connection"))?;
                store.restore(connection, record);
                loaded += 1;
            }
            _ => return Err(invalid("unknown item")),
        }
    }

    Ok(loaded)
}

fn item(writer: &mut impl Write, kind: u8, value: &impl Serialize) -> io::Result<()> {
    let payload = postcard::to_stdvec(value).map_err(invalid)?;

    writer.write_all(&[kind])?;
    writer.write_all(&(payload.len() as u32).to_le_bytes())?;
    writer.write_all(&payload)
}

fn blob(writer: &mut impl Write, body: &Body) -> io::Result<()> {
    let flags = [(body.is_complete(), COMPLETE), (body.is_decoded(), DECODED)]
        .into_iter()
        .filter(|(set, _)| *set)
        .fold(0, |flags, (_, flag)| flags | flag);

    writer.write_all(&[flags])?;
    writer.write_all(&body.len().to_le_bytes())?;
    body.write_to(writer)
}

fn next(reader: &mut impl Read) -> io::Result<Option<u8>> {
    let mut kind = [0u8];

    match reader.read_exact(&mut kind) {
        Ok(()) => Ok(Some(kind[0])),
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => Ok(None),
        Err(error) => Err(error),
    }
}

fn payload(reader: &mut impl Read) -> io::Result<Vec<u8>> {
    let mut length = [0u8; 4];
    reader.read_exact(&mut length)?;

    let mut payload = vec![0; u32::from_le_bytes(length) as usize];
    reader.read_exact(&mut payload)?;
    Ok(payload)
}

fn fill(body: &mut Body, reader: &mut impl Read, spool: &Path) -> io::Result<()> {
    let mut header = [0u8; 9];
    reader.read_exact(&mut header)?;

    let flags = header[0];
    let mut remaining = u64::from_le_bytes(header[1..].try_into().map_err(invalid)?);
    let mut buffer = vec![0; CHUNK];

    while remaining > 0 {
        let size = CHUNK.min(remaining as usize);
        reader.read_exact(&mut buffer[..size])?;
        body.append(&buffer[..size], spool)?;
        remaining -= size as u64;
    }

    if flags & COMPLETE != 0 {
        body.finish();
    }

    if flags & DECODED != 0 {
        body.mark_decoded();
    }

    Ok(())
}

fn bodies(record: &Record) -> Vec<&Body> {
    let tunnel = record
        .tunnel
        .iter()
        .flat_map(|tunnel| [&tunnel.sent, &tunnel.received]);

    record
        .request
        .iter()
        .map(|request| &request.body)
        .chain(record.response.iter().map(|response| &response.body))
        .chain(tunnel)
        .collect()
}

fn bodies_mut(record: &mut Record) -> Vec<&mut Body> {
    let tunnel = record
        .tunnel
        .iter_mut()
        .flat_map(|tunnel| [&mut tunnel.sent, &mut tunnel.received]);

    record
        .request
        .iter_mut()
        .map(|request| &mut request.body)
        .chain(
            record
                .response
                .iter_mut()
                .map(|response| &mut response.body),
        )
        .chain(tunnel)
        .collect()
}

fn invalid(error: impl ToString) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error.to_string())
}
