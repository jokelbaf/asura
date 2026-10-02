use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, OnceLock};

use capture::{Error, Interceptor, Options};
use parking_lot::Mutex;
use proxy::{Resolve, Route};

use super::Network;

#[derive(Default)]
pub struct Capture {
    interceptor: OnceLock<Interceptor>,
    elevation: Mutex<()>,
    options: Mutex<Options>,
    exclusions: Mutex<Vec<Network>>,
}

impl Capture {
    pub fn interceptor(&self) -> Option<&Interceptor> {
        self.interceptor.get()
    }

    pub fn acquire(&self) -> Result<&Interceptor, Error> {
        let _elevation = self.elevation.lock();

        if let Some(interceptor) = self.interceptor.get() {
            return Ok(interceptor);
        }

        let interceptor = Interceptor::start()?;
        let exclusions = self.exclusions.lock();

        for network in exclusions.iter() {
            interceptor.exclude(network.address, network.prefix)?;
        }

        interceptor.configure(*self.options.lock())?;
        Ok(self.interceptor.get_or_init(|| interceptor))
    }

    pub fn options(&self) -> Options {
        *self.options.lock()
    }

    pub fn is_active(&self) -> bool {
        self.interceptor.get().is_some() && self.options.lock().enabled
    }

    pub fn configure(&self, change: impl FnOnce(&mut Options)) -> Result<(), Error> {
        let options = {
            let mut options = self.options.lock();
            change(&mut options);
            *options
        };

        match self.interceptor.get() {
            Some(interceptor) => interceptor.configure(options),
            None => Ok(()),
        }
    }

    pub fn exclude(&self, networks: Vec<Network>) -> Result<(), Error> {
        let mut current = self.exclusions.lock();

        if let Some(interceptor) = self.interceptor.get() {
            for network in current.iter().filter(|network| !networks.contains(network)) {
                interceptor.include(network.address, network.prefix)?;
            }

            for network in networks.iter().filter(|network| !current.contains(network)) {
                interceptor.exclude(network.address, network.prefix)?;
            }
        }

        *current = networks;
        Ok(())
    }

    pub fn refresh(&self) {
        if let Some(interceptor) = self.interceptor.get() {
            let _ = interceptor.refresh();
        }
    }
}

pub struct Resolver(pub Arc<Capture>);

impl Resolve for Resolver {
    fn resolve(&self, peer: SocketAddr) -> Option<Route> {
        let origin = self.0.interceptor()?.resolve(peer)?;

        Some(Route {
            destination: origin.destination,
            process: origin.process,
            command: origin.command,
            target: origin.target,
        })
    }

    fn unreachable(&self, destination: SocketAddr) {
        if let Some(interceptor) = self.0.interceptor() {
            let _ = interceptor.bypass(destination.ip());
        }
    }
}

pub fn network(text: &str) -> Option<Network> {
    let text = text.trim();

    let (address, prefix) = match text.split_once('/') {
        Some((address, prefix)) => (
            address.parse::<IpAddr>().ok()?,
            Some(prefix.parse::<u8>().ok()?),
        ),
        None => (text.parse::<IpAddr>().ok()?, None),
    };

    let width = if address.is_ipv4() { 32 } else { 128 };
    let prefix = prefix.unwrap_or(width);

    (prefix <= width).then_some(Network { address, prefix })
}
