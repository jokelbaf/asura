use std::sync::Arc;

use session::{Direction, Entry};

use super::{Event, Observer, Scope};

const PREVIEW: u64 = 1 << 20;

pub struct Tunnel {
    entry: Arc<Entry>,
}

impl Tunnel {
    pub fn new(scope: &Scope) -> Self {
        Self::attach(scope.store.open(scope.connection.clone()))
    }

    pub fn attach(entry: Arc<Entry>) -> Self {
        entry.update(|record, _| record.tunnel = Some(Default::default()));
        Self { entry }
    }
}

impl Observer for Tunnel {
    fn observe(&mut self, event: Event) {
        match event {
            Event::Data {
                direction, chunk, ..
            } => self.entry.update(|record, spool| {
                let tunnel = record.tunnel.get_or_insert_default();

                let (body, total) = match direction {
                    Direction::Outbound => (&mut tunnel.sent, &mut tunnel.sent_bytes),
                    Direction::Inbound => (&mut tunnel.received, &mut tunnel.received_bytes),
                };

                *total += chunk.len() as u64;

                if body.len() < PREVIEW {
                    let _ = body.append(&chunk, spool);
                }
            }),
            Event::End { .. } => {}
            Event::Failed(reason) => self.entry.update(|record, _| record.fail(reason)),
        }
    }

    fn close(&mut self) {
        self.entry.update(|record, _| record.complete());
    }
}
