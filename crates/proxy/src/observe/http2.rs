use std::collections::HashMap;
use std::sync::Arc;

use bytes::{Buf, Bytes, BytesMut};
use session::{Direction, Entry, Headers, Request, Response, Scheme, Version};
use time::OffsetDateTime;

use super::{Event, Observer, Scope, index};
use crate::hpack::{Decoder, Field};

const PREFACE: &[u8] = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";
const FRAME_HEADER: usize = 9;
const MAX_FRAME: usize = 16 << 20;

const DATA: u8 = 0x0;
const HEADERS: u8 = 0x1;
const RST_STREAM: u8 = 0x3;
const PUSH_PROMISE: u8 = 0x5;
const CONTINUATION: u8 = 0x9;

const END_STREAM: u8 = 0x1;
const END_HEADERS: u8 = 0x4;
const PADDED: u8 = 0x8;
const PRIORITY: u8 = 0x20;

const ERRORS: [&str; 14] = [
    "NO_ERROR",
    "PROTOCOL_ERROR",
    "INTERNAL_ERROR",
    "FLOW_CONTROL_ERROR",
    "SETTINGS_TIMEOUT",
    "STREAM_CLOSED",
    "FRAME_SIZE_ERROR",
    "REFUSED_STREAM",
    "CANCEL",
    "COMPRESSION_ERROR",
    "CONNECT_ERROR",
    "ENHANCE_YOUR_CALM",
    "INADEQUATE_SECURITY",
    "HTTP_1_1_REQUIRED",
];

pub struct Http2 {
    scope: Scope,
    sides: [Side; 2],
    streams: HashMap<u32, Arc<Entry>>,
    broken: bool,
}

#[derive(Default)]
struct Side {
    buffer: BytesMut,
    decoder: Decoder,
    preface: bool,
    block: Option<Block>,
}

struct Block {
    stream: u32,
    promised: Option<u32>,
    end_stream: bool,
    fragment: Vec<u8>,
}

struct Frame {
    kind: u8,
    flags: u8,
    stream: u32,
    payload: Bytes,
}

impl Frame {
    fn has(&self, flag: u8) -> bool {
        self.flags & flag != 0
    }

    fn content(&self) -> Option<Bytes> {
        if !self.has(PADDED) {
            return Some(self.payload.clone());
        }

        let padding = usize::from(*self.payload.first()?);
        let end = self.payload.len().checked_sub(padding)?;
        (end >= 1).then(|| self.payload.slice(1..end))
    }
}

impl Http2 {
    pub fn new(scope: Scope) -> Self {
        let mut sides: [Side; 2] = Default::default();
        sides[0].preface = true;

        Self {
            scope,
            sides,
            streams: HashMap::new(),
            broken: false,
        }
    }

    fn process(&mut self, direction: Direction, time: OffsetDateTime) {
        let side = &mut self.sides[index(direction)];

        if side.preface {
            if side.buffer.len() < PREFACE.len() {
                self.broken = !PREFACE.starts_with(&side.buffer);
                return;
            }

            if !side.buffer.starts_with(PREFACE) {
                self.broken = true;
                return;
            }

            side.buffer.advance(PREFACE.len());
            side.preface = false;
        }

        while !self.broken {
            let Some(frame) = self.next_frame(direction) else {
                return;
            };
            self.frame(direction, frame, time);
        }
    }

    fn next_frame(&mut self, direction: Direction) -> Option<Frame> {
        let buffer = &mut self.sides[index(direction)].buffer;

        if buffer.len() < FRAME_HEADER {
            return None;
        }

        let length =
            usize::from(buffer[0]) << 16 | usize::from(buffer[1]) << 8 | usize::from(buffer[2]);

        if length > MAX_FRAME {
            self.broken = true;
            return None;
        }

        if buffer.len() < FRAME_HEADER + length {
            return None;
        }

        let header = buffer.split_to(FRAME_HEADER);
        let stream = u32::from_be_bytes([header[5], header[6], header[7], header[8]]) & 0x7fff_ffff;

        Some(Frame {
            kind: header[3],
            flags: header[4],
            stream,
            payload: buffer.split_to(length).freeze(),
        })
    }

