use std::io;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use async_channel::{Receiver, Sender};
use parking_lot::{Mutex, RwLock};
use tempfile::TempDir;
use time::OffsetDateTime;

use crate::{Connection, Entry, Record};

#[derive(Clone)]
pub enum Event {
    Added(Arc<Entry>),
    Changed(u64),
    Removed(Arc<[u64]>),
    Cleared,
}

pub(crate) struct Notifier {
    subscribers: Mutex<Vec<Sender<Event>>>,
    spool: TempDir,
}

impl Notifier {
    pub(crate) fn publish(&self, event: Event) {
        self.subscribers
            .lock()
            .retain(|subscriber| subscriber.try_send(event.clone()).is_ok());
    }

    pub(crate) fn spool(&self) -> &Path {
        self.spool.path()
    }
}

pub struct Store {
    entries: RwLock<Vec<Arc<Entry>>>,
    recording: AtomicBool,
    next_entry: AtomicU64,
    next_connection: AtomicU64,
    notifier: Arc<Notifier>,
}

impl Store {
    pub fn new() -> io::Result<Self> {
        Ok(Self {
            entries: RwLock::default(),
            recording: AtomicBool::new(true),
            next_entry: AtomicU64::new(1),
            next_connection: AtomicU64::new(1),
            notifier: Arc::new(Notifier {
                subscribers: Mutex::default(),
                spool: tempfile::Builder::new().prefix("asura-").tempdir()?,
            }),
        })
    }

    pub fn subscribe(&self) -> Receiver<Event> {
        let (sender, receiver) = async_channel::unbounded();
        self.notifier.subscribers.lock().push(sender);
        receiver
    }

    pub fn connection_id(&self) -> u64 {
        self.next_connection.fetch_add(1, Ordering::Relaxed)
    }

    pub fn spool(&self) -> &Path {
        self.notifier.spool()
    }

    pub fn set_recording(&self, recording: bool) {
        self.recording.store(recording, Ordering::Relaxed);
    }

    pub fn is_recording(&self) -> bool {
        self.recording.load(Ordering::Relaxed)
    }

    pub fn open(&self, connection: Arc<Connection>) -> Arc<Entry> {
        let record = Record::new(OffsetDateTime::now_utc());

        if self.is_recording() {
            self.restore(connection, record)
        } else {
            Arc::new(Entry::new(
                0,
                false,
                connection,
                record,
                self.notifier.clone(),
            ))
        }
    }

    pub fn insert(&self, connection: Arc<Connection>) -> Arc<Entry> {
        self.restore(connection, Record::new(OffsetDateTime::now_utc()))
    }

    pub fn restore(&self, connection: Arc<Connection>, record: Record) -> Arc<Entry> {
        let mut entries = self.entries.write();
        let id = self.next_entry.fetch_add(1, Ordering::Relaxed);
        let entry = Arc::new(Entry::new(
            id,
            true,
            connection,
            record,
            self.notifier.clone(),
        ));

        entries.push(entry.clone());
        self.notifier.publish(Event::Added(entry.clone()));
        entry
    }

    pub fn entries(&self) -> Vec<Arc<Entry>> {
        self.entries.read().clone()
    }

    pub fn get(&self, id: u64) -> Option<Arc<Entry>> {
        let entries = self.entries.read();
        let index = entries.binary_search_by_key(&id, |entry| entry.id()).ok()?;
        Some(entries[index].clone())
    }

    pub fn remove(&self, ids: &[u64]) {
        let mut removed = ids.to_vec();
        removed.sort_unstable();

        self.entries
            .write()
            .retain(|entry| removed.binary_search(&entry.id()).is_err());

        self.notifier.publish(Event::Removed(removed.into()));
    }

    pub fn clear(&self) {
        self.entries.write().clear();
        self.notifier.publish(Event::Cleared);
    }
}
