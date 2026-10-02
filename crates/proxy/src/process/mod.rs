mod socket;

use std::fs;
use std::net::SocketAddr;
use std::num::NonZeroUsize;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use lru::LruCache;
use parking_lot::Mutex;
use rustix::param::clock_ticks_per_second;
use session::Client;

use crate::Route;

const CAPACITY: NonZeroUsize = NonZeroUsize::new(512).unwrap();
const RELAY: &str = "wineserver";
const WINE: &str = "wine";
const DELETED: &str = " (deleted)";
const BOOT: &str = "btime ";

pub struct Processes {
    cache: Mutex<LruCache<(u32, u64), Client>>,
}

impl Default for Processes {
    fn default() -> Self {
        Self {
            cache: Mutex::new(LruCache::new(CAPACITY)),
        }
    }
}

impl Processes {
    pub fn describe(&self, route: &Route, peer: SocketAddr, local: SocketAddr) -> Client {
        let process = if route.command == RELAY {
            socket::owner(route.process, peer, local).unwrap_or(route.process)
        } else {
            route.process
        };

        let mut client = self.inspect(process).unwrap_or_else(|| fallback(route));
        client.target = route.target;
        client
    }

    fn inspect(&self, process: u32) -> Option<Client> {
        let key = (process, started(process)?);

        if let Some(client) = self.cache.lock().get(&key) {
            return Some(client.clone());
        }

        let client = read(process)?;
        self.cache.lock().put(key, client.clone());
        Some(client)
    }
}

pub fn fallback(route: &Route) -> Client {
    Client {
        process: route.process,
        name: route.command.clone(),
        command_line: String::new(),
        target: route.target,
    }
}

fn read(process: u32) -> Option<Client> {
    let raw = fs::read(format!("/proc/{process}/cmdline")).ok()?;
    let arguments: Vec<String> = raw
        .split(|&byte| byte == 0)
        .filter(|argument| !argument.is_empty())
        .map(|argument| String::from_utf8_lossy(argument).into_owned())
        .collect();

    Some(Client {
        process,
        name: name(process, arguments.first())?,
        command_line: arguments.join(" "),
        target: None,
    })
}

fn name(process: u32, program: Option<&String>) -> Option<String> {
    let program = program
        .and_then(|program| program.rsplit(['/', '\\']).next())
        .filter(|name| !name.is_empty());

    match (executable(process), program) {
        (Some(executable), Some(program)) if executable.starts_with(WINE) => {
            Some(program.to_owned())
        }
        (Some(executable), _) => Some(executable),
        (None, Some(program)) => Some(program.to_owned()),
        (None, None) => Some(
            fs::read_to_string(format!("/proc/{process}/comm"))
                .ok()?
                .trim()
                .to_owned(),
        ),
    }
}

fn executable(process: u32) -> Option<String> {
    let path = fs::read_link(format!("/proc/{process}/exe")).ok()?;
    let name = path.file_name()?.to_string_lossy();

    Some(name.strip_suffix(DELETED).unwrap_or(&name).to_owned())
}

pub fn started_at(process: u32) -> Option<SystemTime> {
    let ticks = started(process)?;
    let stat = fs::read_to_string("/proc/stat").ok()?;
    let boot: u64 = stat
        .lines()
        .find_map(|line| line.strip_prefix(BOOT))?
        .trim()
        .parse()
        .ok()?;

    Some(
        UNIX_EPOCH
            + Duration::from_secs(boot)
            + Duration::from_secs_f64(ticks as f64 / clock_ticks_per_second() as f64),
    )
}

fn started(process: u32) -> Option<u64> {
    let stat = fs::read_to_string(format!("/proc/{process}/stat")).ok()?;
    let fields = &stat[stat.rfind(')')? + 2..];
    fields.split_whitespace().nth(19)?.parse().ok()
}
