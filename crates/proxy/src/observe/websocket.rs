use std::sync::Arc;

use bytes::{Buf, BytesMut};
use flate2::{Decompress, FlushDecompress, Status};
use session::{Direction, Entry, Frame, Headers, Opcode};
use time::OffsetDateTime;

use super::{Event, Observer, index};

const MAX_FRAME: u64 = 64 << 20;
const MAX_MESSAGE: usize = 16 << 20;
const TAIL: [u8; 4] = [0x00, 0x00, 0xff, 0xff];

pub struct WebSocket {
    entry: Arc<Entry>,
    sides: [Side; 2],
    broken: bool,
}

struct Side {
    direction: Direction,
    buffer: BytesMut,
    message: Option<Message>,
    inflater: Option<Inflater>,
}

struct Message {
    opcode: Opcode,
    compressed: bool,
    payload: Vec<u8>,
}

struct Inflater {
    state: Decompress,
    takeover: bool,
}

struct Header {
    fin: bool,
    compressed: bool,
    opcode: Opcode,
    mask: Option<[u8; 4]>,
    offset: usize,
    length: usize,
}

impl WebSocket {
    pub fn new(entry: Arc<Entry>, extensions: &Headers) -> Self {
        let deflate = extension(extensions, "permessage-deflate");

        let inflater = |no_takeover: &str| {
            deflate.as_ref().map(|parameters| Inflater {
                state: Decompress::new(false),
                takeover: !parameters.iter().any(|parameter| parameter == no_takeover),
            })
        };

        Self {
            entry,
            sides: [
                Side::new(Direction::Outbound, inflater("client_no_context_takeover")),
                Side::new(Direction::Inbound, inflater("server_no_context_takeover")),
            ],
            broken: false,
        }
    }

    fn process(&mut self, direction: Direction, time: OffsetDateTime) {
        let side = &mut self.sides[index(direction)];

        while let Some(header) = side.header() {
            if header.length as u64 > MAX_FRAME {
                self.broken = true;
                return;
            }

            if side.buffer.len() < header.offset + header.length {
                return;
            }

            side.buffer.advance(header.offset);
            let mut payload = side.buffer.split_to(header.length).to_vec();

            if let Some(mask) = header.mask {
                for (position, byte) in payload.iter_mut().enumerate() {
                    *byte ^= mask[position % 4];
                }
            }

            if let Some(frame) = side.accept(header, payload, time) {
                self.entry.update(|record, _| record.frames.push(frame));
            }
        }
    }
}

impl Side {
    fn new(direction: Direction, inflater: Option<Inflater>) -> Self {
        Self {
            direction,
            buffer: BytesMut::new(),
            message: None,
            inflater,
        }
    }

    fn header(&self) -> Option<Header> {
        let bytes = &self.buffer;

        if bytes.len() < 2 {
            return None;
        }

        let masked = bytes[1] & 0x80 != 0;

        let (length, mut offset) = match bytes[1] & 0x7f {
            126 if bytes.len() >= 4 => (u64::from(u16::from_be_bytes([bytes[2], bytes[3]])), 4),
            127 if bytes.len() >= 10 => (u64::from_be_bytes(bytes[2..10].try_into().ok()?), 10),
            126 | 127 => return None,
            short => (u64::from(short), 2),
        };

        let mask = if masked {
            let key = bytes.get(offset..offset + 4)?.try_into().ok()?;
            offset += 4;
            Some(key)
        } else {
            None
        };

        Some(Header {
            fin: bytes[0] & 0x80 != 0,
            compressed: bytes[0] & 0x40 != 0,
            opcode: Opcode::from(bytes[0] & 0x0f),
            mask,
            offset,
            length: usize::try_from(length).unwrap_or(usize::MAX),
        })
    }

    fn accept(&mut self, header: Header, payload: Vec<u8>, time: OffsetDateTime) -> Option<Frame> {
        if matches!(header.opcode, Opcode::Close | Opcode::Ping | Opcode::Pong) {
            return Some(self.frame(header.opcode, payload, false, time));
        }

        let message = self.message.get_or_insert(Message {
            opcode: header.opcode,
            compressed: header.compressed,
            payload: Vec::new(),
        });

        let room = MAX_MESSAGE.saturating_sub(message.payload.len());
        message
            .payload
            .extend_from_slice(&payload[..payload.len().min(room)]);

        if !header.fin {
            return None;
        }

        let message = self.message.take()?;
        let payload = if message.compressed {
            self.inflate(&message.payload).unwrap_or(message.payload)
        } else {
            message.payload
        };

        Some(self.frame(message.opcode, payload, message.compressed, time))
    }

    fn inflate(&mut self, payload: &[u8]) -> Option<Vec<u8>> {
        let inflater = self.inflater.as_mut()?;

        if !inflater.takeover {
            inflater.state.reset(false);
        }

        let input = [payload, &TAIL].concat();
        let mut output = Vec::new();
        let mut consumed = 0;

        loop {
            output.reserve(input.len().max(64 << 10));

            let (read, written) = (inflater.state.total_in(), inflater.state.total_out());
            let status = inflater
                .state
                .decompress_vec(&input[consumed..], &mut output, FlushDecompress::Sync)
                .ok()?;
            consumed += (inflater.state.total_in() - read) as usize;

            let stalled =
                inflater.state.total_in() == read && inflater.state.total_out() == written;

            if stalled || status == Status::StreamEnd || output.len() >= MAX_MESSAGE {
                return Some(output);
            }
        }
    }

    fn frame(
        &self,
        opcode: Opcode,
        payload: Vec<u8>,
        compressed: bool,
        time: OffsetDateTime,
    ) -> Frame {
        Frame {
            direction: self.direction,
            opcode,
            payload,
            compressed,
            time,
        }
    }
}

impl Observer for WebSocket {
    fn observe(&mut self, event: Event) {
        match event {
            Event::Data {
                direction,
                chunk,
                time,
            } if !self.broken => {
                self.sides[index(direction)]
                    .buffer
                    .extend_from_slice(&chunk);
                self.process(direction, time);
            }
            Event::Failed(reason) => self.entry.update(|record, _| record.fail(reason)),
            _ => {}
        }
    }

    fn close(&mut self) {
        self.entry.update(|record, _| record.complete());
    }
}

fn extension(headers: &Headers, name: &str) -> Option<Vec<String>> {
    headers
        .all("sec-websocket-extensions")
        .filter_map(|value| std::str::from_utf8(value).ok())
        .flat_map(|value| value.split(','))
        .map(|extension| extension.split(';').map(str::trim).collect::<Vec<_>>())
        .find(|parts| {
            parts
                .first()
                .is_some_and(|first| first.eq_ignore_ascii_case(name))
        })
        .map(|parts| {
            parts[1..]
                .iter()
                .map(|part| part.to_ascii_lowercase())
                .collect()
        })
}
