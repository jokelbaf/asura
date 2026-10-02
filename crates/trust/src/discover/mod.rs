mod bottles;
mod heroic;
mod lutris;
mod steam;

use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use directories::BaseDirs;

use crate::{Prefix, Source};

pub fn discover() -> Vec<Prefix> {
    let Some(directories) = BaseDirs::new() else {
        return Vec::new();
    };

    let home = directories.home_dir();
    let mut prefixes = Vec::new();

    if let Some(path) = env::var_os("WINEPREFIX") {
        prefixes.push(Prefix::new(
            PathBuf::from(path),
            "WINEPREFIX",
            Source::Custom,
        ));
    }

    prefixes.push(Prefix::new(
        home.join(".wine"),
        "Default Wine prefix",
        Source::Wine,
    ));
    prefixes.extend(steam::prefixes(home));
    prefixes.extend(lutris::prefixes(
        directories.config_dir(),
        directories.data_dir(),
    ));
    prefixes.extend(heroic::prefixes(directories.config_dir()));
    prefixes.extend(bottles::prefixes(home, directories.data_dir()));

    let mut seen = HashSet::new();
    prefixes.retain(|prefix| is_prefix(&prefix.path) && seen.insert(canonical(&prefix.path)));
    prefixes
}

fn is_prefix(path: &Path) -> bool {
    path.join("system.reg").is_file() || path.join("drive_c").is_dir()
}

fn canonical(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn entries(directory: &Path) -> impl Iterator<Item = PathBuf> + use<> {
    fs::read_dir(directory)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
}

fn stem(path: &Path) -> String {
    path.file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default()
}
