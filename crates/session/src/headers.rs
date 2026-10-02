use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Headers {
    fields: Vec<(String, Vec<u8>)>,
}

impl Headers {
    pub fn parse(text: &str) -> Self {
        let mut headers = Self::default();

        for line in text.lines() {
            if let Some((name, value)) = line.split_once(':')
                && !name.trim().is_empty()
            {
                headers.push(name.trim(), value.trim().as_bytes());
            }
        }

        headers
    }

    pub fn lines(&self) -> String {
        self.iter()
            .map(|(name, value)| format!("{name}: {}\n", String::from_utf8_lossy(value)))
            .collect()
    }

    pub fn push(&mut self, name: impl Into<String>, value: impl Into<Vec<u8>>) {
        self.fields.push((name.into(), value.into()));
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &[u8])> {
        self.fields
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_slice()))
    }

    pub fn get<'a>(&'a self, name: &str) -> Option<&'a [u8]> {
        self.all(name).next()
    }

    pub fn text<'a>(&'a self, name: &str) -> Option<&'a str> {
        self.get(name)
            .and_then(|value| std::str::from_utf8(value).ok())
    }

    pub fn all<'a, 'b>(&'a self, name: &'b str) -> impl Iterator<Item = &'a [u8]> + use<'a, 'b> {
        self.iter()
            .filter(move |(field, _)| field.eq_ignore_ascii_case(name))
            .map(|(_, value)| value)
    }

    pub fn contains_token(&self, name: &str, token: &str) -> bool {
        self.all(name)
            .filter_map(|value| std::str::from_utf8(value).ok())
            .flat_map(|value| value.split(','))
            .any(|item| item.trim().eq_ignore_ascii_case(token))
    }

    pub fn remove(&mut self, name: &str) {
        self.fields
            .retain(|(field, _)| !field.eq_ignore_ascii_case(name));
    }

    pub fn len(&self) -> usize {
        self.fields.len()
    }

    pub fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }
}
