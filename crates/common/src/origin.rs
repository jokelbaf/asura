use crate::Endpoint;

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Origin {
    pub target: u64,
    pub destination: Endpoint,
    pub process: u32,
    pub command: [u8; 16],
}
