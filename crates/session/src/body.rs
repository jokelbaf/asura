use std::borrow::Cow;
use std::fs::File;
use std::io::{self, Write};
use std::os::unix::fs::FileExt;
use std::path::Path;

use crate::{Headers, decode};

const MEMORY_LIMIT: usize = 1 << 20;
const CHUNK: usize = 64 << 10;

#[derive(Debug, Default)]
pub struct Body {
    memory: Vec<u8>,
    file: Option<File>,
    length: u64,
    complete: bool,
    decoded: bool,
}

impl Body {
    pub fn append(&mut self, chunk: &[u8], spool: &Path) -> io::Result<()> {
        if let Some(file) = &mut self.file {
            file.write_all(chunk)?;
        } else if self.memory.len() + chunk.len() > MEMORY_LIMIT {
            let mut file = tempfile::tempfile_in(spool)?;
            file.write_all(&self.memory)?;
            file.write_all(chunk)?;
            self.memory = Vec::new();
            self.file = Some(file);
        } else {
            self.memory.extend_from_slice(chunk);
        }

        self.length += chunk.len() as u64;
        Ok(())
    }

    pub fn finish(&mut self) {
        self.complete = true;
    }

    pub fn mark_decoded(&mut self) {
        self.decoded = true;
    }

    pub fn len(&self) -> u64 {
        self.length
    }

    pub fn is_empty(&self) -> bool {
        self.length == 0
    }

    pub fn is_complete(&self) -> bool {
        self.complete
    }

    pub fn is_decoded(&self) -> bool {
        self.decoded
    }

    pub fn bytes(&self) -> io::Result<Cow<'_, [u8]>> {
        self.head(self.length as usize)
    }

    pub fn head(&self, limit: usize) -> io::Result<Cow<'_, [u8]>> {
        let length = limit.min(self.length as usize);

        let Some(file) = &self.file else {
            return Ok(Cow::Borrowed(&self.memory[..length]));
        };

        let mut buffer = vec![0; length];
        file.read_exact_at(&mut buffer, 0)?;
        Ok(Cow::Owned(buffer))
    }

    pub fn content(&self, headers: &Headers) -> io::Result<Cow<'_, [u8]>> {
        let raw = self.bytes()?;

        match headers.text("content-encoding") {
            Some(encoding) if !self.decoded => decode(&raw, encoding).map(Cow::Owned),
            _ => Ok(raw),
        }
    }

    pub fn write_to(&self, writer: &mut impl Write) -> io::Result<()> {
        let Some(file) = &self.file else {
            return writer.write_all(&self.memory);
        };

        let mut buffer = vec![0; CHUNK];
        let mut offset = 0;

        while offset < self.length {
            let size = CHUNK.min((self.length - offset) as usize);
            file.read_exact_at(&mut buffer[..size], offset)?;
            writer.write_all(&buffer[..size])?;
            offset += size as u64;
        }

        Ok(())
    }
}
