mod huffman;
mod table;

use std::collections::VecDeque;

use table::STATIC;

const ENTRY_OVERHEAD: usize = 32;
const DEFAULT_SIZE: usize = 4096;

#[derive(Debug)]
pub enum Error {
    Truncated,
    Index,
    Integer,
    Huffman,
}

pub type Field = (Vec<u8>, Vec<u8>);

pub struct Decoder {
    dynamic: VecDeque<Field>,
    size: usize,
    capacity: usize,
}

impl Default for Decoder {
    fn default() -> Self {
        Self {
            dynamic: VecDeque::new(),
            size: 0,
            capacity: DEFAULT_SIZE,
        }
    }
}

impl Decoder {
    pub fn decode(&mut self, block: &[u8]) -> Result<Vec<Field>, Error> {
        let mut input = Input { data: block };
        let mut fields = Vec::new();

        while let Some(&first) = input.data.first() {
            if first & 0x80 != 0 {
                let index = input.integer(7)?;
                fields.push(self.field(index)?);
            } else if first & 0x40 != 0 {
                let field = self.literal(&mut input, 6)?;
                self.insert(field.clone());
                fields.push(field);
            } else if first & 0x20 != 0 {
                self.capacity = input.integer(5)?;
                self.evict(0);
            } else {
                fields.push(self.literal(&mut input, 4)?);
            }
        }

        Ok(fields)
    }

    fn literal(&self, input: &mut Input<'_>, prefix: u8) -> Result<Field, Error> {
        let index = input.integer(prefix)?;

        let name = if index == 0 {
            input.string()?
        } else {
            self.field(index)?.0
        };

        Ok((name, input.string()?))
    }

    fn field(&self, index: usize) -> Result<Field, Error> {
        match index {
            0 => Err(Error::Index),
            1..=61 => {
                let (name, value) = STATIC[index - 1];
                Ok((name.as_bytes().to_vec(), value.as_bytes().to_vec()))
            }
            _ => self.dynamic.get(index - 62).cloned().ok_or(Error::Index),
        }
    }

    fn insert(&mut self, field: Field) {
        let size = field.0.len() + field.1.len() + ENTRY_OVERHEAD;

        self.evict(size);

        if size <= self.capacity {
            self.size += size;
            self.dynamic.push_front(field);
        }
    }

    fn evict(&mut self, incoming: usize) {
        while self.size + incoming > self.capacity {
            let Some((name, value)) = self.dynamic.pop_back() else {
                break;
            };
            self.size -= name.len() + value.len() + ENTRY_OVERHEAD;
        }
    }
}

struct Input<'a> {
    data: &'a [u8],
}

impl Input<'_> {
    fn integer(&mut self, prefix: u8) -> Result<usize, Error> {
        let (&first, rest) = self.data.split_first().ok_or(Error::Truncated)?;
        self.data = rest;

        let mask = (1u16 << prefix) as u8 - 1;
        let mut value = usize::from(first & mask);

        if value < usize::from(mask) {
            return Ok(value);
        }

        for shift in (0..).step_by(7).take(5) {
            let (&byte, rest) = self.data.split_first().ok_or(Error::Truncated)?;
            self.data = rest;
            value += usize::from(byte & 0x7f) << shift;

            if byte & 0x80 == 0 {
                return Ok(value);
            }
        }

        Err(Error::Integer)
    }

    fn string(&mut self) -> Result<Vec<u8>, Error> {
        let huffman = self.data.first().ok_or(Error::Truncated)? & 0x80 != 0;
        let length = self.integer(7)?;

        if length > self.data.len() {
            return Err(Error::Truncated);
        }

        let (raw, rest) = self.data.split_at(length);
        self.data = rest;

        if huffman {
            huffman::decode(raw)
        } else {
            Ok(raw.to_vec())
        }
    }
}
