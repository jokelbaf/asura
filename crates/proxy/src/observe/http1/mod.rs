mod framing;
mod parse;

use std::collections::VecDeque;
use std::mem;
use std::sync::Arc;

use bytes::{Buf, BytesMut};
use session::{Direction, Entry, Request, Response};
use time::OffsetDateTime;

use framing::{Chunk, Framing};
use parse::Parsed;

use super::{Event, Observer, Scope, Tunnel, WebSocket, index};

const MAX_HEAD: usize = 256 << 10;
const MAX_LINE: usize = 8 << 10;

pub struct Http1 {
    scope: Scope,
    sides: [Side; 2],
    waiting: VecDeque<Waiting>,
    upgraded: Option<Box<dyn Observer>>,
}

struct Waiting {
    entry: Arc<Entry>,
    head: bool,
    connect: bool,
    websocket: bool,
}

#[derive(Default)]
struct Side {
    buffer: BytesMut,
    phase: Phase,
}

#[derive(Default)]
enum Phase {
    #[default]
    Head,
    Body {
        entry: Arc<Entry>,
        framing: Framing,
    },
    Held,
}

enum Step {
    Continue,
    Wait,
    Upgrade(Box<dyn Observer>),
    Broken,
}

impl Http1 {
    pub fn new(scope: Scope) -> Self {
        Self {
            scope,
            sides: Default::default(),
            waiting: VecDeque::new(),
            upgraded: None,
        }
    }

    fn process(&mut self, direction: Direction, time: OffsetDateTime) {
        loop {
            let step = match (&self.sides[index(direction)].phase, direction) {
                (Phase::Head, Direction::Outbound) => self.request_head(time),
                (Phase::Head, Direction::Inbound) => self.response_head(time),
                (Phase::Body { .. }, _) => self.body(direction, time),
                (Phase::Held, _) => Step::Wait,
            };

            match step {
                Step::Continue => {}
                Step::Wait => return,
                Step::Upgrade(observer) => return self.upgrade(observer, time),
                Step::Broken => {
                    self.abandon("malformed HTTP/1 message");
                    return self.upgrade(Box::new(Tunnel::new(&self.scope)), time);
                }
            }
        }
    }

    fn request_head(&mut self, time: OffsetDateTime) -> Step {
        let side = &mut self.sides[0];

        let head = match parse::request(&side.buffer) {
            Parsed::Complete(head) => head,
            Parsed::Partial if side.buffer.len() <= MAX_HEAD => return Step::Wait,
            _ => return Step::Broken,
        };

        side.buffer.advance(head.length);

        let framing = framing::request(&head.headers);
        let connect = head.method.eq_ignore_ascii_case("CONNECT");
        let upgrade = head.headers.contains_token("connection", "upgrade");
        let waiting = Waiting {
            entry: self.scope.store.open(self.scope.connection.clone()),
            head: head.method.eq_ignore_ascii_case("HEAD"),
            connect,
            websocket: head.headers.contains_token("upgrade", "websocket"),
        };

        let authority = head
            .headers
            .text("host")
            .map(str::to_owned)
            .unwrap_or_else(|| self.scope.connection.server.to_string());

        let complete = framing.is_none();
        waiting.entry.update(|record, _| {
            let mut request = Request {
                method: head.method,
                scheme: self.scope.scheme,
                authority,
                target: head.target,
                version: head.version,
                headers: head.headers,
                trailers: Default::default(),
                body: Default::default(),
            };

            if complete {
                request.body.finish();
                record.timing.request_complete = Some(time);
            }

            record.request = Some(request);
        });

        side.phase = match framing {
            Some(framing) => Phase::Body {
                entry: waiting.entry.clone(),
                framing,
            },
            None if upgrade || connect => Phase::Held,
            None => Phase::Head,
        };

        self.waiting.push_back(waiting);
        Step::Continue
    }

