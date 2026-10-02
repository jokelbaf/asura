use time::{Duration, OffsetDateTime};

use crate::{Entry, Mark, State};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Pending,
    Complete,
    Failed(String),
}

#[derive(Clone, Debug)]
pub struct Summary {
    pub id: u64,
    pub started: OffsetDateTime,
    pub process: String,
    pub pid: u32,
    pub protocol: String,
    pub method: String,
    pub host: String,
    pub path: String,
    pub url: String,
    pub status: Option<u16>,
    pub content_type: String,
    pub size: u64,
    pub duration: Option<Duration>,
    pub outcome: Outcome,
    pub decrypted: bool,
    pub websocket: bool,
    pub tunnel: bool,
    pub paused: bool,
    pub mocked: bool,
    pub edited: bool,
    pub composed: bool,
}

impl Entry {
    pub fn summary(&self) -> Summary {
        let connection = self.connection();
        let record = self.read();

        let tls = connection.tls.as_ref();
        let decrypted = tls.is_none_or(|tls| tls.decrypted);
        let tunnel = record.tunnel.is_some();
        let websocket = !record.frames.is_empty()
            || record
                .response
                .as_ref()
                .is_some_and(|response| response.status == 101);

        let fallback_host = tls
            .and_then(|tls| tls.server_name.clone())
            .unwrap_or_else(|| connection.server.to_string());

        let (method, host, path, url) = match &record.request {
            Some(request) => (
                request.method.clone(),
                request.authority.clone(),
                request.target.clone(),
                request.url(),
            ),
            None => (
                String::new(),
                fallback_host.clone(),
                String::new(),
                fallback_host,
            ),
        };

        let protocol = if websocket {
            String::from("WebSocket")
        } else if tunnel {
            String::from(match (tls.is_some(), decrypted) {
                (true, true) => "TLS (decrypted)",
                (true, false) => "TLS",
                (false, _) => "TCP",
            })
        } else {
            record
                .request
                .as_ref()
                .map(|request| request.version.as_str())
                .unwrap_or("HTTP")
                .to_owned()
        };

        let content_type = record
            .response
            .as_ref()
            .and_then(|response| response.headers.text("content-type"))
            .map(|value| {
                value
                    .split(';')
                    .next()
                    .unwrap_or_default()
                    .trim()
                    .to_owned()
            })
            .unwrap_or_default();

        let size = match (&record.response, &record.tunnel) {
            (Some(response), _) => response.body.len(),
            (None, Some(tunnel)) => tunnel.received_bytes,
            (None, None) => 0,
        };

        let finished = record
            .timing
            .response_complete
            .or(record.timing.request_complete);
        let marked = |wanted: fn(&Mark) -> bool| record.marks.iter().any(wanted);

        Summary {
            id: self.id(),
            started: record.timing.started,
            process: connection.client.name.clone(),
            pid: connection.client.process,
            protocol,
            method,
            host,
            path,
            url,
            status: record.response.as_ref().map(|response| response.status),
            content_type,
            size,
            duration: finished.map(|finished| finished - record.timing.started),
            outcome: match &record.state {
                State::Open => Outcome::Pending,
                State::Complete => Outcome::Complete,
                State::Failed(reason) => Outcome::Failed(reason.clone()),
            },
            decrypted,
            websocket,
            tunnel,
            paused: record.paused.is_some(),
            mocked: marked(|mark| matches!(mark, Mark::Mocked(_))),
            edited: marked(|mark| matches!(mark, Mark::RequestEdited | Mark::ResponseEdited)),
            composed: marked(|mark| *mark == Mark::Composed),
        }
    }
}
