#![no_std]
#![no_main]

mod maps;
mod policy;
mod redirect;
mod tracking;
mod words;

use aya_ebpf::macros::{cgroup_sock_addr, sock_ops};
use aya_ebpf::programs::{SockAddrContext, SockOpsContext};

#[cgroup_sock_addr(connect4)]
pub fn connect4(mut ctx: SockAddrContext) -> i32 {
    redirect::connect4(&mut ctx)
}

#[cgroup_sock_addr(connect6)]
pub fn connect6(mut ctx: SockAddrContext) -> i32 {
    redirect::connect6(&mut ctx)
}

#[cgroup_sock_addr(sendmsg4)]
pub fn sendmsg4(mut ctx: SockAddrContext) -> i32 {
    redirect::sendmsg4(&mut ctx)
}

#[cgroup_sock_addr(sendmsg6)]
pub fn sendmsg6(mut ctx: SockAddrContext) -> i32 {
    redirect::sendmsg6(&mut ctx)
}

#[cgroup_sock_addr(getpeername4)]
pub fn getpeername4(mut ctx: SockAddrContext) -> i32 {
    redirect::getpeername4(&mut ctx)
}

#[cgroup_sock_addr(getpeername6)]
pub fn getpeername6(mut ctx: SockAddrContext) -> i32 {
    redirect::getpeername6(&mut ctx)
}

#[sock_ops]
pub fn lifecycle(ctx: SockOpsContext) -> u32 {
    tracking::handle(&ctx)
}

#[cfg(target_arch = "bpf")]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}

#[unsafe(link_section = "license")]
#[unsafe(no_mangle)]
static LICENSE: [u8; 4] = *b"GPL\0";
