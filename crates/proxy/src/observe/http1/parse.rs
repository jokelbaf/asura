use httparse::{EMPTY_HEADER, Error, ParserConfig, Status};
use session::{Headers, Version};

const SLOTS: [usize; 3] = [64, 512, 4096];

pub struct RequestHead {
    pub length: usize,
    pub method: String,
    pub target: String,
    pub version: Version,
    pub headers: Headers,
}

pub struct ResponseHead {
    pub length: usize,
    pub status: u16,
    pub reason: String,
    pub version: Version,
    pub headers: Headers,
}

pub enum Parsed<T> {
    Complete(T),
    Partial,
    Invalid,
}

pub fn request(buffer: &[u8]) -> Parsed<RequestHead> {
    for slots in SLOTS {
        let mut fields = vec![EMPTY_HEADER; slots];
        let mut request = httparse::Request::new(&mut fields);

        match ParserConfig::default().parse_request(&mut request, buffer) {
            Ok(Status::Complete(length)) => {
                return Parsed::Complete(RequestHead {
                    length,
                    method: request.method.unwrap_or_default().to_owned(),
                    target: request.path.unwrap_or_default().to_owned(),
                    version: version(request.version),
                    headers: collect(request.headers),
                });
            }
            Ok(Status::Partial) => return Parsed::Partial,
            Err(Error::TooManyHeaders) => continue,
            Err(_) => return Parsed::Invalid,
        }
    }

    Parsed::Invalid
}

pub fn response(buffer: &[u8]) -> Parsed<ResponseHead> {
    let mut config = ParserConfig::default();
    config
        .allow_spaces_after_header_name_in_responses(true)
        .allow_obsolete_multiline_headers_in_responses(true)
        .allow_multiple_spaces_in_response_status_delimiters(true);

    for slots in SLOTS {
        let mut fields = vec![EMPTY_HEADER; slots];
        let mut response = httparse::Response::new(&mut fields);

        match config.parse_response(&mut response, buffer) {
            Ok(Status::Complete(length)) => {
                return Parsed::Complete(ResponseHead {
                    length,
                    status: response.code.unwrap_or_default(),
                    reason: response.reason.unwrap_or_default().to_owned(),
                    version: version(response.version),
                    headers: collect(response.headers),
                });
            }
            Ok(Status::Partial) => return Parsed::Partial,
            Err(Error::TooManyHeaders) => continue,
            Err(_) => return Parsed::Invalid,
        }
    }

    Parsed::Invalid
}

pub fn trailer(line: &[u8]) -> Option<(String, Vec<u8>)> {
    let separator = line.iter().position(|&byte| byte == b':')?;
    let name = std::str::from_utf8(&line[..separator])
        .ok()?
        .trim()
        .to_owned();
    let value = line[separator + 1..].trim_ascii().to_vec();
    Some((name, value))
}

fn version(minor: Option<u8>) -> Version {
    match minor {
        Some(0) => Version::Http10,
        _ => Version::Http11,
    }
}

fn collect(fields: &[httparse::Header<'_>]) -> Headers {
    let mut headers = Headers::default();
    for field in fields.iter().take_while(|field| !field.name.is_empty()) {
        headers.push(field.name, field.value);
    }
    headers
}
