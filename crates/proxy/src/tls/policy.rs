use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

use parking_lot::Mutex;
use session::Client;

use crate::process;

#[derive(Clone, Debug)]
pub struct Rejection {
    pub client: Client,
    pub reason: String,
    pub stale: bool,
}

#[derive(Default)]
pub struct Policy {
    bypassed: Mutex<HashMap<String, Rejection>>,
    rules: Mutex<Vec<String>>,
    vouched: Mutex<HashSet<(u32, String)>>,
    trusted_since: Mutex<Option<SystemTime>>,
    revision: AtomicU64,
}

impl Policy {
    pub fn bypass(&self, key: &str, client: &Client) -> Option<String> {
        if let Some(rejection) = self.bypassed.lock().get(key)
            && (!rejection.stale || rejection.client.process == client.process)
        {
            return Some(rejection.reason.clone());
        }

        let host = host(key);

        self.rules
            .lock()
            .iter()
            .find(|rule| matches(rule, &host))
            .map(|rule| format!("excluded from decryption by the rule {rule}"))
    }

    pub fn reject(&self, key: String, client: &Client, reason: &str) {
        let rejection = Rejection {
            client: client.clone(),
            reason: reason.to_owned(),
            stale: self.is_stale(client),
        };

        self.bypassed.lock().insert(key, rejection);
        self.touch();
    }

    pub fn vouch(&self, client: &Client) {
        self.vouched
            .lock()
            .insert((client.process, client.name.clone()));
    }

    pub fn has_vouched(&self, client: &Client) -> bool {
        self.vouched
            .lock()
            .contains(&(client.process, client.name.clone()))
    }

    pub fn bypassed(&self) -> Vec<(String, Rejection)> {
        let mut entries: Vec<_> = self
            .bypassed
            .lock()
            .iter()
            .map(|(key, rejection)| (key.clone(), rejection.clone()))
            .collect();
        entries.sort_by(|left, right| left.0.cmp(&right.0));
        entries
    }

    pub fn revision(&self) -> u64 {
        self.revision.load(Ordering::Relaxed)
    }

    pub fn forget(&self, key: &str) {
        self.bypassed.lock().remove(key);
        self.touch();
    }

    pub fn retain(&self, keep: impl Fn(&Rejection) -> bool) {
        self.bypassed.lock().retain(|_, rejection| keep(rejection));
        self.touch();
    }

    pub fn clear(&self) {
        self.bypassed.lock().clear();
        self.touch();
    }

    pub fn set_trusted_since(&self, since: Option<SystemTime>) {
        *self.trusted_since.lock() = since;
    }

    pub fn set_rules(&self, rules: &[String]) {
        *self.rules.lock() = rules
            .iter()
            .map(|rule| rule.trim().to_lowercase())
            .collect();
    }

    fn is_stale(&self, client: &Client) -> bool {
        let Some(since) = *self.trusted_since.lock() else {
            return false;
        };

        !client.is_windows()
            && process::started_at(client.process).is_some_and(|started| started < since)
    }

    fn touch(&self) {
        self.revision.fetch_add(1, Ordering::Relaxed);
    }
}

fn host(key: &str) -> String {
    key.parse::<SocketAddr>()
        .map_or_else(|_| key.to_lowercase(), |address| address.ip().to_string())
}

fn matches(rule: &str, host: &str) -> bool {
    match rule.strip_prefix("*.") {
        Some(domain) => host
            .strip_suffix(domain)
            .is_some_and(|rest| rest.ends_with('.')),
        None => host == rule,
    }
}
