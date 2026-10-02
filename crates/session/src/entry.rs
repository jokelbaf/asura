use std::path::Path;
use std::sync::Arc;

use parking_lot::{RwLock, RwLockReadGuard};

use crate::store::Notifier;
use crate::{Connection, Event, Record};

pub struct Entry {
    id: u64,
    listed: bool,
    connection: Arc<Connection>,
    record: RwLock<Record>,
    notifier: Arc<Notifier>,
}

impl Entry {
    pub(crate) fn new(
        id: u64,
        listed: bool,
        connection: Arc<Connection>,
        record: Record,
        notifier: Arc<Notifier>,
    ) -> Self {
        Self {
            id,
            listed,
            connection,
            record: RwLock::new(record),
            notifier,
        }
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn is_listed(&self) -> bool {
        self.listed
    }

    pub fn connection(&self) -> &Arc<Connection> {
        &self.connection
    }

    pub fn spool(&self) -> &Path {
        self.notifier.spool()
    }

    pub fn read(&self) -> RwLockReadGuard<'_, Record> {
        self.record.read()
    }

    pub fn update<T>(&self, change: impl FnOnce(&mut Record, &Path) -> T) -> T {
        let result = change(&mut self.record.write(), self.notifier.spool());

        if self.listed {
            self.notifier.publish(Event::Changed(self.id));
        }

        result
    }
}
