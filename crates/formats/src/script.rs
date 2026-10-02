const INDENT: &str = "  ";

const EXPRESSION_KEYWORDS: [&str; 15] = [
    "return",
    "typeof",
    "instanceof",
    "in",
    "of",
    "new",
    "delete",
    "void",
    "throw",
    "case",
    "do",
    "else",
    "yield",
    "await",
    "extends",
];

const CONTINUATIONS: [&str; 3] = ["else", "catch", "finally"];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Dialect {
    JavaScript,
    Css,
}

#[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
enum Break {
    #[default]
    None,
    Space,
    Line,
    Blank,
}

#[derive(Clone, Copy)]
enum Token<'a> {
    Space(usize),
    Comment(&'a str),
    LineComment(&'a str),
    Literal(&'a str),
    Word(&'a str),
    Punct(char),
}

pub fn javascript(source: &str) -> String {
    format(source, Dialect::JavaScript)
}

pub fn css(source: &str) -> String {
    format(source, Dialect::Css)
}

fn format(source: &str, dialect: Dialect) -> String {
    let mut writer = Writer::default();
    let mut brackets = Vec::new();
    let mut opened = false;
    let mut closed = false;

    for token in Lexer::new(source, dialect) {
        if closed && !matches!(token, Token::Space(_)) {
            closed = false;

            match (dialect, token) {
                (_, Token::Punct(';')) => writer.pending = Break::None,
                (Dialect::JavaScript, Token::Punct(',' | ')' | ']' | '.' | ':' | '(')) => {
                    writer.pending = Break::None
                }
                (Dialect::JavaScript, Token::Word(word)) if CONTINUATIONS.contains(&word) => {
                    writer.pending = Break::Space
                }
                _ => {}
            }
        }

        match token {
            Token::Space(newlines) => writer.request(match newlines {
                0 => Break::Space,
                1 => Break::Line,
                _ => Break::Blank,
            }),
            Token::Comment(text) => writer.emit(text),
            Token::LineComment(text) => {
                writer.emit(text);
                writer.request(Break::Line);
            }
            Token::Literal(text) | Token::Word(text) => writer.emit(text),
            Token::Punct('{') => {
                if writer.last().is_some_and(spaced) {
                    writer.request(Break::Space);
                }

                writer.emit("{");
                writer.depth += 1;
                writer.request(Break::Line);
                brackets.push('{');
            }
            Token::Punct('}') => {
                writer.depth = writer.depth.saturating_sub(1);
                writer.pending = if opened {
                    Break::None
                } else {
                    writer.pending.max(Break::Line)
                };
                writer.emit("}");
                writer.request(Break::Line);
                brackets.pop();
                closed = true;
            }
            Token::Punct(';') => {
                writer.emit(";");

                if brackets.last() != Some(&'(') {
                    writer.request(Break::Line);
                }
            }
            Token::Punct(',') => {
                writer.emit(",");

                if dialect == Dialect::JavaScript && brackets.last() == Some(&'{') {
                    writer.request(Break::Line);
                }
            }
            Token::Punct(open @ ('(' | '[')) => {
                writer.emit_char(open);
                brackets.push(open);
            }
            Token::Punct(close @ (')' | ']')) => {
                writer.emit_char(close);
                brackets.pop();
            }
            Token::Punct(other) => writer.emit_char(other),
        }

        if !matches!(token, Token::Space(_)) {
            opened = matches!(token, Token::Punct('{'));
        }
    }

    writer.finish()
}

fn spaced(character: char) -> bool {
    character.is_alphanumeric() || matches!(character, ')' | ']' | '_' | '$' | '"' | '\'' | '>')
}

#[derive(Default)]
struct Writer {
    output: String,
    depth: usize,
    pending: Break,
    started: bool,
}

impl Writer {
    fn request(&mut self, wanted: Break) {
        self.pending = self.pending.max(wanted);
    }

    fn last(&self) -> Option<char> {
        self.output.chars().next_back()
    }

    fn emit_char(&mut self, character: char) {
        let mut buffer = [0; 4];
        self.emit(character.encode_utf8(&mut buffer));
    }

    fn emit(&mut self, text: &str) {
        let breaks = match self.pending {
            Break::Line => 1,
            Break::Blank => 2,
            _ => 0,
        };

        let line_start = !self.started || self.output.ends_with('\n');

        if breaks > 0 && self.started {
            let trimmed = self.output.trim_end_matches([' ', '\t']).len();
            self.output.truncate(trimmed);

            let existing = self.output.len() - self.output.trim_end_matches('\n').len();

            for _ in existing..breaks {
                self.output.push('\n');
            }
        } else if self.pending == Break::Space && !line_start {
            self.output.push(' ');
        }

        if self.output.ends_with('\n') || !self.started {
            self.output.extend(std::iter::repeat_n(INDENT, self.depth));
        }

        self.output.push_str(text);
        self.pending = Break::None;
        self.started = true;
    }

    fn finish(mut self) -> String {
        let trimmed = self.output.trim_end().len();
        self.output.truncate(trimmed);
        self.output.push('\n');
        self.output
    }
}

struct Lexer<'a> {
    source: &'a str,
    position: usize,
    dialect: Dialect,
    expression: bool,
}

impl<'a> Lexer<'a> {
    fn new(source: &'a str, dialect: Dialect) -> Self {
        Self {
            source,
            position: 0,
            dialect,
            expression: true,
        }
    }

    fn lex(&self, rest: &'a str) -> (Token<'a>, usize) {
        let first = rest.chars().next().unwrap_or_default();
        let script = self.dialect == Dialect::JavaScript;

        if first.is_whitespace() {
            let length = rest
                .find(|character: char| !character.is_whitespace())
                .unwrap_or(rest.len());
            return (Token::Space(rest[..length].matches('\n').count()), length);
        }

        if let Some(inside) = rest.strip_prefix("/*") {
            let length = inside.find("*/").map_or(rest.len(), |end| end + 4);
            return (Token::Comment(&rest[..length]), length);
        }

        if script && rest.starts_with("//") {
            let length = rest.find('\n').unwrap_or(rest.len());
            return (Token::LineComment(rest[..length].trim_end()), length);
        }

        if matches!(first, '"' | '\'') || (script && first == '`') {
            let length = quoted(rest, first);
            return (Token::Literal(&rest[..length]), length);
        }

        if script
            && first == '/'
            && self.expression
            && let Some(length) = regex(rest)
        {
            return (Token::Literal(&rest[..length]), length);
        }

        if word(first) {
            let length = rest
                .find(|character: char| !word(character))
                .unwrap_or(rest.len());
            return (Token::Word(&rest[..length]), length);
        }

        (Token::Punct(first), first.len_utf8())
    }
}

impl<'a> Iterator for Lexer<'a> {
    type Item = Token<'a>;

    fn next(&mut self) -> Option<Token<'a>> {
        let rest = &self.source[self.position..];

        if rest.is_empty() {
            return None;
        }

        let (token, length) = self.lex(rest);
        self.position += length;

        match token {
            Token::Word(word) => self.expression = EXPRESSION_KEYWORDS.contains(&word),
            Token::Punct(character) => self.expression = !matches!(character, ')' | ']' | '}'),
            Token::Literal(_) => self.expression = false,
            _ => {}
        }

        Some(token)
    }
}

fn word(character: char) -> bool {
    character.is_alphanumeric() || matches!(character, '_' | '$')
}

fn quoted(text: &str, quote: char) -> usize {
    let mut index = quote.len_utf8();

    while let Some(character) = text[index..].chars().next() {
        match character {
            '\\' => {
                index += 1;
                index += text[index..].chars().next().map_or(0, char::len_utf8);
                continue;
            }
            '\n' if quote != '`' => return index,
            '$' if quote == '`' && text[index + 1..].starts_with('{') => {
                index += 2 + substitution(&text[index + 2..]);
                continue;
            }
            _ if character == quote => return index + character.len_utf8(),
            _ => {}
        }

        index += character.len_utf8();
    }

    text.len()
}

fn substitution(text: &str) -> usize {
    let mut depth = 1;
    let mut index = 0;

    while index < text.len() {
        let character = text[index..].chars().next().unwrap_or_default();

        match character {
            '{' => depth += 1,
            '}' => {
                depth -= 1;

                if depth == 0 {
                    return index + 1;
                }
            }
            '"' | '\'' | '`' => {
                index += quoted(&text[index..], character);
                continue;
            }
            _ => {}
        }

        index += character.len_utf8();
    }

    text.len()
}

fn regex(text: &str) -> Option<usize> {
    let mut class = false;
    let mut characters = text.char_indices().skip(1);

    while let Some((index, character)) = characters.next() {
        match character {
            '\\' => {
                characters.next();
            }
            '\n' => return None,
            '[' => class = true,
            ']' => class = false,
            '/' if !class => {
                let flags = text[index + 1..]
                    .find(|character: char| !character.is_ascii_alphabetic())
                    .unwrap_or(text.len() - index - 1);
                return Some(index + 1 + flags);
            }
            _ => {}
        }
    }

    None
}
