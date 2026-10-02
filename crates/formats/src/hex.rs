use std::fmt::Write;

const WIDTH: usize = 16;

pub fn hexdump(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 4 + 16);

    for (line, chunk) in bytes.chunks(WIDTH).enumerate() {
        let _ = write!(output, "{:08x}  ", line * WIDTH);

        for column in 0..WIDTH {
            match chunk.get(column) {
                Some(byte) => {
                    let _ = write!(output, "{byte:02x} ");
                }
                None => output.push_str("   "),
            }

            if column == WIDTH / 2 - 1 {
                output.push(' ');
            }
        }

        output.push(' ');
        output.extend(chunk.iter().map(|&byte| match byte {
            0x20..=0x7e => byte as char,
            _ => '.',
        }));
        output.push('\n');
    }

    output
}
