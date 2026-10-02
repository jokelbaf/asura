use std::env;
use std::thread;
use std::time::Duration;

use anyhow::{Context, bail};

const GUI: &str = "asura";
const ATTEMPTS: usize = 150;
const INTERVAL: Duration = Duration::from_millis(100);
const FORWARDED: [&str; 10] = [
    "DISPLAY",
    "WAYLAND_DISPLAY",
    "XDG_RUNTIME_DIR",
    "XDG_SESSION_TYPE",
    "XDG_CURRENT_DESKTOP",
    "DBUS_SESSION_BUS_ADDRESS",
    "HOME",
    "PATH",
    "LANG",
    "XAUTHORITY",
];

pub fn ensure() -> anyhow::Result<()> {
    if control::reachable() {
        return Ok(());
    }

    let program = env::current_exe()?.with_file_name(GUI);
    let environment: Vec<String> = FORWARDED
        .iter()
        .filter_map(|name| Some(format!("{name}={}", env::var(name).ok()?)))
        .collect();

    capture::start_service(GUI, &program.to_string_lossy(), &[], &environment)
        .context("starting the Asura application")?;

    for _ in 0..ATTEMPTS {
        thread::sleep(INTERVAL);

        if control::reachable() {
            return Ok(());
        }
    }

    bail!("the Asura application did not come up")
}
