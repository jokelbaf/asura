const MAPPED: u32 = u32::from_ne_bytes([0, 0, 0xff, 0xff]);

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Endpoint {
    pub address: [u32; 4],
    pub port: u16,
    pub reserved: u16,
}

impl Endpoint {
    pub const fn new(address: [u32; 4], port: u16) -> Self {
        Self {
            address,
            port,
            reserved: 0,
        }
    }

    pub const fn mapped(address: u32, port: u16) -> Self {
        Self::new([0, 0, MAPPED, address], port)
    }

    pub const fn is_mapped(&self) -> bool {
        self.address[0] == 0 && self.address[1] == 0 && self.address[2] == MAPPED
    }
}
