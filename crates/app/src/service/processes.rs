use std::collections::HashMap;
use std::ffi::OsString;
use std::fs;
use std::os::unix::ffi::OsStringExt;
use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct Process {
    pub pid: u32,
    pub parent: u32,
    pub name: String,
    pub command: Vec<String>,
    pub executable: Option<PathBuf>,
}

impl Process {
    pub fn is_windows(&self) -> bool {
        self.command
            .first()
            .is_some_and(|program| program.to_ascii_lowercase().ends_with(".exe"))
    }

    pub fn command_line(&self) -> String {
        self.command.join(" ")
    }

    pub fn program(&self) -> Vec<String> {
        self.executable
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .chain(self.command.iter().cloned())
            .collect()
    }
}

pub fn list() -> Vec<Process> {
    let Ok(user) = fs::metadata("/proc/self").map(|metadata| metadata.uid()) else {
        return Vec::new();
    };

    let own = std::process::id();

    fs::read_dir("/proc")
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| {
            entry
                .metadata()
                .is_ok_and(|metadata| metadata.uid() == user)
        })
        .filter_map(|entry| entry.file_name().to_str()?.parse::<u32>().ok())
        .filter(|&pid| pid != own)
        .filter_map(read)
        .collect()
}

pub fn tree(processes: &[Process], root: u32) -> Vec<u32> {
    let mut members = vec![root];
    let mut index = 0;

    while let Some(&parent) = members.get(index) {
        members.extend(
            processes
                .iter()
                .filter(|process| process.parent == parent)
                .map(|process| process.pid),
        );
        index += 1;
    }

    members
}

pub fn environment(pid: u32) -> HashMap<String, OsString> {
    fs::read(format!("/proc/{pid}/environ"))
        .unwrap_or_default()
        .split(|&byte| byte == 0)
        .filter_map(|variable| {
            let separator = variable.iter().position(|&byte| byte == b'=')?;
            let name = String::from_utf8(variable[..separator].to_vec()).ok()?;
            Some((name, OsString::from_vec(variable[separator + 1..].to_vec())))
        })
        .collect()
}

fn read(pid: u32) -> Option<Process> {
    let command: Vec<String> = fs::read(format!("/proc/{pid}/cmdline"))
        .ok()?
        .split(|&byte| byte == 0)
        .filter(|argument| !argument.is_empty())
        .map(|argument| String::from_utf8_lossy(argument).into_owned())
        .collect();

    let program = command.first()?;
    let name = program
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(program)
        .to_owned();

    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let parent = stat[stat.rfind(')')? + 2..]
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()?;

    Some(Process {
        pid,
        parent,
        name,
        executable: fs::read_link(format!("/proc/{pid}/exe")).ok(),
        command,
    })
}
