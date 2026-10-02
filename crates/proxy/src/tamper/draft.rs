use std::borrow::Cow;

use session::{Body, Headers, Request, Response};

const FRAMING: [&str; 2] = ["content-length", "transfer-encoding"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RequestDraft {
    pub method: String,
    pub target: String,
    pub headers: Headers,
    pub body: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResponseDraft {
    pub status: u16,
    pub headers: Headers,
    pub body: Vec<u8>,
}

impl RequestDraft {
    pub fn capture(request: &Request) -> Self {
        let (headers, body) = editable(&request.body, &request.headers);

        Self {
            method: request.method.clone(),
            target: request.target.clone(),
            headers,
            body,
        }
    }
}

impl ResponseDraft {
    pub fn capture(response: &Response) -> Self {
        let (headers, body) = editable(&response.body, &response.headers);

        Self {
            status: response.status,
            headers,
            body,
        }
    }
}

fn editable(body: &Body, headers: &Headers) -> (Headers, Vec<u8>) {
    let mut headers = headers.clone();

    let body = match body.content(&headers) {
        Ok(content) => {
            headers.remove("content-encoding");
            content.into_owned()
        }
        Err(_) => body.bytes().map(Cow::into_owned).unwrap_or_default(),
    };

    for name in FRAMING {
        headers.remove(name);
    }

    (headers, body)
}
