use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::os::fd::OwnedFd;

use aya::maps::lpm_trie::Key;
use aya::maps::{Array, HashMap, LpmTrie, Map, MapData, MapError};
use common::{Endpoint, Settings};
use parking_lot::Mutex;

use crate::endpoint::{to_endpoint, words};
use crate::{Bundle, Error, Origin, elevation, routes};

const ALWAYS_EXCLUDED: [(IpAddr, u8); 4] = [
    (IpAddr::V4(Ipv4Addr::new(127, 0, 0, 0)), 8),
    (IpAddr::V4(Ipv4Addr::UNSPECIFIED), 32),
    (IpAddr::V6(Ipv6Addr::LOCALHOST), 128),
    (IpAddr::V6(Ipv6Addr::UNSPECIFIED), 128),
];

#[derive(Clone, Copy, Debug, Default)]
pub struct Options {
    pub enabled: bool,
    pub capture_all: bool,
    pub block_quic: bool,
    pub proxy_port: u16,
}

pub struct Interceptor {
    options: Mutex<Options>,
    settings: Mutex<Array<MapData, Settings>>,
    targets: Mutex<HashMap<MapData, u64, u8>>,
    exclusions: Mutex<LpmTrie<MapData, [u32; 4], u8>>,
    origins: Mutex<HashMap<MapData, Endpoint, common::Origin>>,
    _links: Vec<OwnedFd>,
}

impl Interceptor {
    pub fn start() -> Result<Self, Error> {
        let interceptor = Self::from_bundle(elevation::acquire()?)?;

        for (network, prefix) in ALWAYS_EXCLUDED {
            interceptor.exclude(network, prefix)?;
        }

        Ok(interceptor)
    }

    fn from_bundle(bundle: Bundle) -> Result<Self, Error> {
        let [settings, targets, exclusions, origins] =
            <[OwnedFd; 4]>::try_from(bundle.maps).map_err(|_| Error::Handoff)?;

        Ok(Self {
            options: Mutex::default(),
            settings: Mutex::new(typed(settings)?),
            targets: Mutex::new(typed(targets)?),
            exclusions: Mutex::new(typed(exclusions)?),
            origins: Mutex::new(typed(origins)?),
            _links: bundle.links,
        })
    }

    pub fn configure(&self, options: Options) -> Result<(), Error> {
        *self.options.lock() = options;
        self.refresh()
    }

    pub fn refresh(&self) -> Result<(), Error> {
        let options = *self.options.lock();

        let flags = [
            (options.enabled, Settings::ENABLED),
            (options.capture_all, Settings::CAPTURE_ALL),
            (options.block_quic, Settings::BLOCK_QUIC),
            (routes::ipv4(), Settings::ROUTE4),
            (routes::ipv6(), Settings::ROUTE6),
        ]
        .into_iter()
        .filter(|(enabled, _)| *enabled)
        .fold(0, |flags, (_, flag)| flags | flag);

        let settings = Settings {
            flags,
            excluded_process: std::process::id(),
            proxy_port: options.proxy_port,
            reserved: [0; 6],
        };

        self.settings.lock().set(0, settings, 0)?;
        Ok(())
    }

    pub fn track(&self, cgroup: u64) -> Result<(), Error> {
        self.targets.lock().insert(cgroup, 1, 0)?;
        Ok(())
    }

    pub fn untrack(&self, cgroup: u64) -> Result<(), Error> {
        self.targets.lock().remove(&cgroup)?;
        Ok(())
    }

    pub fn exclude(&self, network: IpAddr, prefix: u8) -> Result<(), Error> {
        self.exclusions.lock().insert(&key(network, prefix), 1, 0)?;
        Ok(())
    }

    pub fn include(&self, network: IpAddr, prefix: u8) -> Result<(), Error> {
        if ALWAYS_EXCLUDED.contains(&(network, prefix)) {
            return Ok(());
        }

        self.exclusions.lock().remove(&key(network, prefix))?;
        Ok(())
    }

    pub fn bypass(&self, address: IpAddr) -> Result<(), Error> {
        let prefix = if address.is_ipv4() { 32 } else { 128 };
        self.exclude(address, prefix)
    }

    pub fn resolve(&self, peer: SocketAddr) -> Option<Origin> {
        let key = to_endpoint(peer);
        let mut origins = self.origins.lock();
        let origin = origins.get(&key, 0).ok()?;

        let _ = origins.remove(&key);
        Some(origin.into())
    }
}

fn key(network: IpAddr, prefix: u8) -> Key<[u32; 4]> {
    let length = match network {
        IpAddr::V4(_) => 96 + u32::from(prefix),
        IpAddr::V6(_) => u32::from(prefix),
    };

    Key::new(length, words(network))
}

fn typed<T>(fd: OwnedFd) -> Result<T, Error>
where
    T: TryFrom<Map, Error = MapError>,
{
    Ok(T::try_from(Map::from_map_data(MapData::from_fd(fd)?)?)?)
}
