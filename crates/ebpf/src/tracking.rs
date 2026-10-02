use aya_ebpf::bindings::{
    BPF_SOCK_OPS_STATE_CB, BPF_SOCK_OPS_STATE_CB_FLAG, BPF_SOCK_OPS_TCP_CONNECT_CB, BPF_TCP_CLOSE,
};
use aya_ebpf::helpers::generated::bpf_get_socket_cookie;
use aya_ebpf::programs::SockOpsContext;
use common::Endpoint;

use crate::maps::{ORIGINS, PENDING};
use crate::words;

const AF_INET: u32 = 2;

pub fn handle(ctx: &SockOpsContext) -> u32 {
    match ctx.op() {
        BPF_SOCK_OPS_TCP_CONNECT_CB => connecting(ctx),
        BPF_SOCK_OPS_STATE_CB if ctx.arg(1) == BPF_TCP_CLOSE => closed(ctx),
        _ => {}
    }

    1
}

fn connecting(ctx: &SockOpsContext) {
    let Some(origin) = (unsafe { PENDING.get(cookie(ctx)) }) else {
        return;
    };

    let port = ctx.local_port() as u16;
    let source = if ctx.family() == AF_INET {
        Endpoint::mapped(ctx.local_ip4(), port)
    } else {
        Endpoint::new(words::load(unsafe { &(*ctx.ops).local_ip6 }), port)
    };

    let _ = ORIGINS.insert(source, origin, 0);
    let _ = ctx.set_cb_flags(BPF_SOCK_OPS_STATE_CB_FLAG as i32);
}

fn closed(ctx: &SockOpsContext) {
    let _ = PENDING.remove(cookie(ctx));
}

fn cookie(ctx: &SockOpsContext) -> u64 {
    unsafe { bpf_get_socket_cookie(ctx.ops.cast()) }
}
