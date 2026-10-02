use std::net::SocketAddr;
use std::time::Duration;

use bytes::BytesMut;
use rustls::server::Acceptor;
use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;
use tokio::time::timeout;

use crate::rewind::Rewind;

const LIMIT: usize = 64 << 10;
const PATIENCE: Duration = Duration::from_secs(10);

pub struct Hello {
    pub server_name: Option<String>,
    pub protocols: Vec<Vec<u8>>,
}

impl Hello {
    pub fn key(&self, server: SocketAddr) -> String {
        self.server_name
            .clone()
            .unwrap_or_else(|| server.to_string())
    }

    pub fn offered(&self) -> Vec<String> {
        self.protocols
            .iter()
            .map(|protocol| String::from_utf8_lossy(protocol).into_owned())
            .collect()
    }
}

pub async fn read(
    mut client: Rewind<TcpStream>,
) -> Result<(Hello, Rewind<TcpStream>), Rewind<TcpStream>> {
    let mut buffer = BytesMut::new();
    let hello = timeout(PATIENCE, parse(&mut client, &mut buffer))
        .await
        .ok()
        .flatten();

    let (unread, stream) = client.into_parts();
    buffer.extend_from_slice(&unread);
    let client = Rewind::new(buffer.freeze(), stream);

    match hello {
        Some(hello) => Ok((hello, client)),
        None => Err(client),
    }
}

async fn parse(client: &mut Rewind<TcpStream>, buffer: &mut BytesMut) -> Option<Hello> {
    let mut acceptor = Acceptor::default();

    loop {
        let start = buffer.len();

        if start > LIMIT || client.read_buf(buffer).await.ok()? == 0 {
            return None;
        }

        let mut fresh = &buffer[start..];
        while !fresh.is_empty() {
            acceptor.read_tls(&mut fresh).ok()?;
        }

        if let Some(accepted) = acceptor.accept().ok()? {
            let hello = accepted.client_hello();

            return Some(Hello {
                server_name: hello.server_name().map(str::to_owned),
                protocols: hello
                    .alpn()
                    .map(|protocols| protocols.map(<[u8]>::to_vec).collect())
                    .unwrap_or_default(),
            });
        }
    }
}
