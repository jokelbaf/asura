use core::ptr::{read_volatile, write_volatile};

pub fn load(words: &[u32; 4]) -> [u32; 4] {
    unsafe {
        [
            read_volatile(&words[0]),
            read_volatile(&words[1]),
            read_volatile(&words[2]),
            read_volatile(&words[3]),
        ]
    }
}

pub fn store(words: &mut [u32; 4], value: [u32; 4]) {
    unsafe {
        write_volatile(&mut words[0], value[0]);
        write_volatile(&mut words[1], value[1]);
        write_volatile(&mut words[2], value[2]);
        write_volatile(&mut words[3], value[3]);
    }
}
