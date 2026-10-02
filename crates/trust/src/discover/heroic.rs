use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::{entries, stem};
use crate::{Prefix, Source};

pub fn prefixes(config: &Path) -> Vec<Prefix> {
    entries(&config.join("heroic/GamesConfig"))
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .filter_map(|path| {
            let document: Value = serde_json::from_str(&fs::read_to_string(&path).ok()?).ok()?;
            let name = stem(&path);
            let prefix = document.get(&name)?.get("winePrefix")?.as_str()?;
            Some(Prefix::new(PathBuf::from(prefix), name, Source::Heroic))
        })
        .collect()
}
