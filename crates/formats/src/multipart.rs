use memchr::{memchr, memmem};

#[derive(Clone, Debug, Default)]
pub struct Part {
    pub headers: Vec<(String, String)>,
    pub name: Option<String>,
    pub file_name: Option<String>,
    pub content_type: Option<String>,
    pub body: Vec<u8>,
}

pub fn boundary(content_type: &str) -> Option<String> {
    content_type
        .split(';')
        .skip(1)
        .find_map(|parameter| {
            let (key, value) = parameter.split_once('=')?;
            key.trim()
                .eq_ignore_ascii_case("boundary")
                .then(|| value.trim().trim_matches('"').to_owned())
        })
        .filter(|boundary| !boundary.is_empty())
}

pub fn multipart(bytes: &[u8], boundary: &str) -> Vec<Part> {
    let delimiter = format!("--{boundary}");
    let finder = memmem::Finder::new(delimiter.as_bytes());
    let mut parts = Vec::new();

    let Some(first) = finder.find(bytes) else {
        return parts;
    };

    let mut position = first + delimiter.len();

    loop {
        let rest = &bytes[position..];

        if rest.starts_with(b"--") {
            break;
        }

        let Some(line) = memchr(b'\n', rest) else {
            break;
        };

        let start = position + line + 1;
        let (end, next) = match finder.find(&bytes[start..]) {
            Some(offset) => (start + offset, start + offset + delimiter.len()),
            None => (bytes.len(), bytes.len()),
        };

        let content = &bytes[start..end];
        let content = content
            .strip_suffix(b"\r\n")
            .or_else(|| content.strip_suffix(b"\n"))
            .unwrap_or(content);

        parts.push(part(content));

        if next >= bytes.len() {
            break;
        }

        position = next;
    }

    parts
}

fn part(content: &[u8]) -> Part {
    let (head, body) = match memmem::find(content, b"\r\n\r\n") {
        Some(end) => (&content[..end], &content[end + 4..]),
        None => match memmem::find(content, b"\n\n") {
            Some(end) => (&content[..end], &content[end + 2..]),
            None if content.starts_with(b"\r\n") => (&content[..0], &content[2..]),
            None => (content, &content[content.len()..]),
        },
    };

    let headers: Vec<(String, String)> = String::from_utf8_lossy(head)
        .lines()
        .filter_map(|line| {
            let (name, value) = line.split_once(':')?;
            Some((name.trim().to_owned(), value.trim().to_owned()))
        })
        .collect();

    let header = |wanted: &str| {
        headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(wanted))
            .map(|(_, value)| value.as_str())
    };

    let disposition = header("content-disposition").unwrap_or_default();

    Part {
        name: parameter(disposition, "name"),
        file_name: parameter(disposition, "filename*")
            .map(|value| extended(&value))
            .or_else(|| parameter(disposition, "filename")),
        content_type: header("content-type").map(str::to_owned),
        body: body.to_vec(),
        headers,
    }
}

fn parameter(value: &str, wanted: &str) -> Option<String> {
    value.split(';').skip(1).find_map(|parameter| {
        let (key, value) = parameter.split_once('=')?;
        key.trim()
            .eq_ignore_ascii_case(wanted)
            .then(|| value.trim().trim_matches('"').to_owned())
    })
}

fn extended(value: &str) -> String {
    let encoded = value.splitn(3, '\'').nth(2).unwrap_or(value);
    url::form_urlencoded::parse(format!("x={}", encoded.replace('+', "%2B")).as_bytes())
        .next()
        .map(|(_, decoded)| decoded.into_owned())
        .unwrap_or_else(|| encoded.to_owned())
}
