use std::fs;
use std::net::{IpAddr, SocketAddr};

type Encoder = fn(SocketAddr) -> Option<String>;

const TABLES: [(&str, Encoder); 2] = [("/proc/net/tcp", encode4), ("/proc/net/tcp6", encode6)];

pub fn owner(relay: u32, peer: SocketAddr, local: SocketAddr) -> Option<u32> {
    let link = format!("socket:[{}]", inode(peer, local)?);

    let membership = fs::read_to_string(format!("/proc/{relay}/cgroup")).ok()?;
    let cgroup = membership
        .lines()
        .find_map(|line| line.strip_prefix("0::"))?;
    let members = fs::read_to_string(format!("/sys/fs/cgroup{cgroup}/cgroup.procs")).ok()?;

    members
        .lines()
        .filter_map(|line| line.parse().ok())
        .filter(|&process| process != relay)
        .find(|&process| holds(process, &link))
}

fn inode(peer: SocketAddr, local: SocketAddr) -> Option<u64> {
    TABLES.iter().find_map(|(table, encode)| {
        let (source, destination) = (encode(peer)?, encode(local)?);

        fs::read_to_string(table)
            .ok()?
            .lines()
            .skip(1)
            .find_map(|line| {
                let fields: Vec<&str> = line.split_whitespace().collect();
                (fields.get(1) == Some(&source.as_str())
                    && fields.get(2) == Some(&destination.as_str()))
                .then(|| fields.get(9)?.parse().ok())
                .flatten()
            })
    })
}

fn holds(process: u32, link: &str) -> bool {
    fs::read_dir(format!("/proc/{process}/fd"))
        .into_iter()
        .flatten()
        .flatten()
        .any(|descriptor| {
            fs::read_link(descriptor.path()).is_ok_and(|target| target.as_os_str() == link)
        })
}

fn encode4(address: SocketAddr) -> Option<String> {
    let IpAddr::V4(ip) = address.ip() else {
        return None;
    };

    Some(format!(
        "{:08X}:{:04X}",
        u32::from_ne_bytes(ip.octets()),
        address.port()
    ))
}

fn encode6(address: SocketAddr) -> Option<String> {
    let octets = match address.ip() {
        IpAddr::V4(ip) => ip.to_ipv6_mapped().octets(),
        IpAddr::V6(ip) => ip.octets(),
    };

    let words: String = octets
        .as_chunks::<4>()
        .0
        .iter()
        .map(|chunk| format!("{:08X}", u32::from_ne_bytes(*chunk)))
        .collect();

    Some(format!("{words}:{:04X}", address.port()))
}
