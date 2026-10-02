use std::collections::HashMap;
use std::rc::Rc;

use session::{Store, Summary};

use super::completion::Suggestion;

const LIMIT: usize = 60;
const RECENT: usize = 5000;
const FIELDS: usize = 6;

const METHODS: [&str; 7] = ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];

const KEYS: [(&str, &str); 9] = [
    ("host:", "Host contains the text"),
    ("status:", "Response status, e.g. 404, 4xx, >399"),
    ("method:", "Request method"),
    ("process:", "Process name or ID"),
    ("type:", "Response content type"),
    ("proto:", "Protocol"),
    ("is:", "Request state"),
    ("size>", "Response larger than, e.g. 1m"),
    ("size<", "Response smaller than, e.g. 10k"),
];

const FLAGS: [(&str, &str); 9] = [
    ("error", "Failed"),
    ("pending", "Still in progress"),
    ("ws", "WebSocket"),
    ("tunnel", "Raw TLS or TCP stream"),
    ("encrypted", "Passed through without decryption"),
    ("paused", "Paused at a breakpoint"),
    ("mocked", "Answered by the auto responder"),
    ("edited", "Edited at a breakpoint"),
    ("composed", "Sent from Asura"),
];

const CLASSES: [(&str, &str); 5] = [
    ("2xx", "Success"),
    ("3xx", "Redirection"),
    ("4xx", "Client error"),
    ("5xx", "Server error"),
    (">399", "Any error status"),
];

const SIZES: [(&str, &str); 4] = [
    ("10k", "10 KB"),
    ("100k", "100 KB"),
    ("1m", "1 MB"),
    ("10m", "10 MB"),
];

const PATTERNS: [(&str, &str); 2] = [
    ("exact:", "The URL equals the text"),
    ("regex:", "Regular expression"),
];

const STATUSES: [(&str, &str); 16] = [
    ("200", "OK"),
    ("201", "Created"),
    ("204", "No Content"),
    ("301", "Moved Permanently"),
    ("302", "Found"),
    ("304", "Not Modified"),
    ("400", "Bad Request"),
    ("401", "Unauthorized"),
    ("403", "Forbidden"),
    ("404", "Not Found"),
    ("409", "Conflict"),
    ("429", "Too Many Requests"),
    ("500", "Internal Server Error"),
    ("502", "Bad Gateway"),
    ("503", "Service Unavailable"),
    ("504", "Gateway Timeout"),
];

#[derive(Clone, Copy)]
enum Field {
    Host,
    Method,
    Process,
    Type,
    Protocol,
    Status,
}

#[derive(Default)]
pub struct Catalog {
    urls: Vec<String>,
    fields: [Vec<(String, usize)>; FIELDS],
}

impl Catalog {
    fn values(&self, field: Field) -> impl Iterator<Item = (&str, usize)> {
        self.fields[field as usize]
            .iter()
            .map(|(value, count)| (value.as_str(), *count))
    }

    fn hosts(&self) -> Vec<String> {
        let mut hosts: Vec<String> = Vec::new();

        for (host, _) in self.values(Field::Host) {
            let name = bare(host);

            for candidate in std::iter::once(name.to_owned()).chain(parent(name)) {
                if !hosts.contains(&candidate) {
                    hosts.push(candidate);
                }
            }
        }

        hosts
    }
}

#[derive(Default)]
pub struct Builder {
    urls: HashMap<String, u64>,
    fields: [HashMap<String, usize>; FIELDS],
}

impl Builder {
    pub fn add(&mut self, summary: &Summary) {
        let essence = summary
            .content_type
            .split(';')
            .next()
            .unwrap_or_default()
            .trim()
            .to_lowercase();

        let values = [
            (Field::Host, summary.host.clone()),
            (Field::Method, summary.method.to_ascii_uppercase()),
            (Field::Process, summary.process.clone()),
            (Field::Type, essence),
            (Field::Protocol, summary.protocol.to_lowercase()),
            (
                Field::Status,
                summary
                    .status
                    .map(|status| status.to_string())
                    .unwrap_or_default(),
            ),
        ];

        for (field, value) in values.into_iter().filter(|(_, value)| !value.is_empty()) {
            *self.fields[field as usize].entry(value).or_default() += 1;
        }

        if summary.method.is_empty() {
            return;
        }

        let address = summary.url.split(['?', '#']).next().unwrap_or_default();
        let origin = address.split_once("://").map(|(scheme, rest)| {
            format!("{scheme}://{}/", rest.split('/').next().unwrap_or_default())
        });

        for url in [Some(address.to_owned()), origin].into_iter().flatten() {
            let newest = self.urls.entry(url).or_default();
            *newest = (*newest).max(summary.id);
        }
    }

    pub fn build(self) -> Catalog {
        let mut urls: Vec<(String, u64)> = self.urls.into_iter().collect();
        urls.sort_by_key(|(_, newest)| std::cmp::Reverse(*newest));

        Catalog {
            urls: urls.into_iter().map(|(url, _)| url).collect(),
            fields: self.fields.map(|counts| {
                let mut values: Vec<(String, usize)> = counts.into_iter().collect();
                values
                    .sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
                values
            }),
        }
    }
}

