use std::net::{IpAddr, Ipv6Addr, SocketAddr};

use common::Endpoint;

pub fn words(address: IpAddr) -> [u32; 4] {
    let octets = match address {
        IpAddr::V4(address) => address.to_ipv6_mapped().octets(),
        IpAddr::V6(address) => address.octets(),
    };

    let mut words = [0u32; 4];
    for (word, chunk) in words.iter_mut().zip(octets.as_chunks::<4>().0) {
        *word = u32::from_ne_bytes(*chunk);
    }
    words
}

pub fn to_endpoint(address: SocketAddr) -> Endpoint {
    Endpoint::new(words(address.ip()), address.port())
}

pub fn to_socket(endpoint: &Endpoint) -> SocketAddr {
    let mut octets = [0u8; 16];
    for (chunk, word) in octets
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(endpoint.address)
    {
        *chunk = word.to_ne_bytes();
    }

    SocketAddr::new(Ipv6Addr::from(octets).to_canonical(), endpoint.port)
}
