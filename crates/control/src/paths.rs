use std::env;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;

use directories::BaseDirs;

const NAME: &str = "asura";
const SOCKET: &str = "control.sock";

pub fn socket() -> PathBuf {
    let directories = BaseDirs::new();

    let runtime = directories
        .as_ref()
        .and_then(BaseDirs::runtime_dir)
        .map(|directory| directory.join(NAME))
        .unwrap_or_else(|| {
            let user = fs::metadata("/proc/self")
                .map(|metadata| metadata.uid())
                .unwrap_or_default();
            env::temp_dir().join(format!("{NAME}-{user}"))
        });

    runtime.join(SOCKET)
}

pub fn data() -> PathBuf {
    BaseDirs::new()
        .map(|directories| directories.data_dir().join(NAME))
        .unwrap_or_else(|| env::temp_dir().join(NAME))
}
