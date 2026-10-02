use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::Path;
use std::sync::Arc;

use session::{Body, Entry, Headers, Opcode, Record};

use crate::ui::format;

const KEEP: usize = 64 << 20;
const SNIFF: usize = 4 << 20;
const ROWS: usize = 50_000;
const PREVIEW: usize = 200;

pub const TEXT_LIMIT: usize = 2 << 20;

const PROTOBUF: [&str; 5] = [
    "application/x-protobuf",
    "application/protobuf",
    "application/vnd.google.protobuf",
    "application/x-google-protobuf",
    "application/octet-stream+protobuf",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Request,
    Response,
    Sent,
    Received,
    Frame(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Signature {
    pub length: u64,
    pub complete: bool,
}

pub struct Content {
    pub bytes: Vec<u8>,
    pub total: u64,
    pub wire: u64,
    pub kind: String,
    pub encoding: Option<String>,
    pub textual: bool,
    pub problem: Option<String>,
    pub language: Option<&'static str>,
    pub pretty: Option<String>,
    pub structure: Option<Structure>,
}

pub struct Structure {
    pub label: &'static str,
    pub preferred: bool,
    pub view: View,
}

pub enum View {
    Table(Table),
    Tree(String),
}

pub struct Table {
    pub columns: Vec<&'static str>,
    pub rows: Vec<Row>,
}

pub struct Row {
    pub cells: Vec<String>,
    pub detail: Vec<u8>,
    pub kind: String,
}

impl Part {
    pub fn signature(self, record: &Record) -> Option<Signature> {
        if let Self::Frame(index) = self {
            return record.frames.get(index).map(|frame| Signature {
                length: frame.payload.len() as u64,
                complete: true,
            });
        }

        let (body, _) = self.body(record)?;

        Some(Signature {
            length: body.len(),
            complete: body.is_complete(),
        })
    }

    fn body(self, record: &Record) -> Option<(&Body, Option<&Headers>)> {
        match self {
            Self::Request => record
                .request
                .as_ref()
                .map(|request| (&request.body, Some(&request.headers))),
            Self::Response => record
                .response
                .as_ref()
                .map(|response| (&response.body, Some(&response.headers))),
            Self::Sent => record.tunnel.as_ref().map(|tunnel| (&tunnel.sent, None)),
            Self::Received => record
                .tunnel
                .as_ref()
                .map(|tunnel| (&tunnel.received, None)),
            Self::Frame(_) => None,
        }
    }
}

pub fn load(entry: &Arc<Entry>, part: Part) -> Option<Content> {
    let record = entry.read();

    if let Part::Frame(index) = part {
        let frame = record.frames.get(index)?;
        let (payload, compressed, text) = (
            frame.payload.clone(),
            frame.compressed,
            frame.opcode == Opcode::Text,
        );
        drop(record);

        let wire = payload.len() as u64;
        let mut content = Content::new(
            payload,
            wire,
            String::new(),
            compressed.then(|| String::from("permessage-deflate")),
            None,
            None,
        );

        content.textual = text;
        return Some(content);
    }

    let (body, headers) = part.body(&record)?;
    let kind = headers
        .and_then(|headers| headers.text("content-type"))
        .unwrap_or_default()
        .to_owned();
    let encoding = headers
        .and_then(|headers| headers.text("content-encoding"))
        .filter(|_| !body.is_decoded())
        .map(str::to_owned);

    let (bytes, problem) = match headers.map(|headers| body.content(headers)) {
        Some(Ok(content)) => (content.into_owned(), None),
        Some(Err(error)) => (
            body.bytes().map(|raw| raw.into_owned()).unwrap_or_default(),
            Some(error.to_string()),
        ),
        None => (
            body.bytes().map(|raw| raw.into_owned()).unwrap_or_default(),
            None,
        ),
    };

    let compression = headers
        .and_then(|headers| headers.text("grpc-encoding"))
        .map(str::to_owned);
    let wire = body.len();
    drop(record);

    Some(Content::new(
        bytes,
        wire,
        kind,
        encoding,
        problem,
        compression.as_deref(),
    ))
}

impl Content {
    fn new(
        mut bytes: Vec<u8>,
        wire: u64,
        kind: String,
        encoding: Option<String>,
        problem: Option<String>,
        compression: Option<&str>,
    ) -> Self {
        let total = bytes.len() as u64;
        bytes.truncate(KEEP);

        let shown = &bytes[..bytes.len().min(TEXT_LIMIT)];
        let textual = textual(&kind, &bytes);
        let language = language(&kind, shown);
        let pretty = (shown.len() as u64 == total)
            .then(|| pretty(language, shown))
            .flatten();
        let structure = (bytes.len() as u64 == total)
            .then(|| structure(&kind, &bytes, compression, textual))
            .flatten();

        Self {
            bytes,
            total,
            wire,
            kind,
            encoding,
            textual,
            problem,
            language,
            pretty,
            structure,
        }
    }
}

pub fn export(entry: &Arc<Entry>, part: Part, path: &Path) -> io::Result<()> {
    let record = entry.read();
    let mut file = BufWriter::new(File::create(path)?);

    if let Part::Frame(index) = part {
        file.write_all(
            record
                .frames
                .get(index)
                .map(|frame| frame.payload.as_slice())
                .unwrap_or_default(),
        )?;
        return file.flush();
    }

    match part.body(&record) {
        Some((body, Some(headers))) => {
            file.write_all(&body.content(headers).or_else(|_| body.bytes())?)?
        }
        Some((body, None)) => body.write_to(&mut file)?,
        None => {}
    }

    file.flush()
}

pub fn file_name(entry: &Arc<Entry>, part: Part) -> String {
    let record = entry.read();

    let named = match part {
        Part::Request | Part::Response => record.request.as_ref().and_then(|request| {
            let path = request.target.split(['?', '#']).next()?;
            let name = path.rsplit('/').next()?;
            (!name.is_empty()).then(|| name.to_owned())
        }),
        Part::Sent => Some(String::from("sent.bin")),
        Part::Received => Some(String::from("received.bin")),
        Part::Frame(index) => Some(format!("message-{}.bin", index + 1)),
    };

    named.unwrap_or_else(|| String::from("body"))
}

pub fn language(kind: &str, bytes: &[u8]) -> Option<&'static str> {
    let kind = kind.to_ascii_lowercase();

    let language = match () {
        _ if kind.contains("json") => "json",
        _ if kind.contains("javascript") || kind.contains("ecmascript") => "js",
        _ if kind.contains("html") => "html",
        _ if kind.contains("xml") || kind.contains("svg") => "xml",
        _ if kind.contains("css") => "css",
        _ if looks_like_json(bytes) => "json",
        _ => return None,
    };

    Some(language)
}

pub fn pretty(language: Option<&str>, bytes: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(bytes);

    Some(match language? {
        "json" => formats::json(&text),
        "xml" => formats::xml(&text),
        "html" => formats::html(&text),
        "js" => formats::javascript(&text),
        "css" => formats::css(&text),
        _ => return None,
    })
}

fn structure(
    kind: &str,
    bytes: &[u8],
    compression: Option<&str>,
    textual: bool,
) -> Option<Structure> {
    let lower = kind.to_ascii_lowercase();
    let essence = lower.split(';').next().unwrap_or_default().trim();

    let tree = |label, preferred, text: String| Structure {
        label,
        preferred,
        view: View::Tree(text),
    };

    match essence {
        _ if essence.starts_with("application/grpc") && !essence.ends_with("-text") => {
            formats::grpc(bytes, compression).map(|text| tree("gRPC", true, text))
        }
        _ if PROTOBUF.contains(&essence) => {
            formats::protobuf(bytes).map(|text| tree("Protobuf", true, text))
        }
        "application/x-www-form-urlencoded" => Some(form(bytes)),
        "text/event-stream" => Some(events(bytes)),
        _ if essence.starts_with("multipart/") => {
            formats::boundary(kind).map(|boundary| parts(bytes, &boundary))
        }
        _ if !textual && !is_image(kind) && !bytes.is_empty() && bytes.len() <= SNIFF => {
            formats::protobuf(bytes).map(|text| tree("Protobuf", false, text))
        }
        _ => None,
    }
}

fn form(bytes: &[u8]) -> Structure {
    let rows = formats::form(bytes)
        .into_iter()
        .take(ROWS)
        .map(|(name, value)| Row {
            cells: vec![name, preview(&value)],
            detail: value.into_bytes(),
            kind: String::new(),
        })
        .collect();

    table("Form", vec!["Name", "Value"], rows)
}

fn events(bytes: &[u8]) -> Structure {
    let rows = formats::events(&String::from_utf8_lossy(bytes))
        .into_iter()
        .take(ROWS)
        .enumerate()
        .map(|(index, event)| {
            let kind = match () {
                _ if event.comment => String::from("(comment)"),
                _ if event.kind.is_empty() => String::from("message"),
                _ => event.kind,
            };

            Row {
                cells: vec![
                    (index + 1).to_string(),
                    kind,
                    event.id,
                    preview(&event.data),
                ],
                detail: event.data.into_bytes(),
                kind: String::new(),
            }
        })
        .collect();

    table("Events", vec!["#", "Event", "ID", "Data"], rows)
}

fn parts(bytes: &[u8], boundary: &str) -> Structure {
    let rows = formats::multipart(bytes, boundary)
        .into_iter()
        .take(ROWS)
        .map(|part| {
            let kind = part.content_type.unwrap_or_default();
            let value = if textual(&kind, &part.body) {
                preview(&String::from_utf8_lossy(&part.body))
            } else {
                String::new()
            };

            Row {
                cells: vec![
                    part.name.unwrap_or_default(),
                    part.file_name.unwrap_or_default(),
                    kind.clone(),
                    format::size(part.body.len() as u64),
                    value,
                ],
                detail: part.body,
                kind,
            }
        })
        .collect();

    table("Parts", vec!["Name", "File", "Type", "Size", "Value"], rows)
}

fn table(label: &'static str, columns: Vec<&'static str>, rows: Vec<Row>) -> Structure {
    Structure {
        label,
        preferred: true,
        view: View::Table(Table { columns, rows }),
    }
}

fn preview(text: &str) -> String {
    let line: String = text
        .chars()
        .take(PREVIEW)
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect();
    format::printable(&line).into_owned()
}

pub fn is_image(kind: &str) -> bool {
    kind.to_ascii_lowercase().starts_with("image/")
}

pub fn textual(kind: &str, bytes: &[u8]) -> bool {
    let kind = kind.to_ascii_lowercase();
    let sample = &bytes[..bytes.len().min(4096)];

    kind.starts_with("text/")
        || [
            "json",
            "javascript",
            "xml",
            "html",
            "css",
            "x-www-form-urlencoded",
            "graphql",
        ]
        .iter()
        .any(|marker| kind.contains(marker))
        || (!sample.contains(&0) && utf8(sample))
}

fn looks_like_json(bytes: &[u8]) -> bool {
    let start = bytes.iter().find(|byte| !byte.is_ascii_whitespace());

    matches!(start, Some(b'{' | b'['))
        && serde_json::from_slice::<serde::de::IgnoredAny>(bytes).is_ok()
}

fn utf8(sample: &[u8]) -> bool {
    match std::str::from_utf8(sample) {
        Ok(_) => true,
        Err(error) => error.error_len().is_none(),
    }
}
