use regex::{Regex, RegexBuilder};
use session::Scheme;

#[derive(Clone, Debug)]
pub struct Pattern {
    matcher: Matcher,
    scope: Scope,
}

#[derive(Clone, Debug)]
enum Matcher {
    Expression(Regex),
    Exact(String),
    Prefix(String),
    Contains(String),
}

#[derive(Clone, Debug, Default)]
struct Scope {
    scheme: Option<Scheme>,
    host: Option<Host>,
}

#[derive(Clone, Debug)]
enum Host {
    Exact(String),
    Prefix(String),
}

impl Pattern {
    pub fn parse(text: &str) -> Result<Self, String> {
        let text = text.trim();

        if text.is_empty() {
            return Err(String::from("the pattern is empty"));
        }

        if let Some(expression) = text.strip_prefix("regex:") {
            let regex = Regex::new(expression).map_err(|error| error.to_string())?;

            return Ok(Self {
                matcher: Matcher::Expression(regex),
                scope: Scope::default(),
            });
        }

        if let Some(url) = text.strip_prefix("exact:") {
            return Ok(Self {
                matcher: Matcher::Exact(url.trim().to_lowercase()),
                scope: Scope::of(url.trim()),
            });
        }

        let scope = Scope::of(text);

        let matcher = if text.contains('*') {
            Matcher::Expression(wildcard(text)?)
        } else if scope.scheme.is_some() {
            Matcher::Prefix(text.to_lowercase())
        } else {
            Matcher::Contains(text.to_lowercase())
        };

        Ok(Self { matcher, scope })
    }

    pub fn matches(&self, url: &str) -> bool {
        match &self.matcher {
            Matcher::Expression(regex) => regex.is_match(url),
            Matcher::Exact(exact) => url.to_lowercase() == *exact,
            Matcher::Prefix(prefix) => url.to_lowercase().starts_with(prefix.as_str()),
            Matcher::Contains(text) => url.to_lowercase().contains(text.as_str()),
        }
    }

    pub fn covers(&self, scheme: Scheme, host: Option<&str>) -> bool {
        if self.scope.scheme.is_some_and(|expected| expected != scheme) {
            return false;
        }

        match (&self.scope.host, host) {
            (Some(Host::Exact(expected)), Some(host)) => host.eq_ignore_ascii_case(expected),
            (Some(Host::Prefix(prefix)), Some(host)) => {
                host.to_ascii_lowercase().starts_with(prefix.as_str())
            }
            _ => true,
        }
    }
}

impl Scope {
    fn of(text: &str) -> Self {
        let lower = text.to_ascii_lowercase();

        let (scheme, rest) = if let Some(rest) = lower.strip_prefix("https://") {
            (Scheme::Https, rest)
        } else if let Some(rest) = lower.strip_prefix("http://") {
            (Scheme::Http, rest)
        } else {
            return Self::default();
        };

        let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
        let authority = authority.rsplit('@').next().unwrap_or_default();

        let host = if authority.starts_with('[') {
            let end = authority.find(['*', ']']).unwrap_or(authority.len());
            match authority.as_bytes().get(end) {
                Some(b']') => Host::Exact(authority[..=end].to_owned()),
                _ => Host::Prefix(authority[..end].to_owned()),
            }
        } else {
            match authority.find(['*', ':']) {
                Some(end) if authority.as_bytes()[end] == b'*' => {
                    Host::Prefix(authority[..end].to_owned())
                }
                Some(end) => Host::Exact(authority[..end].to_owned()),
                None => Host::Exact(authority.to_owned()),
            }
        };

        Self {
            scheme: Some(scheme),
            host: Some(host),
        }
    }
}

fn wildcard(text: &str) -> Result<Regex, String> {
    let expression: Vec<String> = text.split('*').map(regex::escape).collect();

    RegexBuilder::new(&format!("^{}$", expression.join(".*")))
        .case_insensitive(true)
        .build()
        .map_err(|error| error.to_string())
}
