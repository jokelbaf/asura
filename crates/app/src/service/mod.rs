mod anchor;
mod capture;
mod control;
pub mod processes;
mod settings;
mod snapshot;

use std::future::Future;
use std::io;
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use ::capture::{Cgroup, Scope};
use anyhow::Context;
use async_channel::{Receiver, Sender};
use authority::Authority;
use parking_lot::Mutex;
use proxy::{Composer, Engine, Outgoing, Policy, Tamper, Verdict};
use session::{Entry, Store};
use tokio::runtime::Handle;

use self::capture::{Capture, Resolver};

pub use self::capture::network;
pub use self::settings::{Geometry, Settings};

const WATCH: Duration = Duration::from_secs(2);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Network {
    pub address: IpAddr,
    pub prefix: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    Launched,
    Attached,
}

#[derive(Clone, Debug)]
pub struct Target {
    pub name: String,
    pub process: u32,
    pub command: String,
    pub cgroup: Cgroup,
    pub origin: Origin,
}

#[derive(Clone, Debug)]
pub enum Notice {
    Capture,
    Targets,
    Rules,
    Trust,
}

pub struct Service {
    runtime: Handle,
    store: Arc<Store>,
    authority: Arc<Authority>,
    policy: Arc<Policy>,
    tamper: Arc<Tamper>,
    composer: Composer,
    capture: Arc<Capture>,
    port: u16,
    targets: Mutex<Vec<Target>>,
    settings: Mutex<Settings>,
    subscribers: Mutex<Vec<Sender<Notice>>>,
}

impl Service {
    pub fn start(runtime: Handle) -> anyhow::Result<Arc<Self>> {
        let settings = Settings::load();
        let authority = Arc::new(
            Authority::open(&::control::data()).context("opening the certificate authority")?,
        );
        let store = Arc::new(Store::new().context("creating the session spool")?);
        store.set_recording(false);
        let capture = Arc::new(Capture::default());

        let resolver = Arc::new(Resolver(capture.clone()));
        let engine = runtime
            .block_on(Engine::bind(resolver, authority.clone(), store.clone()))
            .context("starting the proxy")?;

        let service = Arc::new(Self {
            runtime: runtime.clone(),
            store,
            authority,
            policy: engine.policy(),
            tamper: engine.tamper(),
            composer: engine.composer(),
            capture,
            port: engine.port(),
            targets: Mutex::default(),
            settings: Mutex::new(settings.clone()),
            subscribers: Mutex::default(),
        });

        service.apply(&settings)?;
        service.policy.set_trusted_since(service.trusted_since());

        runtime.spawn(engine.run());
        runtime.spawn(control::serve(service.clone()));
        runtime.spawn(watch(service.clone()));

        Ok(service)
    }

    pub fn store(&self) -> &Arc<Store> {
        &self.store
    }

    pub fn authority(&self) -> &Arc<Authority> {
        &self.authority
    }

    pub fn policy(&self) -> &Arc<Policy> {
        &self.policy
    }