    fn response_head(&mut self, time: OffsetDateTime) -> Step {
        let side = &mut self.sides[1];

        let head = match parse::response(&side.buffer) {
            Parsed::Complete(head) => head,
            Parsed::Partial if side.buffer.len() <= MAX_HEAD => return Step::Wait,
            _ => return Step::Broken,
        };

        if self.waiting.is_empty() {
            return Step::Broken;
        }

        side.buffer.advance(head.length);

        if head.status / 100 == 1 && head.status != 101 {
            return Step::Continue;
        }

        let Some(waiting) = self.waiting.pop_front() else {
            return Step::Broken;
        };

        let switching = head.status == 101 || (waiting.connect && head.status / 100 == 2);
        let framing = framing::response(head.status, waiting.head, &head.headers);
        let extensions = head.headers.clone();

        waiting.entry.update(|record, _| {
            record.response = Some(Response {
                status: head.status,
                reason: head.reason,
                version: head.version,
                headers: head.headers,
                trailers: Default::default(),
                body: Default::default(),
            });
            record.timing.response_started = Some(time);
        });

        if switching {
            waiting.entry.update(|record, _| {
                if let Some(response) = &mut record.response {
                    response.body.finish();
                }
                record.timing.response_complete = Some(time);
            });

            let observer: Box<dyn Observer> = if waiting.websocket && head.status == 101 {
                Box::new(WebSocket::new(waiting.entry, &extensions))
            } else {
                Box::new(Tunnel::attach(waiting.entry))
            };

            return Step::Upgrade(observer);
        }

        if matches!(self.sides[0].phase, Phase::Held) {
            self.sides[0].phase = Phase::Head;
        }

        self.sides[1].phase = match framing {
            Some(framing) => Phase::Body {
                entry: waiting.entry,
                framing,
            },
            None => {
                finish_response(&waiting.entry, time);
                Phase::Head
            }
        };

        Step::Continue
    }

    fn body(&mut self, direction: Direction, time: OffsetDateTime) -> Step {
        let Side { buffer, phase } = &mut self.sides[index(direction)];

        let Phase::Body { entry, framing } = phase else {
            return Step::Wait;
        };

        match framing {
            Framing::Length(remaining) => {
                if !consume(entry, direction, buffer, remaining) {
                    return Step::Wait;
                }
            }
            Framing::Close => {
                append(entry, direction, &buffer.split());
                return Step::Wait;
            }
            Framing::Chunked(chunk) => match chunk {
                Chunk::Size => {
                    let Some(line) = framing::take_line(buffer) else {
                        return if buffer.len() > MAX_LINE {
                            Step::Broken
                        } else {
                            Step::Wait
                        };
                    };
                    let Some(size) = framing::chunk_size(&line) else {
                        return Step::Broken;
                    };

                    *chunk = if size == 0 {
                        Chunk::Trailers
                    } else {
                        Chunk::Data(size)
                    };
                    return Step::Continue;
                }
                Chunk::Data(remaining) => {
                    if consume(entry, direction, buffer, remaining) {
                        *chunk = Chunk::Delimiter;
                        return Step::Continue;
                    }
                    return Step::Wait;
                }
                Chunk::Delimiter => {
                    if buffer.len() < 2 {
                        return Step::Wait;
                    }

                    buffer.advance(2);
                    *chunk = Chunk::Size;
                    return Step::Continue;
                }
                Chunk::Trailers => {
                    let Some(line) = framing::take_line(buffer) else {
                        return if buffer.len() > MAX_LINE {
                            Step::Broken
                        } else {
                            Step::Wait
                        };
                    };

                    if !line.is_empty() {
                        if let Some((name, value)) = parse::trailer(&line) {
                            trailer(entry, direction, name, value);
                        }
                        return Step::Continue;
                    }
                }
            },
        }

        self.finish(direction, time);
        Step::Continue
    }

