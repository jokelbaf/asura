use std::fmt::Write;

use session::{Request, Version};

const IMPLIED: [&str; 4] = ["content-length", "transfer-encoding", "connection", "host"];

pub fn command(request: &Request) -> String {
    let mut arguments = vec![String::from("curl"), quote(&request.url())];

    if request.method != "GET" || !request.body.is_empty() {
        arguments.push(format!("-X {}", quote(&request.method)));
    }

    match request.version {
        Version::Http10 => arguments.push(String::from("--http1.0")),
        Version::Http11 => arguments.push(String::from("--http1.1")),
        Version::Http2 | Version::Http3 => {}
    }

    for (name, value) in request.headers.iter() {
        if IMPLIED
            .iter()
            .any(|implied| name.eq_ignore_ascii_case(implied))
        {
            continue;
        }

        arguments.push(format!(
            "-H {}",
            quote(&format!("{name}: {}", String::from_utf8_lossy(value)))
        ));
    }

    if request.headers.get("accept-encoding").is_some() {
        arguments.push(String::from("--compressed"));
    }

    if let Some(data) = data(request) {
        arguments.push(data);
    }

    arguments.join(" \\\n  ")
}

fn data(request: &Request) -> Option<String> {
    let body = request.body.bytes().ok()?;

    if body.is_empty() {
        return None;
    }

    Some(match std::str::from_utf8(&body) {
        Ok(text) => format!("--data-raw {}", quote(text)),
        Err(_) => format!("--data-binary {}", escaped(&body)),
    })
}

fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

fn escaped(bytes: &[u8]) -> String {
    let mut output = String::from("$'");

    for byte in bytes {
        let _ = write!(output, "\\x{byte:02x}");
    }

    output.push('\'');
    output
}