    pub fn tamper(&self) -> &Arc<Tamper> {
        &self.tamper
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn system_trust(&self) -> Option<bool> {
        trust::system().map(|store| store.trusts(self.authority.certificate()))
    }

    pub fn set_system_trust(&self, trusted: bool) -> anyhow::Result<()> {
        if trusted {
            anchor::install(self.authority.certificate())?;
            self.policy
                .retain(|rejection| rejection.client.is_windows());
        } else {
            anchor::remove()?;
        }

        self.policy.set_trusted_since(self.trusted_since());
        self.publish(Notice::Trust);
        Ok(())
    }

    fn trusted_since(&self) -> Option<SystemTime> {
        trust::system()
            .filter(|store| store.trusts(self.authority.certificate()))
            .and_then(|store| store.installed())
    }

    pub fn send(&self, outgoing: Outgoing) -> impl Future<Output = io::Result<Arc<Entry>>> + use<> {
        let composer = self.composer.clone();
        self.background(move || composer.send(outgoing))
    }

    pub fn replay(
        &self,
        entry: &Entry,
    ) -> Option<impl Future<Output = io::Result<Arc<Entry>>> + use<>> {
        let outgoing = {
            let record = entry.read();
            let request = record.request.as_ref()?;

            Outgoing {
                method: request.method.clone(),
                url: request.url(),
                headers: request.headers.clone(),
                body: request
                    .body
                    .bytes()
                    .map(|body| body.into_owned())
                    .unwrap_or_default(),
                address: address(entry.connection().server),
            }
        };

        Some(self.send(outgoing))
    }

    pub fn snapshot(&self, entry: Arc<Entry>) -> impl Future<Output = io::Result<PathBuf>> + use<> {
        self.background(move || snapshot::save(&entry))
    }

    pub fn resume(&self, id: u64, verdict: Verdict) {
        self.tamper.resume(id, verdict);
    }

    pub fn resume_all(&self) {
        self.tamper.release_all();
    }

    pub fn remove(&self, ids: &[u64]) {
        self.tamper.release(ids);
        self.store.remove(ids);
    }

    pub fn clear(&self) {
        self.tamper.release_all();
        self.store.clear();
    }

    pub fn background<T, F>(&self, task: F) -> impl Future<Output = T> + use<T, F>
    where
        T: Send + 'static,
        F: FnOnce() -> T + Send + 'static,
    {
        let handle = self.runtime.spawn_blocking(task);

        async move {
            handle
                .await
                .unwrap_or_else(|error| std::panic::resume_unwind(error.into_panic()))
        }
    }

    pub fn subscribe(&self) -> Receiver<Notice> {
        let (sender, receiver) = async_channel::unbounded();
        self.subscribers.lock().push(sender);
        receiver
    }

    fn publish(&self, notice: Notice) {
        self.subscribers
            .lock()
            .retain(|subscriber| subscriber.try_send(notice.clone()).is_ok());
    }

    pub fn is_capturing(&self) -> bool {
        self.capture.is_active()
    }

    pub fn is_capturing_all(&self) -> bool {
        self.capture.options().capture_all
    }

    pub fn start_capture(&self) -> anyhow::Result<()> {
        let interceptor = self
            .capture
            .acquire()
            .context("loading the capture programs")?;

        for target in self.targets.lock().iter() {
            interceptor.track(target.cgroup.id)?;
        }

        self.store.set_recording(true);
        self.capture.configure(|options| options.enabled = true)?;
        self.publish(Notice::Capture);
        Ok(())
    }

    pub fn stop_capture(&self) -> anyhow::Result<()> {
        self.capture.configure(|options| options.enabled = false)?;
        self.store.set_recording(false);
        self.publish(Notice::Capture);
        Ok(())
    }

    pub fn capture_all(&self, enabled: bool) -> anyhow::Result<()> {
        self.capture
            .configure(|options| options.capture_all = enabled)?;
        self.publish(Notice::Capture);
        Ok(())
    }

    pub fn targets(&self) -> Vec<Target> {
        self.targets.lock().clone()
    }

    pub fn enrol(
        &self,
        name: &str,
        process: u32,
        command: &str,
        origin: Origin,
    ) -> anyhow::Result<()> {
        let target = Target {
            name: name.to_owned(),
            process,
            command: command.to_owned(),
            cgroup: Cgroup::of(process)?,
            origin,
        };

        self.targets.lock().push(target);
        self.publish(Notice::Targets);
        self.start_capture()
    }

    pub fn attach(&self, pid: u32) -> anyhow::Result<String> {
        let all = processes::list();
        let process = all
            .iter()
            .find(|process| process.pid == pid)
            .context("the process has exited")?;

        let mut members = processes::tree(&all, pid);
        let environment = processes::environment(pid);

        let server = trust::detect(|name| environment.get(name).cloned(), &process.program())
            .and_then(|prefix| prefix.wineserver())
            .filter(|server| !members.contains(server));

        members.extend(server);

        let scope = Scope::create(&process.name, &members).context("creating the capture scope")?;

        if let Some(interceptor) = self.capture.interceptor() {
            interceptor.track(scope.cgroup.id)?;
        }

        let target = Target {
            name: process.name.clone(),
            process: pid,
            command: process.command_line(),
            cgroup: scope.cgroup,
            origin: Origin::Attached,
        };

        self.targets.lock().push(target);
        self.publish(Notice::Targets);

        Ok(process.name.clone())
    }

    pub fn release(&self, cgroup: u64) {
        self.targets
            .lock()
            .retain(|target| target.cgroup.id != cgroup);

        if let Some(interceptor) = self.capture.interceptor() {
            let _ = interceptor.untrack(cgroup);
        }

        self.publish(Notice::Targets);
    }

    pub fn settings(&self) -> Settings {
        self.settings.lock().clone()
    }

    pub fn update(&self, change: impl FnOnce(&mut Settings)) -> anyhow::Result<()> {
        let (settings, previous) = {
            let mut settings = self.settings.lock();
            let previous = settings.clone();
            change(&mut settings);
            (settings.clone(), previous)
        };

        if settings != previous {
            self.apply(&settings)?;
            settings.save().context("saving the settings")?;
        }

        if settings.responders != previous.responders
            || settings.breakpoints != previous.breakpoints
        {
            self.publish(Notice::Rules);
        }

        Ok(())
    }

    fn apply(&self, settings: &Settings) -> anyhow::Result<()> {
        self.policy.set_rules(&settings.undecrypted);
        self.tamper
            .configure(&settings.responders, &settings.breakpoints);
        self.capture.exclude(
            settings
                .exclusions
                .iter()
                .filter_map(|text| network(text))
                .collect(),
        )?;

        let port = self.port;
        self.capture.configure(|options| {
            options.proxy_port = port;
            options.block_quic = settings.block_quic;
        })?;

        Ok(())
    }

    fn prune(&self) {
        let ended: Vec<u64> = self
            .targets
            .lock()
            .iter()
            .filter(|target| !target.cgroup.exists())
            .map(|target| target.cgroup.id)
            .collect();

        for cgroup in ended {
            self.release(cgroup);
        }
    }
}

pub fn address(server: SocketAddr) -> Option<SocketAddr> {
    (!server.ip().is_unspecified() && server.port() != 0).then_some(server)
}

async fn watch(service: Arc<Service>) {
    let mut interval = tokio::time::interval(WATCH);

    loop {
        interval.tick().await;

        let service = service.clone();
        let _ = tokio::task::spawn_blocking(move || {
            service.prune();
            service.capture.refresh();
        })
        .await;
    }
}
