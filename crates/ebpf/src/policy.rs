use aya_ebpf::helpers::{bpf_get_current_ancestor_cgroup_id, bpf_get_current_pid_tgid};
use aya_ebpf::maps::lpm_trie::Key;
use common::{Endpoint, Settings};

use crate::maps::{EXCLUSIONS, SETTINGS, TARGETS};

const MAX_DEPTH: i32 = 16;
const QUIC_PORT: u16 = 443;

pub const ALLOW: i32 = 1;
pub const REJECT: i32 = 0;

pub struct Decision {
    pub target: u64,
    pub proxy_port: u16,
    block_quic: bool,
}

impl Decision {
    pub fn evaluate(destination: &Endpoint) -> Option<Self> {
        let settings = SETTINGS
            .get(0)
            .filter(|settings| settings.has(Settings::ENABLED))?;

        let route = if destination.is_mapped() {
            Settings::ROUTE4
        } else {
            Settings::ROUTE6
        };

        if !settings.has(route) {
            return None;
        }

        let target = target(settings)?;

        if EXCLUSIONS.get(Key::new(128, destination.address)).is_some() {
            return None;
        }

        Some(Self {
            target,
            proxy_port: settings.proxy_port,
            block_quic: settings.has(Settings::BLOCK_QUIC),
        })
    }

    pub fn datagram(&self, port: u16) -> i32 {
        if self.block_quic && port == QUIC_PORT {
            REJECT
        } else {
            ALLOW
        }
    }
}

pub fn current_process() -> u32 {
    (bpf_get_current_pid_tgid() >> 32) as u32
}

fn target(settings: &Settings) -> Option<u64> {
    if current_process() == settings.excluded_process {
        return None;
    }

    tracked().or(settings.has(Settings::CAPTURE_ALL).then_some(0))
}

fn tracked() -> Option<u64> {
    for level in 1..MAX_DEPTH {
        let id = unsafe { bpf_get_current_ancestor_cgroup_id(level) };

        if id == 0 {
            return None;
        }

        if unsafe { TARGETS.get(id) }.is_some() {
            return Some(id);
        }
    }

    None
}
