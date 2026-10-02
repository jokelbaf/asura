use std::path::{Path, PathBuf};

use bytes::Bytes;
use http::StatusCode;
use httparse::{EMPTY_HEADER, Status};
use session::Headers;

use crate::tamper::Action;

const FIELDS: usize = 256;

const TYPES: [(&str, &str); 26] = [
    ("html", "text/html; charset=utf-8"),
    ("htm", "text/html; charset=utf-8"),
    ("css", "text/css; charset=utf-8"),
    ("js", "text/javascript; charset=utf-8"),
    ("mjs", "text/javascript; charset=utf-8"),
    ("json", "application/json"),
    ("xml", "application/xml"),
    ("txt", "text/plain; charset=utf-8"),
    ("csv", "text/csv; charset=utf-8"),
    ("svg", "image/svg+xml"),
    ("png", "image/png"),
    ("jpg", "image/jpeg"),
    ("jpeg", "image/jpeg"),
    ("gif", "image/gif"),
    ("webp", "image/webp"),
    ("avif", "image/avif"),
    ("ico", "image/x-icon"),
    ("wasm", "application/wasm"),
    ("pdf", "application/pdf"),
    ("zip", "application/zip"),
    ("mp3", "audio/mpeg"),
    ("mp4", "video/mp4"),
    ("webm", "video/webm"),
    ("woff", "font/woff"),
    ("woff2", "font/woff2"),
    ("pb", "application/x-protobuf"),
];

pub struct Mock {
    pub status: StatusCode,
    pub reason: Option<String>,
    pub headers: Headers,
    pub body: Bytes,
}

pub async fn mock(action: &Action) -> Mock {
    match action {
        Action::Mock {
            status,
            content_type,
            body,
        } => {
            let mut headers = Headers::default();

            if !content_type.trim().is_empty() {
                headers.push("content-type", content_type.trim().as_bytes());
            }

            Mock {
                status: StatusCode::from_u16(*status).unwrap_or(StatusCode::OK),
                reason: None,
                headers,
                body: Bytes::from(body.clone()),
            }
        }
        Action::File(path) => {
            let path = path.clone();
            let read = tokio::task::spawn_blocking(move || file(&path)).await;

            read.unwrap_or_else(|error| failure(&PathBuf::new(), &error.to_string()))
        }
    }
}

fn file(path: &Path) -> Mock {
    let contents = match std::fs::read(path) {
        Ok(contents) => contents,
        Err(error) => return failure(path, &error.to_string()),
    };

    if contents.starts_with(b"HTTP/")
        && let Some(mock) = raw(&contents)
    {
        return mock;
    }

    let mut headers = Headers::default();
    headers.push("content-type", guess(path).as_bytes());

    Mock {
        status: StatusCode::OK,
        reason: None,
        headers,
        body: Bytes::from(contents),
    }
}

fn raw(contents: &[u8]) -> Option<Mock> {
    let mut fields = [EMPTY_HEADER; FIELDS];
    let mut response = httparse::Response::new(&mut fields);

    let Ok(Status::Complete(length)) = response.parse(contents) else {
        return None;
    };

    let mut headers = Headers::default();

    for field in response.headers.iter() {
        headers.push(field.name, field.value);
    }

    headers.remove("content-length");
    headers.remove("transfer-encoding");

    Some(Mock {
        status: StatusCode::from_u16(response.code?).ok()?,
        reason: response.reason.map(str::to_owned),
        headers,
        body: Bytes::copy_from_slice(&contents[length..]),
    })
}

fn failure(path: &Path, reason: &str) -> Mock {
    let mut headers = Headers::default();
    headers.push("content-type", "text/plain; charset=utf-8");

    Mock {
        status: StatusCode::BAD_GATEWAY,
        reason: None,
        headers,
        body: Bytes::from(format!(
            "Asura could not read the auto-responder file {}: {reason}\n",
            path.display()
        )),
    }
}

fn guess(path: &Path) -> &'static str {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();

    TYPES
        .iter()
        .find(|(known, _)| *known == extension)
        .map_or("application/octet-stream", |(_, kind)| kind)
}