pub fn snapshot(store: &Store) -> Rc<Catalog> {
    let mut builder = Builder::default();

    for entry in store.entries().iter().rev().take(RECENT) {
        builder.add(&entry.summary());
    }

    Rc::new(builder.build())
}

pub fn filter(
    source: impl Fn() -> Rc<Catalog> + 'static,
) -> impl Fn(&str, usize) -> Vec<Suggestion> {
    move |text, cursor| {
        let token = Token::at(text, cursor);
        let (sign, body) = match token.typed.strip_prefix('-') {
            Some(rest) => ("-", rest),
            None => ("", token.typed.as_str()),
        };

        let suggest = |label: &str, detail: String, insert: String, more: bool| Suggestion {
            label: label.to_owned(),
            detail,
            insert,
            start: token.start,
            end: token.end,
            more,
        };

        let space = if token.end == text.chars().count() {
            " "
        } else {
            ""
        };

        if let Some((key, value)) = body.split_once(':') {
            let value = value.trim_start_matches('"');

            return values(&source(), key, value)
                .into_iter()
                .map(|(candidate, detail)| {
                    suggest(
                        &candidate,
                        detail,
                        format!("{sign}{key}:{}{space}", quote(&candidate)),
                        false,
                    )
                })
                .collect();
        }

        if let Some(index) = body
            .find(['>', '<'])
            .filter(|_| body.to_ascii_lowercase().starts_with("size"))
        {
            let (key, value) = body.split_at(index + 1);

            return rank(SIZES.iter().map(|(size, _)| *size), value)
                .into_iter()
                .filter_map(|size| SIZES.iter().find(|(candidate, _)| *candidate == size))
                .map(|(size, detail)| {
                    suggest(
                        size,
                        (*detail).to_owned(),
                        format!("{sign}{key}{size}{space}"),
                        false,
                    )
                })
                .collect();
        }

        let keys = rank(KEYS.iter().map(|(key, _)| *key), body)
            .into_iter()
            .filter_map(|key| KEYS.iter().find(|(candidate, _)| *candidate == key))
            .map(|(key, detail)| {
                suggest(
                    &format!("{sign}{key}"),
                    (*detail).to_owned(),
                    format!("{sign}{key}"),
                    true,
                )
            });

        let catalog = source();
        let hosts = match body {
            "" => Vec::new(),
            body => rank(catalog.values(Field::Host).map(|(host, _)| host), body),
        };

        let hosts = hosts.into_iter().map(|host| {
            suggest(
                &format!("{sign}host:{host}"),
                String::from("Host"),
                format!("{sign}host:{host}{space}"),
                false,
            )
        });

        keys.chain(hosts).take(LIMIT).collect()
    }
}

pub fn pattern(catalog: Rc<Catalog>) -> impl Fn(&str, usize) -> Vec<Suggestion> {
    move |text, _| {
        let typed = text.trim();

        if prefixed(typed, "regex:").is_some() {
            return Vec::new();
        }

        let (prefix, rest) = match prefixed(typed, "exact:") {
            Some(rest) => ("exact:", rest),
            None => ("", typed),
        };

        let prefixes = rank(PATTERNS.iter().map(|(pattern, _)| *pattern), typed)
            .into_iter()
            .filter(|_| prefix.is_empty())
            .filter_map(|pattern| PATTERNS.iter().find(|(candidate, _)| *candidate == pattern))
            .map(|(pattern, detail)| whole(text, pattern, detail, pattern.to_string(), true));

        let urls = rank(catalog.urls.iter().map(String::as_str), rest)
            .into_iter()
            .map(|url| whole(text, url, "", format!("{prefix}{url}"), false));

        prefixes.chain(urls).take(LIMIT).collect()
    }
}

pub fn method(catalog: Rc<Catalog>) -> impl Fn(&str, usize) -> Vec<Suggestion> {
    move |text, _| {
        let mut methods: Vec<&str> = METHODS.to_vec();

        for (method, _) in catalog.values(Field::Method) {
            if !methods.contains(&method) {
                methods.push(method);
            }
        }

        rank(methods, text.trim())
            .into_iter()
            .map(|method| whole(text, method, "", method.to_owned(), false))
            .collect()
    }
}

pub fn status() -> impl Fn(&str, usize) -> Vec<Suggestion> {
    |text, _| {
        rank(STATUSES.iter().map(|(code, _)| *code), text.trim())
            .into_iter()
            .filter_map(|code| STATUSES.iter().find(|(candidate, _)| *candidate == code))
            .map(|(code, reason)| whole(text, code, reason, (*code).to_owned(), false))
            .collect()
    }
}

pub fn code(text: &str) -> Option<u16> {
    text.trim()
        .parse()
        .ok()
        .filter(|code| (100..=999).contains(code))
}