    fn finish(&mut self, direction: Direction, time: OffsetDateTime) {
        let side = &mut self.sides[index(direction)];

        let Phase::Body { entry, .. } = mem::take(&mut side.phase) else {
            return;
        };

        match direction {
            Direction::Outbound => entry.update(|record, _| {
                if let Some(request) = &mut record.request {
                    request.body.finish();
                }
                record.timing.request_complete = Some(time);
            }),
            Direction::Inbound => finish_response(&entry, time),
        }
    }

    fn upgrade(&mut self, mut observer: Box<dyn Observer>, time: OffsetDateTime) {
        for direction in [Direction::Outbound, Direction::Inbound] {
            let chunk = self.sides[index(direction)].buffer.split().freeze();

            if !chunk.is_empty() {
                observer.observe(Event::Data {
                    direction,
                    chunk,
                    time,
                });
            }
        }

        self.upgraded = Some(observer);
    }

    fn end(&mut self, direction: Direction, time: OffsetDateTime) {
        if direction == Direction::Outbound {
            return;
        }

        if let Phase::Body {
            framing: Framing::Close,
            ..
        } = self.sides[1].phase
        {
            self.finish(Direction::Inbound, time);
        }

        self.abandon("connection closed before the response completed");
    }

    fn abandon(&mut self, reason: &str) {
        for side in &mut self.sides {
            if let Phase::Body { entry, .. } = mem::take(&mut side.phase) {
                entry.update(|record, _| record.fail(reason));
            }
        }

        for waiting in self.waiting.drain(..) {
            waiting.entry.update(|record, _| record.fail(reason));
        }
    }
}

impl Observer for Http1 {
    fn observe(&mut self, event: Event) {
        if let Some(upgraded) = &mut self.upgraded {
            return upgraded.observe(event);
        }

        match event {
            Event::Data {
                direction,
                chunk,
                time,
            } => {
                self.sides[index(direction)]
                    .buffer
                    .extend_from_slice(&chunk);
                self.process(direction, time);

                if self.upgraded.is_none() {
                    self.process(opposite(direction), time);
                }
            }
            Event::End { direction, time } => self.end(direction, time),
            Event::Failed(reason) => self.abandon(&reason),
        }
    }

    fn close(&mut self) {
        match &mut self.upgraded {
            Some(upgraded) => upgraded.close(),
            None => self.abandon("connection closed"),
        }
    }
}

fn consume(
    entry: &Entry,
    direction: Direction,
    buffer: &mut BytesMut,
    remaining: &mut u64,
) -> bool {
    let length = (*remaining).min(buffer.len() as u64) as usize;

    if length > 0 {
        append(entry, direction, &buffer.split_to(length));
        *remaining -= length as u64;
    }

    *remaining == 0
}

fn append(entry: &Entry, direction: Direction, chunk: &[u8]) {
    if chunk.is_empty() {
        return;
    }

    entry.update(|record, spool| {
        let body = match direction {
            Direction::Outbound => record.request.as_mut().map(|request| &mut request.body),
            Direction::Inbound => record.response.as_mut().map(|response| &mut response.body),
        };

        if let Some(body) = body {
            let _ = body.append(chunk, spool);
        }
    });
}

fn trailer(entry: &Entry, direction: Direction, name: String, value: Vec<u8>) {
    entry.update(|record, _| {
        let trailers = match direction {
            Direction::Outbound => record.request.as_mut().map(|request| &mut request.trailers),
            Direction::Inbound => record
                .response
                .as_mut()
                .map(|response| &mut response.trailers),
        };

        if let Some(trailers) = trailers {
            trailers.push(name, value);
        }
    });
}

fn finish_response(entry: &Entry, time: OffsetDateTime) {
    entry.update(|record, _| {
        if let Some(response) = &mut record.response {
            response.body.finish();
        }
        record.timing.response_complete = Some(time);
        record.complete();
    });
}

fn opposite(direction: Direction) -> Direction {
    match direction {
        Direction::Outbound => Direction::Inbound,
        Direction::Inbound => Direction::Outbound,
    }
}
