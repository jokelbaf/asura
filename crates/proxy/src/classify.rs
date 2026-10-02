use std::io;
use std::time::Duration;

use bytes::{Bytes, BytesMut};
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::time::timeout;

const PATIENCE: Duration = Duration::from_secs(3);

#[derive(Clone, Copy)]
pub enum Protocol {
    Tls,
    Http1,
    Http2,
    Unknown,
}

const SIGNATURES: [(&[u8], Protocol); 11] = [
    (&[0x16, 0x03], Protocol::Tls),
    (b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n", Protocol::Http2),
    (b"GET ", Protocol::Http1),
    (b"POST ", Protocol::Http1),
    (b"PUT ", Protocol::Http1),
    (b"HEAD ", Protocol::Http1),
    (b"DELETE ", Protocol::Http1),
    (b"OPTIONS ", Protocol::Http1),
    (b"PATCH ", Protocol::Http1),
    (b"CONNECT ", Protocol::Http1),
    (b"TRACE ", Protocol::Http1),
];

pub async fn classify(stream: &mut (impl AsyncRead + Unpin)) -> io::Result<(Protocol, Bytes)> {
    let mut buffer = BytesMut::with_capacity(4096);

    loop {
        let Ok(read) = timeout(PATIENCE, stream.read_buf(&mut buffer)).await else {
            return Ok((Protocol::Unknown, buffer.freeze()));
        };

        if read? == 0 {
            return Ok((Protocol::Unknown, buffer.freeze()));
        }

        if let Some(protocol) = detect(&buffer) {
            return Ok((protocol, buffer.freeze()));
        }
    }
}

fn detect(bytes: &[u8]) -> Option<Protocol> {
    let mut undecided = false;

    for (signature, protocol) in SIGNATURES {
        if bytes.starts_with(signature) {
            return Some(protocol);
        }

        undecided |= signature.starts_with(bytes);
    }

    (!undecided).then_some(Protocol::Unknown)
}