pub fn host(catalog: Rc<Catalog>) -> impl Fn(&str, usize) -> Vec<Suggestion> {
    move |text, _| {
        let hosts = catalog.hosts();

        rank(hosts.iter().map(String::as_str), text.trim())
            .into_iter()
            .map(|host| {
                let detail = if host.starts_with("*.") {
                    "Every subdomain"
                } else {
                    ""
                };
                whole(text, host, detail, host.to_owned(), false)
            })
            .collect()
    }
}

pub fn url(catalog: Rc<Catalog>) -> impl Fn(&str, usize) -> Vec<Suggestion> {
    move |text, _| {
        rank(catalog.urls.iter().map(String::as_str), text.trim())
            .into_iter()
            .map(|url| whole(text, url, "", url.to_owned(), false))
            .collect()
    }
}

struct Token {
    start: usize,
    end: usize,
    typed: String,
}

impl Token {
    fn at(text: &str, cursor: usize) -> Self {
        let characters: Vec<char> = text.chars().collect();
        let cursor = cursor.min(characters.len());

        let mut start = 0;
        let mut quoted = false;

        for (index, character) in characters.iter().enumerate().take(cursor) {
            match character {
                '"' => quoted = !quoted,
                character if character.is_whitespace() && !quoted => start = index + 1,
                _ => {}
            }
        }

        let mut end = cursor;

        while let Some(character) = characters.get(end) {
            match character {
                '"' => quoted = !quoted,
                character if character.is_whitespace() && !quoted => break,
                _ => {}
            }

            end += 1;
        }

        Self {
            start,
            end,
            typed: characters[start..cursor].iter().collect(),
        }
    }
}

fn values(catalog: &Catalog, key: &str, typed: &str) -> Vec<(String, String)> {
    let counted = |field: Field| -> Vec<(String, String)> {
        let counts: Vec<(&str, usize)> = catalog.values(field).collect();

        rank(counts.iter().map(|(value, _)| *value), typed)
            .into_iter()
            .map(|value| {
                let count = counts
                    .iter()
                    .find(|(candidate, _)| *candidate == value)
                    .map_or(0, |(_, count)| *count);
                (value.to_owned(), requests(count))
            })
            .collect()
    };

    let described = |entries: &[(&str, &str)]| -> Vec<(String, String)> {
        rank(entries.iter().map(|(value, _)| *value), typed)
            .into_iter()
            .filter_map(|value| entries.iter().find(|(candidate, _)| *candidate == value))
            .map(|(value, detail)| ((*value).to_owned(), (*detail).to_owned()))
            .collect()
    };

    match key.to_lowercase().as_str() {
        "host" | "domain" => counted(Field::Host),
        "process" | "app" => counted(Field::Process),
        "type" | "mime" => counted(Field::Type),
        "proto" | "protocol" => counted(Field::Protocol),
        "is" => described(&FLAGS),
        "status" => {
            let mut statuses = described(&CLASSES);
            statuses.extend(counted(Field::Status));
            statuses
        }
        "method" => {
            let mut methods = counted(Field::Method);

            for method in rank(METHODS, typed) {
                if !methods.iter().any(|(candidate, _)| candidate == method) {
                    methods.push((method.to_owned(), String::new()));
                }
            }

            methods
        }
        _ => Vec::new(),
    }
}

fn whole(text: &str, label: &str, detail: &str, insert: String, more: bool) -> Suggestion {
    Suggestion {
        label: label.to_owned(),
        detail: detail.to_owned(),
        insert,
        start: 0,
        end: text.chars().count(),
        more,
    }
}

fn rank<'a>(candidates: impl IntoIterator<Item = &'a str>, typed: &str) -> Vec<&'a str> {
    let needle = typed.to_lowercase();
    let mut leading = Vec::new();
    let mut inner = Vec::new();

    for candidate in candidates {
        let lower = candidate.to_lowercase();

        if lower == needle {
            continue;
        }

        if lower.starts_with(&needle) {
            leading.push(candidate);
        } else if lower.contains(&needle) {
            inner.push(candidate);
        }
    }

    leading.extend(inner);
    leading.truncate(LIMIT);
    leading
}

fn prefixed<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    text.get(..prefix.len())
        .filter(|head| head.eq_ignore_ascii_case(prefix))
        .map(|_| &text[prefix.len()..])
}

fn quote(value: &str) -> String {
    if value.contains(char::is_whitespace) {
        format!("\"{value}\"")
    } else {
        value.to_owned()
    }
}

fn requests(count: usize) -> String {
    match count {
        0 => String::new(),
        1 => String::from("1 request"),
        count => format!("{count} requests"),
    }
}

fn bare(host: &str) -> &str {
    match host.rsplit_once(':') {
        Some((name, port)) if !name.contains(':') && port.parse::<u16>().is_ok() => name,
        _ => host,
    }
}

fn parent(host: &str) -> Option<String> {
    if host.parse::<std::net::IpAddr>().is_ok() {
        return None;
    }

    let (_, domain) = host.split_once('.')?;
    domain.contains('.').then(|| format!("*.{domain}"))
}
