use regex::bytes::{Regex, RegexBuilder};

use crate::{Body, Direction, Entry, Headers, Record};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Sides {
    #[default]
    Both,
    Requests,
    Responses,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Parts {
    #[default]
    All,
    Headers,
    Bodies,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Query {
    pub text: String,
    pub regex: bool,
    pub case_sensitive: bool,
    pub sides: Sides,
    pub parts: Parts,
}

pub struct Search {
    pattern: Regex,
    requests: bool,
    responses: bool,
    headers: bool,
    bodies: bool,
}

impl Search {
    pub fn new(query: &Query) -> Result<Self, regex::Error> {
        let expression = if query.regex {
            query.text.clone()
        } else {
            regex::escape(&query.text)
        };

        Ok(Self {
            pattern: RegexBuilder::new(&expression)
                .case_insensitive(!query.case_sensitive)
                .build()?,
            requests: query.sides != Sides::Responses,
            responses: query.sides != Sides::Requests,
            headers: query.parts != Parts::Bodies,
            bodies: query.parts != Parts::Headers,
        })
    }

    pub fn matches(&self, entry: &Entry) -> bool {
        let record = entry.read();

        (self.requests && self.request(entry, &record))
            || (self.responses && self.response(&record))
    }

    fn request(&self, entry: &Entry, record: &Record) -> bool {
        let request = record.request.as_ref();

        if self.headers {
            let line = match request {
                Some(request) => format!(
                    "{} {} {}",
                    request.method,
                    request.url(),
                    request.version.as_str()
                ),
                None => {
                    let connection = entry.connection();

                    connection
                        .tls
                        .as_ref()
                        .and_then(|tls| tls.server_name.clone())
                        .unwrap_or_else(|| connection.server.to_string())
                }
            };

            if self.find(line.as_bytes())
                || request.is_some_and(|request| {
                    self.fields(&request.headers) || self.fields(&request.trailers)
                })
            {
                return true;
            }
        }

        self.bodies
            && (request.is_some_and(|request| self.body(&request.body, Some(&request.headers)))
                || self.frames(record, Direction::Outbound)
                || record
                    .tunnel
                    .as_ref()
                    .is_some_and(|tunnel| self.body(&tunnel.sent, None)))
    }

    fn response(&self, record: &Record) -> bool {
        let response = record.response.as_ref();

        if self.headers
            && let Some(response) = response
        {
            let line = format!(
                "{} {} {}",
                response.version.as_str(),
                response.status,
                response.reason
            );

            if self.find(line.as_bytes())
                || self.fields(&response.headers)
                || self.fields(&response.trailers)
            {
                return true;
            }
        }

        self.bodies
            && (response.is_some_and(|response| self.body(&response.body, Some(&response.headers)))
                || self.frames(record, Direction::Inbound)
                || record
                    .tunnel
                    .as_ref()
                    .is_some_and(|tunnel| self.body(&tunnel.received, None)))
    }

    fn fields(&self, headers: &Headers) -> bool {
        headers.iter().any(|(name, value)| {
            let mut line = Vec::with_capacity(name.len() + value.len() + 2);
            line.extend_from_slice(name.as_bytes());
            line.extend_from_slice(b": ");
            line.extend_from_slice(value);
            self.find(&line)
        })
    }

    fn body(&self, body: &Body, headers: Option<&Headers>) -> bool {
        let content = match headers {
            Some(headers) => body.content(headers).or_else(|_| body.bytes()),
            None => body.bytes(),
        };

        content.is_ok_and(|content| self.find(&content))
    }

    fn frames(&self, record: &Record, direction: Direction) -> bool {
        record
            .frames
            .iter()
            .filter(|frame| frame.direction == direction)
            .any(|frame| self.find(&frame.payload))
    }

    fn find(&self, haystack: &[u8]) -> bool {
        self.pattern.is_match(haystack)
    }
}
