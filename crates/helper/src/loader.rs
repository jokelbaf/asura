use std::fs::File;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};

use anyhow::{Context, Result, bail};
use aya::Ebpf;
use aya::maps::Map;
use aya::programs::Program;
use aya_obj::generated::bpf_attach_type::{self, *};
use capture::{Bundle, MAPS};

use crate::link;

const OBJECT: &[u8] = aya::include_bytes_aligned!(concat!(env!("OUT_DIR"), "/asura"));
const CGROUP_ROOT: &str = "/sys/fs/cgroup";

const HOOKS: [(&str, bpf_attach_type); 7] = [
    ("connect4", BPF_CGROUP_INET4_CONNECT),
    ("connect6", BPF_CGROUP_INET6_CONNECT),
    ("sendmsg4", BPF_CGROUP_UDP4_SENDMSG),
    ("sendmsg6", BPF_CGROUP_UDP6_SENDMSG),
    ("getpeername4", BPF_CGROUP_INET4_GETPEERNAME),
    ("getpeername6", BPF_CGROUP_INET6_GETPEERNAME),
    ("lifecycle", BPF_CGROUP_SOCK_OPS),
];

pub fn load() -> Result<Bundle> {
    let mut ebpf = Ebpf::load(OBJECT).context("loading eBPF object")?;
    let root = File::open(CGROUP_ROOT).context("opening cgroup root")?;

    let links = HOOKS
        .into_iter()
        .map(|(name, attach_type)| attach(&mut ebpf, name, attach_type, root.as_fd()))
        .collect::<Result<_>>()?;

    let maps = MAPS
        .into_iter()
        .map(|name| descriptor(&ebpf, name))
        .collect::<Result<_>>()?;

    Ok(Bundle { maps, links })
}

fn attach(
    ebpf: &mut Ebpf,
    name: &str,
    attach_type: bpf_attach_type,
    cgroup: BorrowedFd<'_>,
) -> Result<OwnedFd> {
    let program = ebpf
        .program_mut(name)
        .with_context(|| format!("missing program {name}"))?;

    match program {
        Program::CgroupSockAddr(program) => program.load(),
        Program::SockOps(program) => program.load(),
        _ => bail!("unexpected program type for {name}"),
    }
    .with_context(|| format!("loading program {name}"))?;

    link::create(program.fd()?.as_fd(), cgroup, attach_type)
        .with_context(|| format!("attaching program {name}"))
}

fn descriptor(ebpf: &Ebpf, name: &str) -> Result<OwnedFd> {
    let data = match ebpf.map(name) {
        Some(
            Map::Array(data) | Map::HashMap(data) | Map::LruHashMap(data) | Map::LpmTrie(data),
        ) => data,
        _ => bail!("missing map {name}"),
    };

    Ok(data.fd().as_fd().try_clone_to_owned()?)
}
