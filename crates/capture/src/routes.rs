use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket};

const PROBE4: IpAddr = IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1));
const PROBE6: IpAddr = IpAddr::V6(Ipv6Addr::new(0x2606, 0x4700, 0x4700, 0, 0, 0, 0, 0x1111));

pub fn ipv4() -> bool {
    routable(Ipv4Addr::UNSPECIFIED.into(), PROBE4)
}

pub fn ipv6() -> bool {
    routable(Ipv6Addr::UNSPECIFIED.into(), PROBE6)
}

fn routable(local: IpAddr, probe: IpAddr) -> bool {
    UdpSocket::bind(SocketAddr::new(local, 0))
        .and_then(|socket| socket.connect(SocketAddr::new(probe, 53)))
        .is_ok()
}
