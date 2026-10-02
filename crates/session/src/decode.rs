use std::io::{self, Read};

use flate2::read::{DeflateDecoder, MultiGzDecoder, ZlibDecoder};

const LIMIT: u64 = 256 << 20;

pub fn decode(body: &[u8], encodings: &str) -> io::Result<Vec<u8>> {
    let mut data = body.to_vec();

    for encoding in encodings
        .rsplit(',')
        .map(str::trim)
        .filter(|encoding| !encoding.is_empty())
    {
        data = match encoding.to_ascii_lowercase().as_str() {
            "gzip" | "x-gzip" => read(MultiGzDecoder::new(data.as_slice()))?,
            "deflate" => read(ZlibDecoder::new(data.as_slice()))
                .or_else(|_| read(DeflateDecoder::new(data.as_slice())))?,
            "br" => read(brotli::Decompressor::new(data.as_slice(), 4096))?,
            "zstd" => read(zstd::Decoder::new(data.as_slice())?)?,
            "identity" => data,
            other => {
                return Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    format!("unsupported content encoding {other}"),
                ));
            }
        };
    }

    Ok(data)
}

fn read(reader: impl Read) -> io::Result<Vec<u8>> {
    let mut output = Vec::new();
    reader.take(LIMIT).read_to_end(&mut output)?;
    Ok(output)
}
