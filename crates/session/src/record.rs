use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::{Body, Request, Response};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Direction {
    Outbound,
    Inbound,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Opcode {
    Continuation,
    Text,
    Binary,
    Close,
    Ping,
    Pong,
    Reserved(u8),
}

impl From<u8> for Opcode {
    fn from(value: u8) -> Self {
        match value {
            0x0 => Self::Continuation,
            0x1 => Self::Text,
            0x2 => Self::Binary,
            0x8 => Self::Close,
            0x9 => Self::Ping,
            0xa => Self::Pong,
            other => Self::Reserved(other),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Request,
    Response,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Mark {
    Composed,
    Mocked(String),
    RequestEdited,
    ResponseEdited,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Frame {
    pub direction: Direction,
    pub opcode: Opcode,
    pub payload: Vec<u8>,
    pub compressed: bool,
    pub time: OffsetDateTime,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Tunnel {
    #[serde(skip)]
    pub sent: Body,
    #[serde(skip)]
    pub received: Body,
    pub sent_bytes: u64,
    pub received_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum State {
    Open,
    Complete,
    Failed(String),
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Timing {
    pub started: OffsetDateTime,
    pub request_complete: Option<OffsetDateTime>,
    pub response_started: Option<OffsetDateTime>,
    pub response_complete: Option<OffsetDateTime>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Record {
    pub request: Option<Request>,
    pub response: Option<Response>,
    pub frames: Vec<Frame>,
    pub tunnel: Option<Tunnel>,
    pub state: State,
    pub timing: Timing,
    pub marks: Vec<Mark>,
    #[serde(skip)]
    pub paused: Option<Stage>,
}

impl Record {
    pub fn new(started: OffsetDateTime) -> Self {
        Self {
            request: None,
            response: None,
            frames: Vec::new(),
            tunnel: None,
            state: State::Open,
            timing: Timing {
                started,
                request_complete: None,
                response_started: None,
                response_complete: None,
            },
            marks: Vec::new(),
            paused: None,
        }
    }

    pub fn mark(&mut self, mark: Mark) {
        if !self.marks.contains(&mark) {
            self.marks.push(mark);
        }
    }

    pub fn fail(&mut self, reason: impl Into<String>) {
        if self.state == State::Open {
            self.state = State::Failed(reason.into());
        }
    }

    pub fn complete(&mut self) {
        if self.state == State::Open {
            self.state = State::Complete;
        }
    }
}
