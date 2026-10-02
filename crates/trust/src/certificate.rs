use sha1::{Digest, Sha1};

const SHA1_HASH_PROPERTY: u32 = 3;
const CERTIFICATE_PROPERTY: u32 = 0x20;

pub struct Certificate<'a> {
    der: &'a [u8],
    hash: [u8; 20],
}

impl<'a> Certificate<'a> {
    pub fn new(der: &'a [u8]) -> Self {
        Self {
            der,
            hash: Sha1::digest(der).into(),
        }
    }

    pub fn key(&self) -> String {
        format!(
            "Software\\\\Microsoft\\\\SystemCertificates\\\\Root\\\\Certificates\\\\{}",
            hex::encode_upper(self.hash)
        )
    }

    pub fn blob(&self) -> Vec<u8> {
        let mut blob = Vec::with_capacity(self.der.len() + 56);
        property(&mut blob, SHA1_HASH_PROPERTY, &self.hash);
        property(&mut blob, CERTIFICATE_PROPERTY, self.der);
        blob
    }
}

fn property(blob: &mut Vec<u8>, id: u32, data: &[u8]) {
    blob.extend_from_slice(&id.to_le_bytes());
    blob.extend_from_slice(&1u32.to_le_bytes());
    blob.extend_from_slice(&(data.len() as u32).to_le_bytes());
    blob.extend_from_slice(data);
}
