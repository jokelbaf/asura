use core::mem::MaybeUninit;
use core::ptr;

use aya_ebpf::bindings::bpf_sock_addr;
use aya_ebpf::helpers::generated::{bpf_get_current_comm, bpf_get_socket_cookie};
use aya_ebpf::programs::SockAddrContext;
use common::{Endpoint, Origin};

use crate::maps::PENDING;
use crate::policy::{ALLOW, Decision, current_process};
use crate::words;

const SOCK_STREAM: u32 = 1;
const IPPROTO_TCP: u32 = 6;

const LOOPBACK4: u32 = u32::from_ne_bytes([127, 0, 0, 1]);
const LOOPBACK6: [u32; 4] = [0, 0, 0, u32::from_ne_bytes([0, 0, 0, 1])];

pub fn connect4(ctx: &mut SockAddrContext) -> i32 {
    let request = request(ctx);
    let destination = Endpoint::mapped(request.user_ip4, decode(request.user_port));

    let Some(decision) = Decision::evaluate(&destination) else {
        return ALLOW;
    };

    if !is_tcp(request) {
        return decision.datagram(destination.port);
    }

    remember(request, decision.target, destination);
    request.user_ip4 = LOOPBACK4;
    request.user_port = encode(decision.proxy_port);

    ALLOW
}

pub fn connect6(ctx: &mut SockAddrContext) -> i32 {
    let request = request(ctx);
    let destination = Endpoint::new(words::load(&request.user_ip6), decode(request.user_port));

    let Some(decision) = Decision::evaluate(&destination) else {
        return ALLOW;
    };

    if !is_tcp(request) {
        return decision.datagram(destination.port);
    }

    let loopback = if destination.is_mapped() {
        Endpoint::mapped(LOOPBACK4, 0).address
    } else {
        LOOPBACK6
    };

    remember(request, decision.target, destination);
    words::store(&mut request.user_ip6, loopback);
    request.user_port = encode(decision.proxy_port);

    ALLOW
}

pub fn sendmsg4(ctx: &mut SockAddrContext) -> i32 {
    let request = request(ctx);
    let destination = Endpoint::mapped(request.user_ip4, decode(request.user_port));

    Decision::evaluate(&destination).map_or(ALLOW, |decision| decision.datagram(destination.port))
}

pub fn sendmsg6(ctx: &mut SockAddrContext) -> i32 {
    let request = request(ctx);
    let destination = Endpoint::new(words::load(&request.user_ip6), decode(request.user_port));

    Decision::evaluate(&destination).map_or(ALLOW, |decision| decision.datagram(destination.port))
}

pub fn getpeername4(ctx: &mut SockAddrContext) -> i32 {
    let request = request(ctx);

    if let Some(origin) = recall(request) {
        request.user_ip4 = origin.destination.address[3];
        request.user_port = encode(origin.destination.port);
    }

    ALLOW
}

pub fn getpeername6(ctx: &mut SockAddrContext) -> i32 {
    let request = request(ctx);

    if let Some(origin) = recall(request) {
        words::store(&mut request.user_ip6, origin.destination.address);
        request.user_port = encode(origin.destination.port);
    }

    ALLOW
}

fn request(ctx: &mut SockAddrContext) -> &mut bpf_sock_addr {
    unsafe { &mut *ctx.sock_addr }
}

fn is_tcp(request: &bpf_sock_addr) -> bool {
    request.type_ == SOCK_STREAM && request.protocol == IPPROTO_TCP
}

fn cookie(request: &mut bpf_sock_addr) -> u64 {
    unsafe { bpf_get_socket_cookie(ptr::from_mut(request).cast()) }
}

fn remember(request: &mut bpf_sock_addr, target: u64, destination: Endpoint) {
    let origin = Origin {
        target,
        destination,
        process: current_process(),
        command: command(),
    };

    let _ = PENDING.insert(cookie(request), origin, 0);
}

fn command() -> [u8; 16] {
    let mut command = MaybeUninit::<[u8; 16]>::uninit();

    unsafe {
        bpf_get_current_comm(command.as_mut_ptr().cast(), 16);
        command.assume_init()
    }
}

fn recall(request: &mut bpf_sock_addr) -> Option<Origin> {
    unsafe { PENDING.get(cookie(request)) }.copied()
}

fn decode(port: u32) -> u16 {
    u16::from_be(port as u16)
}

fn encode(port: u16) -> u32 {
    port.to_be() as u32
}
