use bytes::{Buf, BytesMut};
use memchr::memmem;
use session::Headers;

pub enum Framing {
    Length(u64),
    Chunked(Chunk),
    Close,
}

pub enum Chunk {
    Size,
    Data(u64),
    Delimiter,
    Trailers,
}

pub fn request(headers: &Headers) -> Option<Framing> {
    if chunked(headers) {
        return Some(Framing::Chunked(Chunk::Size));
    }

    content_length(headers)
        .filter(|&length| length > 0)
        .map(Framing::Length)
}

pub fn response(status: u16, head_request: bool, headers: &Headers) -> Option<Framing> {
    if head_request || status / 100 == 1 || status == 204 || status == 304 {
        return None;
    }

    if chunked(headers) {
        return Some(Framing::Chunked(Chunk::Size));
    }

    match content_length(headers) {
        Some(0) => None,
        Some(length) => Some(Framing::Length(length)),
        None => Some(Framing::Close),
    }
}

pub fn take_line(buffer: &mut BytesMut) -> Option<BytesMut> {
    let end = memmem::find(buffer, b"\r\n")?;
    let line = buffer.split_to(end);
    buffer.advance(2);
    Some(line)
}

pub fn chunk_size(line: &[u8]) -> Option<u64> {
    let digits = line.split(|&byte| byte == b';').next()?;
    u64::from_str_radix(std::str::from_utf8(digits).ok()?.trim(), 16).ok()
}

fn chunked(headers: &Headers) -> bool {
    headers
        .all("transfer-encoding")
        .filter_map(|value| std::str::from_utf8(value).ok())
        .flat_map(|value| value.split(','))
        .last()
        .is_some_and(|coding| coding.trim().eq_ignore_ascii_case("chunked"))
}

fn content_length(headers: &Headers) -> Option<u64> {
    headers.text("content-length")?.trim().parse().ok()
}
