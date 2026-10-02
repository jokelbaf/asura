use aya_ebpf::bindings::BPF_F_NO_PREALLOC;
use aya_ebpf::macros::map;
use aya_ebpf::maps::{Array, HashMap, LpmTrie, LruHashMap};
use common::{Endpoint, Origin, Settings};

#[map]
pub static SETTINGS: Array<Settings> = Array::with_max_entries(1, 0);

#[map]
pub static TARGETS: HashMap<u64, u8> = HashMap::with_max_entries(4096, 0);

#[map]
pub static EXCLUSIONS: LpmTrie<[u32; 4], u8> = LpmTrie::with_max_entries(1024, BPF_F_NO_PREALLOC);

#[map]
pub static PENDING: LruHashMap<u64, Origin> = LruHashMap::with_max_entries(65536, 0);

#[map]
pub static ORIGINS: LruHashMap<Endpoint, Origin> = LruHashMap::with_max_entries(65536, 0);
