use std::ffi::OsString;
use std::path::{Path, PathBuf};

use directories::BaseDirs;

use crate::{Prefix, Source};

pub fn detect(variable: impl Fn(&str) -> Option<OsString>, command: &[String]) -> Option<Prefix> {
    if let Some(data) = variable("STEAM_COMPAT_DATA_PATH") {
        let app = variable("SteamAppId")
            .and_then(|app| app.to_str()?.parse().ok())
            .unwrap_or_default();

        return Some(Prefix::new(
            PathBuf::from(data).join("pfx"),
            "Steam",
            Source::Steam { app },
        ));
    }

    if let Some(path) = variable("WINEPREFIX") {
        return Some(Prefix::new(
            PathBuf::from(path),
            "WINEPREFIX",
            Source::Custom,
        ));
    }

    let wine = command.iter().any(|argument| {
        Path::new(argument)
            .file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with("wine"))
    });

    wine.then(BaseDirs::new)
        .flatten()
        .map(|directories| Prefix::new(directories.home_dir().join(".wine"), "Wine", Source::Wine))
}
