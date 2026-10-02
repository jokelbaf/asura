use std::fs;
use std::path::{Path, PathBuf};

use super::{entries, stem};
use crate::{Prefix, Source};

pub fn prefixes(config: &Path, data: &Path) -> Vec<Prefix> {
    [config.join("lutris/games"), data.join("lutris/games")]
        .iter()
        .flat_map(|directory| entries(directory))
        .filter(|path| path.extension().is_some_and(|extension| extension == "yml"))
        .filter_map(|path| {
            let contents = fs::read_to_string(&path).ok()?;
            let prefix = contents
                .lines()
                .find_map(|line| line.trim().strip_prefix("prefix:"))?
                .trim()
                .trim_matches(['\'', '"']);

            Some(Prefix::new(
                PathBuf::from(prefix),
                title(&stem(&path)),
                Source::Lutris,
            ))
        })
        .collect()
}

fn title(slug: &str) -> String {
    let words: Vec<&str> = slug.split('-').collect();
    let words = match words.last() {
        Some(last) if last.chars().all(|character| character.is_ascii_digit()) => {
            &words[..words.len() - 1]
        }
        _ => &words[..],
    };

    words.join(" ")
}
