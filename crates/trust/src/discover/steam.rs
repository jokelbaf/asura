use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use super::{canonical, entries};
use crate::{Prefix, Source, vdf};

const ROOTS: [&str; 3] = [
    ".steam/steam",
    ".local/share/Steam",
    ".var/app/com.valvesoftware.Steam/.local/share/Steam",
];

pub fn prefixes(home: &Path) -> Vec<Prefix> {
    let libraries = libraries(home);

    libraries
        .iter()
        .flat_map(|library| entries(&library.join("steamapps/compatdata")))
        .filter_map(|data| {
            let app: u32 = data.file_name()?.to_str()?.parse().ok()?;
            let name = title(&libraries, app).unwrap_or_else(|| format!("Steam app {app}"));
            Some(Prefix::new(data.join("pfx"), name, Source::Steam { app }))
        })
        .collect()
}

fn libraries(home: &Path) -> Vec<PathBuf> {
    let mut seen = HashSet::new();

    ROOTS
        .iter()
        .map(|root| home.join(root))
        .filter(|root| root.is_dir())
        .flat_map(|root| {
            let folders =
                fs::read_to_string(root.join("steamapps/libraryfolders.vdf")).unwrap_or_default();
            let mut found: Vec<PathBuf> = vdf::values(&folders, "path")
                .into_iter()
                .map(PathBuf::from)
                .collect();
            found.push(root);
            found
        })
        .filter(|library| library.join("steamapps").is_dir() && seen.insert(canonical(library)))
        .collect()
}

fn title(libraries: &[PathBuf], app: u32) -> Option<String> {
    libraries.iter().find_map(|library| {
        let manifest =
            fs::read_to_string(library.join(format!("steamapps/appmanifest_{app}.acf"))).ok()?;
        vdf::values(&manifest, "name").into_iter().next()
    })
}
