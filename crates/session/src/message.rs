use serde::{Deserialize, Serialize};

use crate::{Body, Headers};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Scheme {
    Http,
    Https,
}

impl Scheme {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Http => "http",
            Self::Https => "https",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Version {
    Http10,
    Http11,
    Http2,
    Http3,
}

impl Version {
    pub fn parse(text: &str) -> Self {
        match text.to_ascii_lowercase().as_str() {
            "http/1.0" => Self::Http10,
            "http/2" | "http/2.0" | "h2" | "h2c" => Self::Http2,
            "http/3" | "http/3.0" | "h3" => Self::Http3,
            _ => Self::Http11,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Http10 => "HTTP/1.0",
            Self::Http11 => "HTTP/1.1",
            Self::Http2 => "HTTP/2",
            Self::Http3 => "HTTP/3",
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Request {
    pub method: String,
    pub scheme: Scheme,
    pub authority: String,
    pub target: String,
    pub version: Version,
    pub headers: Headers,
    pub trailers: Headers,
    #[serde(skip)]
    pub body: Body,
}

impl Request {
    pub fn url(&self) -> String {
        if self.target.contains("://") {
            return self.target.clone();
        }

        format!(
            "{}://{}{}",
            self.scheme.as_str(),
            self.authority,
            self.target
        )
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Response {
    pub status: u16,
    pub reason: String,
    pub version: Version,
    pub headers: Headers,
    pub trailers: Headers,
    #[serde(skip)]
    pub body: Body,
}
