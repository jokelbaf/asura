pub fn form(bytes: &[u8]) -> Vec<(String, String)> {
    url::form_urlencoded::parse(bytes.trim_ascii())
        .map(|(name, value)| (name.into_owned(), value.into_owned()))
        .collect()
}
