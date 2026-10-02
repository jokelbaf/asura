pub fn values(source: &str, key: &str) -> Vec<String> {
    let tokens: Vec<String> = tokens(source).collect();

    tokens
        .windows(2)
        .filter(|pair| pair[0].eq_ignore_ascii_case(key))
        .map(|pair| pair[1].clone())
        .collect()
}

fn tokens(source: &str) -> impl Iterator<Item = String> + '_ {
    let mut characters = source.chars().peekable();

    std::iter::from_fn(move || {
        loop {
            match characters.next()? {
                '"' => {
                    let mut token = String::new();

                    while let Some(character) = characters.next() {
                        match character {
                            '\\' => token.extend(characters.next()),
                            '"' => break,
                            other => token.push(other),
                        }
                    }

                    return Some(token);
                }
                '{' | '}' => return Some(String::new()),
                _ => {}
            }
        }
    })
}
