use std::path::Path;

use super::entries;
use crate::{Prefix, Source};

pub fn prefixes(home: &Path, data: &Path) -> Vec<Prefix> {
    [
        data.join("bottles/bottles"),
        home.join(".var/app/com.usebottles.bottles/data/bottles/bottles"),
    ]
    .iter()
    .flat_map(|directory| entries(directory))
    .filter_map(|path| {
        let name = path.file_name()?.to_string_lossy().into_owned();
        Some(Prefix::new(path, name, Source::Bottles))
    })
    .collect()
}
