use crate::{Outcome, Summary};

const KEYS: [&str; 12] = [
    "host", "domain", "method", "process", "app", "type", "mime", "proto", "protocol", "status",
    "is", "size",
];

#[derive(Clone, Debug, Default)]
pub struct Filter {
    terms: Vec<Term>,
}

#[derive(Clone, Debug)]
struct Term {
    negated: bool,
    kind: Kind,
}

#[derive(Clone, Debug)]
enum Kind {
    Text(String),
    Host(String),
    Method(String),
    Process(String),
    Mime(String),
    Protocol(String),
    Status(Bound),
    StatusClass(u16),
    Size(Bound),
    Is(Flag),
}

#[derive(Clone, Copy, Debug)]
enum Bound {
    Equal(u64),
    Above(u64),
    Below(u64),
}

#[derive(Clone, Copy, Debug)]
enum Flag {
    Failed,
    Pending,
    WebSocket,
    Tunnel,
    Encrypted,
    Paused,
    Mocked,
    Edited,
    Composed,
}

impl Filter {
    pub fn parse(query: &str) -> Self {
        Self {
            terms: tokens(query)
                .iter()
                .filter(|token| !incomplete(token))
                .map(|token| Term::parse(token))
                .collect(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.terms.is_empty()
    }

    pub fn matches(&self, summary: &Summary) -> bool {
        self.terms
            .iter()
            .all(|term| term.kind.matches(summary) != term.negated)
    }
}

impl Term {
    fn parse(token: &str) -> Self {
        let (negated, token) = match token.strip_prefix('-') {
            Some(rest) if !rest.is_empty() => (true, rest),
            _ => (false, token),
        };

        Self {
            negated,
            kind: Kind::parse(token),
        }
    }
}

impl Kind {
    fn parse(token: &str) -> Self {
        if let Some(bound) = token.strip_prefix("size").and_then(Bound::parse) {
            return Self::Size(bound);
        }

        let Some((key, value)) = token.split_once(':') else {
            return Self::Text(token.to_lowercase());
        };

        let value = value.to_lowercase();

        match key.to_lowercase().as_str() {
            "host" | "domain" => Self::Host(value),
            "method" => Self::Method(value),
            "process" | "app" => Self::Process(value),
            "type" | "mime" => Self::Mime(value),
            "proto" | "protocol" => Self::Protocol(value),
            "status" => status(&value).unwrap_or(Self::Text(token.to_lowercase())),
            "is" => flag(&value).map_or(Self::Text(token.to_lowercase()), Self::Is),
            _ => Self::Text(token.to_lowercase()),
        }
    }

    fn matches(&self, summary: &Summary) -> bool {
        match self {
            Self::Text(text) => [
                &summary.url,
                &summary.process,
                &summary.method,
                &summary.content_type,
            ]
            .iter()
            .any(|field| contains(field, text)),
            Self::Host(host) => contains(&summary.host, host),
            Self::Method(method) => summary.method.eq_ignore_ascii_case(method),
            Self::Process(process) => {
                contains(&summary.process, process) || summary.pid.to_string() == *process
            }
            Self::Mime(mime) => contains(&summary.content_type, mime),
            Self::Protocol(protocol) => contains(&summary.protocol, protocol),
            Self::Status(bound) => summary
                .status
                .is_some_and(|status| bound.accepts(u64::from(status))),
            Self::StatusClass(class) => summary.status.is_some_and(|status| status / 100 == *class),
            Self::Size(bound) => bound.accepts(summary.size),
            Self::Is(flag) => flag.matches(summary),
        }
    }
}

impl Bound {
    fn parse(text: &str) -> Option<Self> {
        let (constructor, rest): (fn(u64) -> Self, &str) = match text.chars().next()? {
            '>' => (Self::Above, &text[1..]),
            '<' => (Self::Below, &text[1..]),
            '=' | ':' => (Self::Equal, &text[1..]),
            _ => return None,
        };

        Some(constructor(quantity(rest)?))
    }

    fn accepts(self, value: u64) -> bool {
        match self {
            Self::Equal(expected) => value == expected,
            Self::Above(limit) => value > limit,
            Self::Below(limit) => value < limit,
        }
    }
}

impl Flag {
    fn matches(self, summary: &Summary) -> bool {
        match self {
            Self::Failed => matches!(summary.outcome, Outcome::Failed(_)),
            Self::Pending => summary.outcome == Outcome::Pending,
            Self::WebSocket => summary.websocket,
            Self::Tunnel => summary.tunnel,
            Self::Encrypted => !summary.decrypted,
            Self::Paused => summary.paused,
            Self::Mocked => summary.mocked,
            Self::Edited => summary.edited,
            Self::Composed => summary.composed,
        }
    }
}

fn status(value: &str) -> Option<Kind> {
    if let Some(class) = value.strip_suffix("xx") {
        return class.parse().ok().map(Kind::StatusClass);
    }

    Bound::parse(value)
        .or_else(|| value.parse().ok().map(Bound::Equal))
        .map(Kind::Status)
}

fn flag(value: &str) -> Option<Flag> {
    Some(match value {
        "error" | "failed" => Flag::Failed,
        "pending" | "open" => Flag::Pending,
        "ws" | "websocket" => Flag::WebSocket,
        "tunnel" => Flag::Tunnel,
        "encrypted" | "passthrough" => Flag::Encrypted,
        "paused" | "breakpoint" => Flag::Paused,
        "mocked" | "responded" => Flag::Mocked,
        "edited" | "tampered" => Flag::Edited,
        "composed" | "replayed" => Flag::Composed,
        _ => return None,
    })
}

fn quantity(text: &str) -> Option<u64> {
    let text = text.trim().to_lowercase();
    let split = text
        .find(|character: char| !character.is_ascii_digit())
        .unwrap_or(text.len());
    let (digits, unit) = text.split_at(split);

    let multiplier = match unit.trim_end_matches('b') {
        "" => 1,
        "k" => 1 << 10,
        "m" => 1 << 20,
        "g" => 1 << 30,
        _ => return None,
    };

    digits.parse::<u64>().ok().map(|value| value * multiplier)
}

fn contains(haystack: &str, needle: &str) -> bool {
    haystack.to_lowercase().contains(needle)
}

fn tokens(query: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quoted = false;

    for character in query.chars() {
        match character {
            '"' => quoted = !quoted,
            character if character.is_whitespace() && !quoted => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            character => current.push(character),
        }
    }

    if !current.is_empty() {
        tokens.push(current);
    }

    tokens
}

fn incomplete(token: &str) -> bool {
    let token = token.strip_prefix('-').unwrap_or(token).to_lowercase();

    match token.split_once(':') {
        Some((key, "")) => KEYS.contains(&key),
        _ => matches!(token.as_str(), "size>" | "size<" | "size="),
    }
}
