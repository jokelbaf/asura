use std::fmt::Write;

const INDENT: &str = "  ";
const MAX_DEPTH: usize = 32;
const MAX_FIELD: u64 = (1 << 29) - 1;
const PREVIEW: usize = 64;

enum Value<'a> {
    Varint(u64),
    Fixed64(u64),
    Fixed32(u32),
    Bytes(&'a [u8]),
    Group(Vec<Field<'a>>),
}

struct Field<'a> {
    number: u64,
    value: Value<'a>,
}

enum Reading<'a> {
    Text(&'a str),
    Message(Vec<Field<'a>>),
    Binary,
}

pub fn protobuf(bytes: &[u8]) -> Option<String> {
    let fields = message(bytes, 0).filter(|fields| !fields.is_empty())?;
    let mut output = String::new();
    render(&fields, 0, &mut output);
    Some(output)
}

fn message(bytes: &[u8], depth: usize) -> Option<Vec<Field<'_>>> {
    let mut reader = Reader { bytes };
    reader.fields(depth, None)
}

struct Reader<'a> {
    bytes: &'a [u8],
}

impl<'a> Reader<'a> {
    fn fields(&mut self, depth: usize, group: Option<u64>) -> Option<Vec<Field<'a>>> {
        if depth > MAX_DEPTH {
            return None;
        }

        let mut fields = Vec::new();

        while !self.bytes.is_empty() {
            let key = self.varint()?;
            let number = key >> 3;

            if number == 0 || number > MAX_FIELD {
                return None;
            }

            let value = match key & 7 {
                0 => Value::Varint(self.varint()?),
                1 => Value::Fixed64(u64::from_le_bytes(self.take(8)?.try_into().ok()?)),
                2 => {
                    let length = usize::try_from(self.varint()?).ok()?;
                    Value::Bytes(self.take(length)?)
                }
                3 => Value::Group(self.fields(depth + 1, Some(number))?),
                4 => return (group == Some(number)).then_some(fields),
                5 => Value::Fixed32(u32::from_le_bytes(self.take(4)?.try_into().ok()?)),
                _ => return None,
            };

            fields.push(Field { number, value });
        }

        group.is_none().then_some(fields)
    }

    fn varint(&mut self) -> Option<u64> {
        let mut value = 0u64;

        for (index, &byte) in self.bytes.iter().enumerate().take(10) {
            value |= u64::from(byte & 0x7f) << (7 * index);

            if byte & 0x80 == 0 {
                self.bytes = &self.bytes[index + 1..];
                return Some(value);
            }
        }

        None
    }

    fn take(&mut self, length: usize) -> Option<&'a [u8]> {
        if length > self.bytes.len() {
            return None;
        }

        let (taken, rest) = self.bytes.split_at(length);
        self.bytes = rest;
        Some(taken)
    }
}

fn render(fields: &[Field<'_>], depth: usize, output: &mut String) {
    for field in fields {
        output.extend(std::iter::repeat_n(INDENT, depth));
        let number = field.number;

        match &field.value {
            Value::Varint(value) if *value > i64::MAX as u64 => {
                let _ = write!(output, "{number}: {value} ({})", *value as i64);
            }
            Value::Varint(value) => {
                let _ = write!(output, "{number}: {value}");
            }
            Value::Fixed64(value) => {
                let _ = write!(
                    output,
                    "{number}: 0x{value:016x} (int {}, double {})",
                    *value as i64,
                    float(f64::from_bits(*value))
                );
            }
            Value::Fixed32(value) => {
                let _ = write!(
                    output,
                    "{number}: 0x{value:08x} (int {}, float {})",
                    *value as i32,
                    float(f64::from(f32::from_bits(*value)))
                );
            }
            Value::Group(inner) => nested(output, number, inner, depth),
            Value::Bytes(bytes) => match read(bytes, depth) {
                Reading::Text(text) => {
                    let _ = write!(output, "{number}: {text:?}");
                }
                Reading::Message(inner) => nested(output, number, &inner, depth),
                Reading::Binary => {
                    let _ = write!(
                        output,
                        "{number}: <{} bytes: {}>",
                        bytes.len(),
                        preview(bytes)
                    );
                }
            },
        }

        output.push('\n');
    }
}

fn nested(output: &mut String, number: u64, fields: &[Field<'_>], depth: usize) {
    let _ = writeln!(output, "{number} {{");
    render(fields, depth + 1, output);
    output.extend(std::iter::repeat_n(INDENT, depth));
    output.push('}');
}

fn read(bytes: &[u8], depth: usize) -> Reading<'_> {
    let text = std::str::from_utf8(bytes).ok().filter(|text| {
        text.chars()
            .all(|character| !character.is_control() || matches!(character, '\n' | '\r' | '\t'))
    });

    let nested = || message(bytes, depth + 1).filter(|fields| !fields.is_empty());

    match text {
        Some(text) if bytes.first() == Some(&b'\n') => {
            nested().map_or(Reading::Text(text), Reading::Message)
        }
        Some(text) => Reading::Text(text),
        None => nested().map_or(Reading::Binary, Reading::Message),
    }
}

fn preview(bytes: &[u8]) -> String {
    let mut text: Vec<String> = bytes
        .iter()
        .take(PREVIEW)
        .map(|byte| format!("{byte:02x}"))
        .collect();

    if bytes.len() > PREVIEW {
        text.push(String::from("…"));
    }

    text.join(" ")
}

fn float(value: f64) -> String {
    let magnitude = value.abs();

    if value == 0.0 || !value.is_finite() || (1e-6..1e15).contains(&magnitude) {
        format!("{value}")
    } else {
        format!("{value:e}")
    }
}
