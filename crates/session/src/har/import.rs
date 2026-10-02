use std::borrow::Cow;
use std::collections::HashMap;
use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::Path;
use std::sync::Arc;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use serde_json::Value;
use time::format_description::well_known::Rfc3339;
use time::{Duration, OffsetDateTime};
use url::{Position, Url};

use crate::{
    Body, Client, Connection, Direction, Frame, Headers, Opcode, Record, Request, Response, Scheme,
    Store, Tls, Version,
};

pub fn import(store: &Store, document: &[u8]) -> io::Result<usize> {
    let document: Value = serde_json::from_slice(document)?;
    let entries = document["log"]["entries"]
        .as_array()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "not a HAR document"))?;

    let mut connections: HashMap<String, Arc<Connection>> = HashMap::new();
    let mut imported = 0;

    for value in entries {
        let Some(url) = value["request"]["url"]
            .as_str()
            .and_then(|url| Url::parse(url).ok())
        else {
            continue;
        };

        let record = record(value, &url, store.spool())?;
        let fresh = || {
            Arc::new(connection(
                value,
                &url,
                store.connection_id(),
                record.timing.started,
            ))
        };

        let connection = match value["connection"].as_str() {
            Some(key) => connections
                .entry(key.to_owned())
                .or_insert_with(fresh)
                .clone(),
            None => fresh(),
        };

        store.restore(connection, record);
        imported += 1;
    }

    Ok(imported)
}

fn connection(value: &Value, url: &Url, id: u64, opened: OffsetDateTime) -> Connection {
    let process = &value["_process"];
    let port = url.port_or_known_default().unwrap_or_default();

    let address = value["serverIPAddress"]
        .as_str()
        .and_then(|address| address.trim_matches(['[', ']']).parse().ok())
        .unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED));

    let tls = secure(url).then(|| Tls {
        server_name: url.host_str().map(str::to_owned),
        decrypted: true,
        ..Tls::default()
    });

    Connection {
        id,
        client: Client {
            process: process["pid"].as_u64().unwrap_or_default() as u32,
            name: text(&process["name"]),
            command_line: text(&process["commandLine"]),
            target: None,
        },
        server: SocketAddr::new(address, port),
        tls,
        opened,
    }
}

fn record(value: &Value, url: &Url, spool: &Path) -> io::Result<Record> {
    let started = value["startedDateTime"]
        .as_str()
        .and_then(|time| OffsetDateTime::parse(time, &Rfc3339).ok())
        .unwrap_or_else(OffsetDateTime::now_utc);

    let mut record = Record::new(started);
    record.request = Some(request(&value["request"], url, spool)?);
    record.response = response(&value["response"], spool)?;
    record.frames = frames(&value["_webSocketMessages"]);

    let timings = &value["timings"];
    let elapsed = |names: &[&str]| -> f64 {
        names
            .iter()
            .filter_map(|name| timings[name].as_f64())
            .filter(|time| *time > 0.0)
            .sum()
    };
    let at = |milliseconds: f64| Some(started + Duration::seconds_f64(milliseconds / 1000.0));

    let sent = elapsed(&["blocked", "dns", "connect", "send"]);
    let waited = sent + elapsed(&["wait"]);
    let received = waited + elapsed(&["receive"]);

    record.timing.request_complete = at(sent);

    if record.response.is_some() {
        record.timing.response_started = at(waited);
        record.timing.response_complete = at(received);
    }

    match value["response"]["_error"].as_str() {
        Some(reason) => record.fail(reason),
        None => record.complete(),
    }

    Ok(record)
}

fn request(value: &Value, url: &Url, spool: &Path) -> io::Result<Request> {
    let mut body = Body::default();
    fill(&mut body, &value["postData"], spool)?;

    Ok(Request {
        method: value["method"].as_str().unwrap_or("GET").to_owned(),
        scheme: if secure(url) {
            Scheme::Https
        } else {
            Scheme::Http
        },
        authority: url[Position::BeforeHost..Position::AfterPort].to_owned(),
        target: url[Position::BeforePath..Position::AfterQuery].to_owned(),
        version: Version::parse(&text(&value["httpVersion"])),
        headers: headers(&value["headers"]),
        trailers: Headers::default(),
        body,
    })
}

fn response(value: &Value, spool: &Path) -> io::Result<Option<Response>> {
    let Some(status) = value["status"].as_u64().filter(|status| *status > 0) else {
        return Ok(None);
    };

    let mut body = Body::default();
    fill(&mut body, &value["content"], spool)?;

    Ok(Some(Response {
        status: status as u16,
        reason: text(&value["statusText"]),
        version: Version::parse(&text(&value["httpVersion"])),
        headers: headers(&value["headers"]),
        trailers: Headers::default(),
        body,
    }))
}

fn fill(body: &mut Body, content: &Value, spool: &Path) -> io::Result<()> {
    if let Some(text) = content["text"].as_str() {
        let data = match content["encoding"].as_str() {
            Some("base64") => STANDARD
                .decode(text)
                .map_or(Cow::Borrowed(text.as_bytes()), Cow::Owned),
            _ => Cow::Borrowed(text.as_bytes()),
        };

        body.append(&data, spool)?;
    }

    body.mark_decoded();
    body.finish();
    Ok(())
}

fn headers(value: &Value) -> Headers {
    let mut headers = Headers::default();

    for field in value.as_array().into_iter().flatten() {
        let name = text(&field["name"]);

        if !name.is_empty() && !name.starts_with(':') {
            headers.push(name, text(&field["value"]));
        }
    }

    headers
}

fn frames(value: &Value) -> Vec<Frame> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|message| {
            let opcode = Opcode::from(message["opcode"].as_u64()? as u8);
            let data = message["data"].as_str().unwrap_or_default();

            let payload = match opcode {
                Opcode::Text => data.as_bytes().to_vec(),
                _ => STANDARD
                    .decode(data)
                    .unwrap_or_else(|_| data.as_bytes().to_vec()),
            };

            let direction = match message["type"].as_str() {
                Some("send") => Direction::Outbound,
                _ => Direction::Inbound,
            };

            let nanoseconds = (message["time"].as_f64()? * 1e9) as i128;

            Some(Frame {
                direction,
                opcode,
                payload,
                compressed: false,
                time: OffsetDateTime::from_unix_timestamp_nanos(nanoseconds).ok()?,
            })
        })
        .collect()
}

fn secure(url: &Url) -> bool {
    matches!(url.scheme(), "https" | "wss")
}

fn text(value: &Value) -> String {
    value.as_str().unwrap_or_default().to_owned()
}
