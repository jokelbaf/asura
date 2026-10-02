use std::io;
use std::os::fd::{AsRawFd, BorrowedFd, FromRawFd, OwnedFd};

use aya_obj::generated::{bpf_attach_type, bpf_cmd};

#[repr(C)]
struct LinkCreate {
    program: u32,
    target: u32,
    attach_type: u32,
    flags: u32,
}

pub fn create(
    program: BorrowedFd<'_>,
    target: BorrowedFd<'_>,
    attach_type: bpf_attach_type,
) -> io::Result<OwnedFd> {
    let attributes = LinkCreate {
        program: program.as_raw_fd() as u32,
        target: target.as_raw_fd() as u32,
        attach_type: attach_type as u32,
        flags: 0,
    };

    let result = unsafe {
        libc::syscall(
            libc::SYS_bpf,
            bpf_cmd::BPF_LINK_CREATE as libc::c_long,
            &raw const attributes,
            size_of::<LinkCreate>(),
        )
    };

    if result < 0 {
        return Err(io::Error::last_os_error());
    }

    Ok(unsafe { OwnedFd::from_raw_fd(result as i32) })
}
