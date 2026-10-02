use std::borrow::Cow;
use std::fmt::Write;
use std::io::Read;

use flate2::read::{GzDecoder, ZlibDecoder};

use crate::{hexdump, protobuf};

const HEADER: usize = 5;
const COMPRESSED: u8 = 0x01;
const TRAILERS: u8 = 0x80;
const PREVIEW: usize = 64 << 10;
const LIMIT: u64 = 64 << 20;

pub fn grpc(bytes: &[u8], encoding: Option<&str>) -> Option<String> {
    let mut output = String::new();
    let mut rest = bytes;
    let mut count = 0;

    while !rest.is_empty() {
        if rest.len() < HEADER {
            let _ = writeln!(
                output,
                "Incomplete frame header ({} of {HEADER} bytes)",
                rest.len()
            );
            break;
        }

        let flags = rest[0];

        if flags & !(COMPRESSED | TRAILERS) != 0 {
            if count == 0 {
                return None;
            }

            let _ = writeln!(output, "Unrecognised data ({} bytes)", rest.len());
            break;
        }

        let length = u32::from_be_bytes([rest[1], rest[2], rest[3], rest[4]]) as usize;
        let available = rest.len() - HEADER;

        if available < length {
            let _ = writeln!(
                output,
                "Incomplete message ({available} of {length} bytes received)"
            );
            break;
        }

        let payload = &rest[HEADER..HEADER + length];
        rest = &rest[HEADER + length..];
        count += 1;

        if count > 1 {
            output.push('\n');
        }

        if flags & TRAILERS != 0 {
            output.push_str("Trailers\n");
            indented(&mut output, &String::from_utf8_lossy(payload));
            continue;
        }

        let compressed = flags & COMPRESSED != 0;
        let _ = write!(output, "Message {count} · {length} bytes");

        if compressed {
            let _ = write!(output, " · compressed ({})", encoding.unwrap_or("unknown"));
        }

        output.push('\n');

        let data = if compressed {
            decompress(payload, encoding)
        } else {
            Ok(Cow::Borrowed(payload))
        };

        match data {
            Ok(data) if data.is_empty() => indented(&mut output, "(empty)"),
            Ok(data) => match protobuf(&data) {
                Some(tree) => indented(&mut output, &tree),
                None => indented(&mut output, &hexdump(&data[..data.len().min(PREVIEW)])),
            },
            Err(reason) => {
                indented(&mut output, &reason);
                indented(
                    &mut output,
                    &hexdump(&payload[..payload.len().min(PREVIEW)]),
                );
            }
        }
    }

    (count > 0).then_some(output)
}

fn decompress<'a>(payload: &'a [u8], encoding: Option<&str>) -> Result<Cow<'a, [u8]>, String> {
    let mut output = Vec::new();

    let result = match encoding.map(str::to_ascii_lowercase).as_deref() {
        Some("gzip") => GzDecoder::new(payload).take(LIMIT).read_to_end(&mut output),
        Some("deflate") => ZlibDecoder::new(payload)
            .take(LIMIT)
            .read_to_end(&mut output),
        Some("identity") => return Ok(Cow::Borrowed(payload)),
        other => {
            return Err(format!(
                "compressed with {}, which cannot be decoded",
                other.unwrap_or("an unknown encoding")
            ));
        }
    };

    result
        .map(|_| Cow::Owned(output))
        .map_err(|error| format!("decompression failed: {error}"))
}

fn indented(output: &mut String, text: &str) {
    for line in text.lines() {
        output.push_str("  ");
        output.push_str(line);
        output.push('\n');
    }
}
