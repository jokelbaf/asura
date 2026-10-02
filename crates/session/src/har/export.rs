use std::sync::Arc;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use serde_json::{Value, json};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use url::Url;

use crate::{Body, Client, Direction, Entry, Headers, Opcode, Record, Request, Response};

pub fn export(entries: &[Arc<Entry>]) -> Value {
    let entries: Vec<Value> = entries.iter().filter_map(|entry| exchange(entry)).collect();

    json!({
        "log": {
            "version": "1.2",
            "creator": { "name": "Asura", "version": env!("CARGO_PKG_VERSION") },
            "pages": [],
            "entries": entries,
        }
    })
}

fn exchange(entry: &Entry) -> Option<Value> {
    let record = entry.read();
    let request = record.request.as_ref()?;
    let connection = entry.connection();

    let mut value = json!({
        "startedDateTime": timestamp(record.timing.started),
        "time": milliseconds(&record, record.timing.response_complete),
        "request": request_json(request),
        "response": record.response.as_ref().map_or_else(empty_response, response_json),
        "cache": {},
        "timings": timings(&record),
        "serverIPAddress": connection.server.ip().to_string(),
        "connection": connection.id.to_string(),
        "_process": process(&connection.client),
    });

    if !record.frames.is_empty() {
        value["_webSocketMessages"] = Value::Array(record.frames.iter().map(frame).collect());
    }

    Some(value)
}

fn request_json(request: &Request) -> Value {
    let url = request.url();

    let mut value = json!({
        "method": request.method,
        "url": url,
        "httpVersion": request.version.as_str(),
        "cookies": request_cookies(&request.headers),
        "headers": headers(&request.headers),
        "queryString": query(&url),
        "headersSize": -1,
        "bodySize": request.body.len(),
    });

    if !request.body.is_empty() {
        let mime = request.headers.text("content-type").unwrap_or_default();
        let mut post = json!({ "mimeType": mime });
        content(&mut post, &request.body, &request.headers);
        value["postData"] = post;
    }

    value
}

fn response_json(response: &Response) -> Value {
    let mut body = json!({
        "mimeType": response.headers.text("content-type").unwrap_or_default(),
    });
    let decoded_size = content(&mut body, &response.body, &response.headers);
    body["size"] = json!(decoded_size);
    body["compression"] = json!(decoded_size as i64 - response.body.len() as i64);

    json!({
        "status": response.status,
        "statusText": response.reason,
        "httpVersion": response.version.as_str(),
        "cookies": response_cookies(&response.headers),
        "headers": headers(&response.headers),
        "content": body,
        "redirectURL": response.headers.text("location").unwrap_or_default(),
        "headersSize": -1,
        "bodySize": response.body.len(),
    })
}

fn empty_response() -> Value {
    json!({
        "status": 0,
        "statusText": "",
        "httpVersion": "",
        "cookies": [],
        "headers": [],
        "content": { "size": 0, "mimeType": "" },
        "redirectURL": "",
        "headersSize": -1,
        "bodySize": -1,
        "_error": "no response",
    })
}

fn process(client: &Client) -> Value {
    json!({
        "pid": client.process,
        "name": client.name,
        "commandLine": client.command_line,
    })
}

fn content(target: &mut Value, body: &Body, headers: &Headers) -> usize {
    let Ok(data) = body.content(headers).or_else(|_| body.bytes()) else {
        return 0;
    };

    match String::from_utf8(data.into_owned()) {
        Ok(text) => {
            target["text"] = json!(text);
            text.len()
        }
        Err(error) => {
            let bytes = error.into_bytes();
            target["text"] = json!(STANDARD.encode(&bytes));
            target["encoding"] = json!("base64");
            bytes.len()
        }
    }
}

fn headers(headers: &Headers) -> Vec<Value> {
    headers
        .iter()
        .map(|(name, value)| json!({ "name": name, "value": String::from_utf8_lossy(value) }))
        .collect()
}

fn request_cookies(headers: &Headers) -> Vec<Value> {
    headers
        .all("cookie")
        .flat_map(|value| {
            String::from_utf8_lossy(value)
                .split(';')
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .filter_map(|pair| {
            let (name, value) = pair.split_once('=')?;
            Some(json!({ "name": name.trim(), "value": value.trim() }))
        })
        .collect()
}

fn response_cookies(headers: &Headers) -> Vec<Value> {
    headers
        .all("set-cookie")
        .filter_map(|value| {
            let value = String::from_utf8_lossy(value);
            let (name, rest) = value.split(';').next()?.split_once('=')?;
            Some(json!({ "name": name.trim(), "value": rest.trim() }))
        })
        .collect()
}

fn query(url: &str) -> Vec<Value> {
    Url::parse(url)
        .map(|url| {
            url.query_pairs()
                .map(|(name, value)| json!({ "name": name, "value": value }))
                .collect()
        })
        .unwrap_or_default()
}

fn timings(record: &Record) -> Value {
    let started = record.timing.started;
    let span = |from: Option<OffsetDateTime>, to: Option<OffsetDateTime>| match (from, to) {
        (Some(from), Some(to)) => ((to - from).as_seconds_f64() * 1000.0).max(0.0),
        _ => 0.0,
    };

    json!({
        "blocked": -1,
        "dns": -1,
        "connect": -1,
        "ssl": -1,
        "send": span(Some(started), record.timing.request_complete),
        "wait": span(record.timing.request_complete, record.timing.response_started),
        "receive": span(record.timing.response_started, record.timing.response_complete),
    })
}

fn frame(frame: &crate::Frame) -> Value {
    let (opcode, data) = match frame.opcode {
        Opcode::Text => (1, String::from_utf8_lossy(&frame.payload).into_owned()),
        Opcode::Binary => (2, STANDARD.encode(&frame.payload)),
        Opcode::Close => (8, STANDARD.encode(&frame.payload)),
        Opcode::Ping => (9, STANDARD.encode(&frame.payload)),
        Opcode::Pong => (10, STANDARD.encode(&frame.payload)),
        Opcode::Continuation => (0, STANDARD.encode(&frame.payload)),
        Opcode::Reserved(code) => (i32::from(code), STANDARD.encode(&frame.payload)),
    };

    json!({
        "type": match frame.direction {
            Direction::Outbound => "send",
            Direction::Inbound => "receive",
        },
        "time": frame.time.unix_timestamp_nanos() as f64 / 1e9,
        "opcode": opcode,
        "data": data,
    })
}

fn milliseconds(record: &Record, end: Option<OffsetDateTime>) -> f64 {
    end.map_or(0.0, |end| {
        ((end - record.timing.started).as_seconds_f64() * 1000.0).max(0.0)
    })
}

fn timestamp(time: OffsetDateTime) -> String {
    time.format(&Rfc3339).unwrap_or_default()
}
