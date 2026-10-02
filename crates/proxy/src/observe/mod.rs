mod http1;
mod http2;
mod tunnel;
mod websocket;

use std::sync::Arc;

use bytes::Bytes;
use session::{Connection, Direction, Scheme, Store};
use time::OffsetDateTime;

pub use http1::Http1;
pub use http2::Http2;
pub use tunnel::Tunnel;
pub use websocket::WebSocket;

pub enum Event {
    Data {
        direction: Direction,
        chunk: Bytes,
        time: OffsetDateTime,
    },
    End {
        direction: Direction,
        time: OffsetDateTime,
    },
    Failed(String),
}

pub trait Observer: Send {
    fn observe(&mut self, event: Event);

    fn close(&mut self);
}

#[derive(Clone)]
pub struct Scope {
    pub store: Arc<Store>,
    pub connection: Arc<Connection>,
    pub scheme: Scheme,
}

pub fn index(direction: Direction) -> usize {
    match direction {
        Direction::Outbound => 0,
        Direction::Inbound => 1,
    }
}