    fn frame(&mut self, direction: Direction, frame: Frame, time: OffsetDateTime) {
        match frame.kind {
            DATA => {
                if let Some(data) = frame.content() {
                    self.data(direction, frame.stream, &data, frame.has(END_STREAM), time);
                }
            }
            HEADERS => {
                let Some(mut fragment) = frame.content() else {
                    return;
                };

                if frame.has(PRIORITY) {
                    if fragment.len() < 5 {
                        return;
                    }
                    fragment.advance(5);
                }

                let block = Block {
                    stream: frame.stream,
                    promised: None,
                    end_stream: frame.has(END_STREAM),
                    fragment: fragment.to_vec(),
                };
                self.begin(direction, block, frame.has(END_HEADERS), time);
            }
            PUSH_PROMISE => {
                let Some(mut fragment) = frame.content().filter(|content| content.len() >= 4)
                else {
                    return;
                };

                let promised = fragment.get_u32() & 0x7fff_ffff;
                let block = Block {
                    stream: frame.stream,
                    promised: Some(promised),
                    end_stream: false,
                    fragment: fragment.to_vec(),
                };
                self.begin(direction, block, frame.has(END_HEADERS), time);
            }
            CONTINUATION => {
                let side = &mut self.sides[index(direction)];

                if let Some(block) = &mut side.block {
                    block.fragment.extend_from_slice(&frame.payload);
                }

                if frame.has(END_HEADERS)
                    && let Some(block) = side.block.take()
                {
                    self.headers(direction, block, time);
                }
            }
            RST_STREAM => {
                let code = frame
                    .payload
                    .get(..4)
                    .map(|bytes| u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
                    .unwrap_or_default();

                if let Some(entry) = self.streams.remove(&frame.stream) {
                    let name = ERRORS.get(code as usize).copied().unwrap_or("UNKNOWN");
                    entry.update(|record, _| record.fail(format!("stream reset ({name})")));
                }
            }
            _ => {}
        }
    }

    fn begin(&mut self, direction: Direction, block: Block, complete: bool, time: OffsetDateTime) {
        if complete {
            self.headers(direction, block, time);
        } else {
            self.sides[index(direction)].block = Some(block);
        }
    }

    fn headers(&mut self, direction: Direction, block: Block, time: OffsetDateTime) {
        let Ok(fields) = self.sides[index(direction)].decoder.decode(&block.fragment) else {
            self.broken = true;
            self.abandon("HPACK decoding failed");
            return;
        };

        if let Some(promised) = block.promised {
            let entry = self.open(fields, time);
            entry.update(|record, _| record.timing.request_complete = Some(time));
            self.streams.insert(promised, entry);
            return;
        }

        let entry = match (direction, self.streams.get(&block.stream)) {
            (Direction::Outbound, None) => {
                let entry = self.open(fields, time);
                self.streams.insert(block.stream, entry.clone());
                entry
            }
            (_, Some(entry)) => {
                let entry = entry.clone();
                if !self.respond(&entry, direction, fields, time) {
                    return;
                }
                entry
            }
            (Direction::Inbound, None) => return,
        };

        if block.end_stream {
            self.end_stream(direction, block.stream, &entry, time);
        }
    }

    fn respond(
        &self,
        entry: &Entry,
        direction: Direction,
        fields: Vec<Field>,
        time: OffsetDateTime,
    ) -> bool {
        let (pseudo, headers) = split(fields);

        entry.update(|record, _| {
            if direction == Direction::Outbound {
                if let Some(request) = &mut record.request {
                    request.trailers = headers;
                }
                return true;
            }

            if let Some(response) = &mut record.response {
                response.trailers = headers;
                return true;
            }

            let status = lookup(&pseudo, ":status")
                .and_then(|status| status.parse().ok())
                .unwrap_or_default();

            if status / 100 == 1 {
                return false;
            }

            record.response = Some(Response {
                status,
                reason: String::new(),
                version: Version::Http2,
                headers,
                trailers: Headers::default(),
                body: Default::default(),
            });
            record.timing.response_started = Some(time);
            true
        })
    }

    fn data(
        &mut self,
        direction: Direction,
        stream: u32,
        data: &[u8],
        end_stream: bool,
        time: OffsetDateTime,
    ) {
        let Some(entry) = self.streams.get(&stream).cloned() else {
            return;
        };

        if !data.is_empty() {
            entry.update(|record, spool| {
                let body = match direction {
                    Direction::Outbound => record.request.as_mut().map(|request| &mut request.body),
                    Direction::Inbound => {
                        record.response.as_mut().map(|response| &mut response.body)
                    }
                };

                if let Some(body) = body {
                    let _ = body.append(data, spool);
                }
            });
        }

        if end_stream {
            self.end_stream(direction, stream, &entry, time);
        }
    }

    fn end_stream(
        &mut self,
        direction: Direction,
        stream: u32,
        entry: &Entry,
        time: OffsetDateTime,
    ) {
        match direction {
            Direction::Outbound => entry.update(|record, _| {
                if let Some(request) = &mut record.request {
                    request.body.finish();
                }
                record.timing.request_complete = Some(time);
            }),
            Direction::Inbound => {
                entry.update(|record, _| {
                    if let Some(response) = &mut record.response {
                        response.body.finish();
                    }
                    record.timing.response_complete = Some(time);
                    record.complete();
                });
                self.streams.remove(&stream);
            }
        }
    }

    fn open(&self, fields: Vec<Field>, time: OffsetDateTime) -> Arc<Entry> {
        let (pseudo, headers) = split(fields);
        let method = lookup(&pseudo, ":method").unwrap_or("GET").to_owned();

        let authority = lookup(&pseudo, ":authority")
            .map(str::to_owned)
            .or_else(|| headers.text("host").map(str::to_owned))
            .unwrap_or_else(|| self.scope.connection.server.to_string());

        let target = lookup(&pseudo, ":path")
            .map(str::to_owned)
            .unwrap_or_else(|| authority.clone());

        let scheme = match lookup(&pseudo, ":scheme") {
            Some("http") => Scheme::Http,
            Some(_) => Scheme::Https,
            None => self.scope.scheme,
        };

        let entry = self.scope.store.open(self.scope.connection.clone());
        entry.update(|record, _| {
            record.timing.started = time;
            record.request = Some(Request {
                method,
                scheme,
                authority,
                target,
                version: Version::Http2,
                headers,
                trailers: Headers::default(),
                body: Default::default(),
            });
        });
        entry
    }

    fn abandon(&mut self, reason: &str) {
        for (_, entry) in self.streams.drain() {
            entry.update(|record, _| record.fail(reason));
        }
    }
}

impl Observer for Http2 {
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
            Event::Failed(reason) => self.abandon(&reason),
            _ => {}
        }
    }

    fn close(&mut self) {
        self.abandon("connection closed");
    }
}

fn split(fields: Vec<Field>) -> (Vec<(String, String)>, Headers) {
    let mut pseudo = Vec::new();
    let mut headers = Headers::default();

    for (name, value) in fields {
        let name = String::from_utf8_lossy(&name).into_owned();

        if name.starts_with(':') {
            pseudo.push((name, String::from_utf8_lossy(&value).into_owned()));
        } else {
            headers.push(name, value);
        }
    }

    (pseudo, headers)
}

fn lookup<'a>(pseudo: &'a [(String, String)], name: &str) -> Option<&'a str> {
    pseudo
        .iter()
        .find(|(field, _)| field == name)
        .map(|(_, value)| value.as_str())
}
