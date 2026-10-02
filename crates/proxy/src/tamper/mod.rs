mod draft;
mod pattern;
mod rules;

use std::collections::HashMap;

use parking_lot::{Mutex, RwLock};
use session::{Entry, Scheme, Stage};
use tokio::sync::oneshot;

pub use draft::{RequestDraft, ResponseDraft};
pub use pattern::Pattern;
pub use rules::{Action, Breakpoint, Responder};

#[derive(Debug)]
pub enum Verdict {
    Abort,
    Request {
        draft: Option<RequestDraft>,
        break_response: bool,
    },
    Response {
        draft: Option<ResponseDraft>,
    },
}

#[derive(Default)]
pub struct Plan {
    pub responder: Option<(String, Action)>,
    pub request: bool,
    pub response: bool,
}

struct Rule<T> {
    pattern: Pattern,
    rule: T,
}

#[derive(Default)]
struct Rules {
    responders: Vec<Rule<Responder>>,
    breakpoints: Vec<Rule<Breakpoint>>,
}

#[derive(Default)]
pub struct Tamper {
    rules: RwLock<Rules>,
    pauses: Mutex<HashMap<u64, oneshot::Sender<Verdict>>>,
}

impl Tamper {
    pub fn configure(&self, responders: &[Responder], breakpoints: &[Breakpoint]) {
        let responders = compile(responders, |rule| (rule.enabled, &rule.pattern));
        let breakpoints = compile(breakpoints, |rule| {
            (
                rule.enabled && (rule.request || rule.response),
                &rule.pattern,
            )
        });

        *self.rules.write() = Rules {
            responders,
            breakpoints,
        };
    }

    pub fn applies(&self, scheme: Scheme, host: Option<&str>) -> bool {
        let rules = self.rules.read();

        rules
            .responders
            .iter()
            .map(|rule| &rule.pattern)
            .chain(rules.breakpoints.iter().map(|rule| &rule.pattern))
            .any(|pattern| pattern.covers(scheme, host))
    }

    pub fn plan(&self, method: &str, url: &str) -> Plan {
        let rules = self.rules.read();
        let applies = |pattern: &Pattern, wanted: &str| {
            (wanted.is_empty() || wanted.eq_ignore_ascii_case(method)) && pattern.matches(url)
        };

        let responder = rules
            .responders
            .iter()
            .find(|entry| applies(&entry.pattern, &entry.rule.method))
            .map(|entry| (entry.rule.pattern.clone(), entry.rule.action.clone()));

        let (request, response) = rules
            .breakpoints
            .iter()
            .filter(|entry| applies(&entry.pattern, &entry.rule.method))
            .fold((false, false), |(request, response), entry| {
                (
                    request || entry.rule.request,
                    response || entry.rule.response,
                )
            });

        Plan {
            responder,
            request,
            response,
        }
    }

    pub async fn pause(&self, entry: &Entry, stage: Stage) -> Option<Verdict> {
        let (sender, receiver) = oneshot::channel();

        self.pauses.lock().insert(entry.id(), sender);
        entry.update(|record, _| record.paused = Some(stage));

        let mut waiting = Waiting {
            tamper: self,
            entry,
            settled: false,
        };

        let verdict = receiver.await.ok();
        waiting.settled = true;
        verdict
    }

    pub fn resume(&self, id: u64, verdict: Verdict) {
        if let Some(sender) = self.pauses.lock().remove(&id) {
            let _ = sender.send(verdict);
        }
    }

    pub fn paused(&self) -> Vec<u64> {
        let mut ids: Vec<u64> = self.pauses.lock().keys().copied().collect();
        ids.sort_unstable();
        ids
    }

    pub fn release(&self, ids: &[u64]) {
        let mut pauses = self.pauses.lock();

        for id in ids {
            pauses.remove(id);
        }
    }

    pub fn release_all(&self) {
        self.pauses.lock().clear();
    }
}

struct Waiting<'a> {
    tamper: &'a Tamper,
    entry: &'a Entry,
    settled: bool,
}

impl Drop for Waiting<'_> {
    fn drop(&mut self) {
        if self.settled {
            return;
        }

        self.tamper.pauses.lock().remove(&self.entry.id());
        self.entry.update(|record, _| record.paused = None);
    }
}

fn compile<T: Clone>(rules: &[T], key: impl Fn(&T) -> (bool, &String)) -> Vec<Rule<T>> {
    rules
        .iter()
        .filter_map(|rule| {
            let (enabled, pattern) = key(rule);
            let pattern = Pattern::parse(pattern).ok().filter(|_| enabled)?;

            Some(Rule {
                pattern,
                rule: rule.clone(),
            })
        })
        .collect()
}
