pub fn json(text: &str) -> String {
    let mut output = String::with_capacity(text.len() * 2);
    let mut characters = text.chars().peekable();
    let mut depth = 0;
    let mut quoted = false;
    let mut escaped = false;

    while let Some(character) = characters.next() {
        if quoted {
            output.push(character);

            match character {
                _ if escaped => escaped = false,
                '\\' => escaped = true,
                '"' => quoted = false,
                _ => {}
            }

            continue;
        }

        match character {
            '"' => {
                quoted = true;
                output.push(character);
            }
            '{' | '[' => {
                output.push(character);
                while characters.next_if(|next| next.is_whitespace()).is_some() {}

                let closing = if character == '{' { '}' } else { ']' };

                if let Some(closing) = characters.next_if_eq(&closing) {
                    output.push(closing);
                } else {
                    depth += 1;
                    indent(&mut output, depth);
                }
            }
            '}' | ']' => {
                depth = depth.saturating_sub(1);
                indent(&mut output, depth);
                output.push(character);
            }
            ',' => {
                output.push(character);
                indent(&mut output, depth);
            }
            ':' => output.push_str(": "),
            _ if character.is_whitespace() => {}
            _ => output.push(character),
        }
    }

    output
}

fn indent(output: &mut String, depth: usize) {
    output.push('\n');
    output.extend(std::iter::repeat_n("  ", depth));
}
