#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Settings {
    pub flags: u32,
    pub excluded_process: u32,
    pub proxy_port: u16,
    pub reserved: [u8; 6],
}

impl Settings {
    pub const ENABLED: u32 = 1;
    pub const CAPTURE_ALL: u32 = 1 << 1;
    pub const BLOCK_QUIC: u32 = 1 << 2;
    pub const ROUTE4: u32 = 1 << 3;
    pub const ROUTE6: u32 = 1 << 4;

    pub const fn has(&self, flag: u32) -> bool {
        self.flags & flag != 0
    }
}
