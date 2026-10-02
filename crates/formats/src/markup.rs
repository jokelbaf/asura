use memchr::{memchr, memmem};

use crate::{css, javascript, json};

const INDENT: &str = "  ";
const INLINE: usize = 100;

const VOID: [&str; 16] = [
    "area", "base", "br", "col", "command", "embed", "hr", "img", "input", "keygen", "link",
    "meta", "param", "source", "track", "wbr",
];

const RAW: [&str; 4] = ["script", "style", "pre", "textarea"];

const BLOCKS: [&str; 26] = [
    "address",
    "article",
    "aside",
    "blockquote",
    "details",
    "div",
    "dl",
    "fieldset",
    "figure",
    "footer",
    "form",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "header",
    "hr",
    "main",
    "nav",
    "ol",
    "p",
    "pre",
    "section",
    "table",
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Dialect {
    Xml,
    Html,
}

enum Token<'a> {
    Open {
        name: String,
        text: &'a str,
        closed: bool,
    },
    Close {
        name: String,
        text: &'a str,
    },
    Text(&'a str),
    Raw {
        text: &'a str,
        kind: Raw,
    },
    Other(&'a str),
}

#[derive(Clone, Copy)]
enum Raw {
    Script,
    Json,
    Style,
    Verbatim,
}

pub fn xml(text: &str) -> String {
    render(&tokens(text, Dialect::Xml), Dialect::Xml)
}

pub fn html(text: &str) -> String {
    render(&tokens(text, Dialect::Html), Dialect::Html)
}

fn tokens(source: &str, dialect: Dialect) -> Vec<Token<'_>> {
    let bytes = source.as_bytes();
    let mut tokens = Vec::new();
    let mut position = 0;
    let mut text = 0;

    while let Some(offset) = memchr(b'<', &bytes[position..]) {
        let start = position + offset;

        let Some((token, end)) = markup(source, start, dialect) else {
            position = start + 1;
            continue;
        };

        if start > text {
            tokens.push(Token::Text(&source[text..start]));
        }

        let raw = match &token {
            Token::Open {
                name,
                text,
                closed: false,
            } if dialect == Dialect::Html && RAW.contains(&name.as_str()) => {
                Some((name.clone(), kind(name, text)))
            }
            _ => None,
        };

        tokens.push(token);
        position = end;
        text = end;

        if let Some((name, kind)) = raw {
            let close = closing(source, end, &name);

            if close > end {
                tokens.push(Token::Raw {
                    text: &source[end..close],
                    kind,
                });
            }

            position = close;
            text = close;
        }
    }

    if text < source.len() {
        tokens.push(Token::Text(&source[text..]));
    }

    tokens
}

fn markup(source: &str, start: usize, dialect: Dialect) -> Option<(Token<'_>, usize)> {
    let rest = &source[start..];
    let until = |terminator: &str, from: usize| {
        memmem::find(&rest.as_bytes()[from..], terminator.as_bytes())
            .map_or(rest.len(), |end| from + end + terminator.len())
    };

    let other = |end: usize| Some((Token::Other(&rest[..end]), start + end));

    if rest.starts_with("<!--") {
        return other(until("-->", 4));
    }

    if rest.starts_with("<![CDATA[") {
        return other(until("]]>", 9));
    }

    if rest.starts_with("<?") {
        return other(until("?>", 2));
    }

    if rest.starts_with("<!") {
        return other(until(">", 2));
    }

    if let Some(after) = rest.strip_prefix("</") {
        let name = name(after, dialect)?;
        let end = until(">", 2);
        return Some((
            Token::Close {
                name,
                text: &rest[..end],
            },
            start + end,
        ));
    }

    let name = name(&rest[1..], dialect)?;
    let end = tag_end(rest);
    let text = &rest[..end];
    let closed = text.trim_end_matches('>').trim_end().ends_with('/');

    Some((Token::Open { name, text, closed }, start + end))
}

fn name(text: &str, dialect: Dialect) -> Option<String> {
    let first = text.chars().next()?;

    if !(first.is_alphabetic() || first == '_' || (dialect == Dialect::Xml && first == ':')) {
        return None;
    }

    let end = text
        .find(|character: char| character.is_whitespace() || matches!(character, '/' | '>'))
        .unwrap_or(text.len());

    Some(match dialect {
        Dialect::Html => text[..end].to_ascii_lowercase(),
        Dialect::Xml => text[..end].to_owned(),
    })
}

fn tag_end(tag: &str) -> usize {
    let mut quote = None;

    for (index, character) in tag.char_indices() {
        match (quote, character) {
            (Some(open), _) if character == open => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(character),
            (None, '>') => return index + 1,
            _ => {}
        }
    }

    tag.len()
}

fn closing(source: &str, from: usize, name: &str) -> usize {
    let lower = source[from..].to_ascii_lowercase();
    let needle = format!("</{name}");

    memmem::find(lower.as_bytes(), needle.as_bytes()).map_or(source.len(), |offset| from + offset)
}

fn kind(name: &str, tag: &str) -> Raw {
    let tag = tag.to_ascii_lowercase();
    let typed = tag
        .split_once("type=")
        .map(|(_, rest)| rest.trim_start_matches(['"', '\'']));

    match name {
        "style" => Raw::Style,
        "script" => match typed {
            None => Raw::Script,
            Some(kind)
                if kind.starts_with("module")
                    || kind.contains("javascript")
                    || kind.contains("ecmascript") =>
            {
                Raw::Script
            }
            Some(kind) if kind.contains("json") => Raw::Json,
            Some(_) => Raw::Verbatim,
        },
        _ => Raw::Verbatim,
    }
}

fn render(tokens: &[Token<'_>], dialect: Dialect) -> String {
    let mut output = String::new();
    let mut stack: Vec<&str> = Vec::new();
    let mut index = 0;

    while index < tokens.len() {
        match &tokens[index] {
            Token::Open { name, text, closed } => {
                if dialect == Dialect::Html {
                    while stack.last().is_some_and(|open| implies_close(name, open)) {
                        stack.pop();
                    }
                }

                let void = *closed || (dialect == Dialect::Html && VOID.contains(&name.as_str()));

                if !void && let Some((line, used)) = inline(tokens, index, name) {
                    push(&mut output, stack.len(), &line);
                    index += used;
                    continue;
                }

                push(&mut output, stack.len(), text);

                if !void {
                    stack.push(name);
                }
            }
            Token::Close { name, text } => {
                if let Some(position) = stack.iter().rposition(|open| open == name) {
                    stack.truncate(position);
                }

                push(&mut output, stack.len(), text);
            }
            Token::Text(text) => {
                for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
                    push(&mut output, stack.len(), line);
                }
            }
            Token::Raw { text, kind } => raw(&mut output, stack.len(), text, *kind),
            Token::Other(text) => push(&mut output, stack.len(), text.trim()),
        }

        index += 1;
    }

    output
}

fn inline(tokens: &[Token<'_>], index: usize, name: &str) -> Option<(String, usize)> {
    let Token::Open { text: open, .. } = &tokens[index] else {
        return None;
    };

    let closes = |token: Option<&Token<'_>>| matches!(token, Some(Token::Close { name: closing, .. }) if closing == name);

    match tokens.get(index + 1) {
        Some(Token::Close { text: close, .. }) if closes(tokens.get(index + 1)) => {
            Some((format!("{open}{close}"), 2))
        }
        Some(Token::Text(text)) if closes(tokens.get(index + 2)) => {
            let Some(Token::Close { text: close, .. }) = tokens.get(index + 2) else {
                return None;
            };

            let text = text.trim();
            let line = format!("{open}{text}{close}");
            (!text.contains('\n') && line.len() <= INLINE).then_some((line, 3))
        }
        _ => None,
    }
}

fn implies_close(opening: &str, open: &str) -> bool {
    match opening {
        "li" => open == "li",
        "dt" | "dd" => matches!(open, "dt" | "dd"),
        "tr" => matches!(open, "tr" | "td" | "th"),
        "td" | "th" => matches!(open, "td" | "th"),
        "thead" | "tbody" | "tfoot" => {
            matches!(open, "thead" | "tbody" | "tfoot" | "tr" | "td" | "th")
        }
        "option" => open == "option",
        "optgroup" => matches!(open, "optgroup" | "option"),
        _ if BLOCKS.contains(&opening) => open == "p",
        _ => false,
    }
}

fn raw(output: &mut String, depth: usize, text: &str, kind: Raw) {
    let formatted = match kind {
        Raw::Script => javascript(text),
        Raw::Json => json(text),
        Raw::Style => css(text),
        Raw::Verbatim => {
            output.push_str(text);

            if !text.ends_with('\n') {
                output.push('\n');
            }

            return;
        }
    };

    for line in formatted.lines() {
        if line.trim().is_empty() {
            output.push('\n');
        } else {
            push(output, depth, line);
        }
    }
}

fn push(output: &mut String, depth: usize, text: &str) {
    output.extend(std::iter::repeat_n(INDENT, depth));
    output.push_str(text);
    output.push('\n');
}
