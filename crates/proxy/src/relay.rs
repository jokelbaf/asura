use std::io;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use bytes::BytesMut;
use session::Direction;
use time::OffsetDateTime;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, split};
use tokio::sync::mpsc::{self, Sender};

use crate::observe::{Event, Observer};

const CHUNK: usize = 64 << 10;
const BACKLOG: usize = 256;

pub struct Traffic {
    pub sent: u64,
    pub received: u64,
    pub elapsed: Duration,
}

impl Traffic {
    pub fn is_silent(&self) -> bool {
        self.sent == 0 && self.received == 0
    }
}

pub async fn relay<C, U>(client: C, upstream: U, mut observer: Box<dyn Observer>) -> Traffic
where
    C: AsyncRead + AsyncWrite + Unpin,
    U: AsyncRead + AsyncWrite + Unpin,
{
    let (client_reader, client_writer) = split(client);
    let (upstream_reader, upstream_writer) = split(upstream);
    let (events, mut receiver) = mpsc::channel(BACKLOG);
    let (sent, received) = (AtomicU64::new(0), AtomicU64::new(0));
    let started = Instant::now();

    let observing = tokio::spawn(async move {
        while let Some(event) = receiver.recv().await {
            observer.observe(event);
        }
        observer.close();
    });

    let outbound = pump(
        client_reader,
        upstream_writer,
        Direction::Outbound,
        &events,
        &sent,
    );
    let inbound = pump(
        upstream_reader,
        client_writer,
        Direction::Inbound,
        &events,
        &received,
    );

    if let Err(error) = tokio::try_join!(outbound, inbound) {
        let _ = events.send(Event::Failed(error.to_string())).await;
    }

    drop(events);
    let _ = observing.await;

    Traffic {
        sent: sent.into_inner(),
        received: received.into_inner(),
        elapsed: started.elapsed(),
    }
}

async fn pump<R, W>(
    mut reader: R,
    mut writer: W,
    direction: Direction,
    events: &Sender<Event>,
    total: &AtomicU64,
) -> io::Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut buffer = BytesMut::with_capacity(CHUNK);

    loop {
        buffer.reserve(CHUNK);

        let read = match reader.read_buf(&mut buffer).await {
            Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => 0,
            result => result?,
        };
        let time = OffsetDateTime::now_utc();

        if read == 0 {
            let _ = writer.shutdown().await;
            let _ = events.send(Event::End { direction, time }).await;
            return Ok(());
        }

        total.fetch_add(read as u64, Ordering::Relaxed);

        let chunk = buffer.split().freeze();
        writer.write_all(&chunk).await?;
        writer.flush().await?;

        let _ = events
            .send(Event::Data {
                direction,
                chunk,
                time,
            })
            .await;
    }
}
