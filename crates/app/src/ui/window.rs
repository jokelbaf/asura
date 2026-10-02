use std::cell::{Cell, RefCell};
use std::collections::{BTreeSet, HashSet};
use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter, Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use adw::prelude::*;
use async_channel::Receiver;
use gtk::{gio, glib};
use proxy::{Action, Breakpoint, RequestDraft, Responder};
use session::{Entry, Event, Query, Store, archive, har};

use super::chrome::{self, Heading, Toggle};
use super::completion::Completion;
use super::compose::{self, Origin};
use super::inspector::Inspector;
use super::sessions::{Exchange, Sessions};
use super::targets::Targets;
use super::{attach, curl, files, format, hints, launch, rules, search};
use crate::service::{self, Geometry, Notice, Service};

type Handler = fn(&Rc<Window>);

const PUMP: Duration = Duration::from_millis(100);
const ZSTD: [u8; 4] = [0x28, 0xb5, 0x2f, 0xfd];
const NARROW: &str = "max-width: 860sp";
const RECORDED: [&str; 4] = ["save", "export", "clear", "search"];
const SELECTION: [&str; 9] = [
    "copy-url",
    "copy-curl",
    "export-selected",
    "skip-host",
    "remove",
    "replay",
    "resend",
    "respond-with",
    "break-here",
];

pub struct Window {
    pub window: adw::ApplicationWindow,
    service: Arc<Service>,
    heading: Heading,
    toggle: Toggle,
    lists: gtk::Stack,
    paned: gtk::Paned,
    split: Rc<Cell<i32>>,
    filter: gtk::SearchEntry,
    sessions: Rc<Sessions>,
    inspector: Inspector,
    targets: Rc<Targets>,
    capture: gio::SimpleAction,
    toasts: adw::ToastOverlay,
    rules: gtk::Button,
    paused: adw::Banner,
    found: adw::Banner,
    waiting: RefCell<HashSet<u64>>,
    announcement: Rc<RefCell<Option<adw::Toast>>>,
    pending: Cell<Option<u64>>,
    query: RefCell<Option<Query>>,
    rejections: Cell<u64>,
    warned: Cell<bool>,
    warning: RefCell<Option<adw::Toast>>,
    restarts: RefCell<BTreeSet<String>>,
    reminder: Rc<RefCell<Option<adw::Toast>>>,
}

impl Window {
    pub fn new(application: &adw::Application, service: Arc<Service>) -> Rc<Self> {
        let geometry = service.settings().window;

        let window = adw::ApplicationWindow::builder()
            .application(application)
            .title("Asura")
            .default_width(geometry.width)
            .default_height(geometry.height)
            .maximized(geometry.maximized)
            .width_request(400)
            .height_request(480)
            .build();

        let capture = gio::SimpleAction::new_stateful("capture", None, &false.to_variant());

        let toggle = Toggle::new();
        let targets = Targets::new(service.clone());
        let heading = Heading::new();

        let menu = gtk::MenuButton::builder()
            .icon_name("open-menu-symbolic")
            .menu_model(&chrome::main_menu())
            .primary(true)
            .tooltip_text("Main Menu")
            .build();

        let rules = gtk::Button::builder()
            .icon_name("applications-engineering-symbolic")
            .action_name("win.rules")
            .tooltip_text("Rules (Ctrl+B)")
            .build();

        let header = adw::HeaderBar::builder()
            .title_widget(&heading.widget)
            .build();
        header.pack_start(&toggle.button);
        header.pack_start(&targets.button);
        header.pack_end(&menu);
        header.pack_end(&rules);

        let filter = gtk::SearchEntry::builder()
            .placeholder_text("Filter, e.g. host:example.com status:4xx")
            .hexpand(true)
            .search_delay(150)
            .build();

        let bar = gtk::Box::builder()
            .spacing(6)
            .margin_start(6)
            .margin_end(6)
            .margin_top(6)
            .margin_bottom(6)
            .build();
        bar.append(&filter);
        bar.append(&chrome::session_tools());

        let sessions = Sessions::new(chrome::context_menu().upcast_ref());

        let lists = gtk::Stack::builder()
            .vexpand(true)
            .transition_type(gtk::StackTransitionType::Crossfade)
            .transition_duration(150)
            .build();
        lists.add_named(&chrome::welcome(), Some("welcome"));
        lists.add_named(&chrome::unmatched(), Some("unmatched"));
        lists.add_named(&sessions.widget, Some("sessions"));

        let paused = adw::Banner::builder()
            .button_label("Resume All")
            .action_name("win.resume-all")
            .build();
        let found = adw::Banner::builder()
            .use_markup(false)
            .button_label("Clear")
            .action_name("win.clear-search")
            .build();

        let list = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .width_request(360)
            .build();
        list.append(&bar);
        list.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        list.append(&paused);
        list.append(&found);
        list.append(&lists);

        let inspector = Inspector::new(service.clone());
        inspector.widget.set_width_request(360);

        let paned = gtk::Paned::builder()
            .orientation(gtk::Orientation::Horizontal)
            .start_child(&list)
            .end_child(&inspector.widget)
            .shrink_start_child(false)
            .shrink_end_child(false)
            .position(geometry.split)
            .build();

        let toolbar = adw::ToolbarView::builder().content(&paned).build();
        toolbar.add_top_bar(&header);

        let toasts = adw::ToastOverlay::new();
        toasts.set_child(Some(&toolbar));
        window.set_content(Some(&toasts));

        let split = Rc::new(Cell::new(geometry.split));
        window.add_breakpoint(narrow(&window, &paned, &split));

        let weak = Rc::downgrade(&sessions);
        Completion::attach(
            &filter,
            hints::filter(move || {
                weak.upgrade()
                    .map(|sessions| sessions.catalog())
                    .unwrap_or_default()
            }),
        );

        let this = Rc::new(Self {
            window,
            service,
            heading,
            toggle,
            lists,
            paned,
            split,
            filter,
            sessions,
            inspector,
            targets,
            capture,
            toasts,
            rules,
            paused,
            found,
            waiting: RefCell::default(),
            announcement: Rc::default(),
            pending: Cell::new(None),
            query: RefCell::default(),
            rejections: Cell::new(0),
            warned: Cell::new(false),
            warning: RefCell::default(),
            restarts: RefCell::default(),
            reminder: Rc::default(),
        });

        this.actions();
        this.signals();
        this.pump();
        this.selected();
        this.sync();
        this
    }

    pub fn present(&self) {
        self.window.present();
    }

    pub fn open(self: &Rc<Self>, path: PathBuf) {
        let store = self.service.store().clone();
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let window = self.clone();

        glib::spawn_future_local(async move {
            match window.service.background(move || load(&store, &path)).await {
                Ok(1) => window.notify(&format!("Loaded 1 request from {name}")),
                Ok(count) => window.notify(&format!("Loaded {count} requests from {name}")),
                Err(error) => window.notify(&format!("Could not open {name}: {error}")),
            }
        });
    }

    fn actions(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        self.capture.connect_change_state(move |action, value| {
            if let (Some(window), Some(desired)) =
                (weak.upgrade(), value.and_then(|value| value.get::<bool>()))
            {
                window.toggle(action, desired);
            }
        });
        self.window.add_action(&self.capture);

        let inspect = gio::SimpleAction::new("inspect", Some(glib::VariantTy::UINT64));
        let weak = Rc::downgrade(self);
        inspect.connect_activate(move |_, parameter| {
            if let (Some(window), Some(id)) = (
                weak.upgrade(),
                parameter.and_then(|parameter| parameter.get::<u64>()),
            ) {
                window.reveal(id);
            }
        });
        self.window.add_action(&inspect);

        let handlers: [(&str, Handler); 24] = [
            ("start", Self::start),
            ("shortcuts", Self::shortcuts),
            ("open", Self::choose),
            ("save", Self::save),
            ("export", Self::export_visible),
            ("export-selected", Self::export_selected),
            ("clear", Self::clear),
            ("remove", Self::remove),
            ("copy-url", Self::copy_url),
            ("copy-curl", Self::copy_curl),
            ("skip-host", Self::skip_host),
            ("launch", Self::launch),
            ("attach", Self::attach),
            ("find", Self::find),
            ("replay", Self::replay),
            ("resend", Self::resend),
            ("compose", Self::compose),
            ("respond-with", Self::respond_with),
            ("break-here", Self::break_here),
            ("rules", Self::rules),
            ("search", Self::search),
            ("clear-search", Self::clear_search),
            ("resume-all", Self::resume_all),
            ("trust", Self::trust),
        ];

        for (name, handler) in handlers {
            let action = gio::SimpleAction::new(name, None);
            let weak = Rc::downgrade(self);

            action.connect_activate(move |_, _| {
                if let Some(window) = weak.upgrade() {
                    handler(&window);
                }
            });

            self.window.add_action(&action);
        }
    }

    fn signals(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        self.sessions.connect_selection(move || {
            if let Some(window) = weak.upgrade() {
                window.selected();
            }
        });

        let weak = Rc::downgrade(self);
        self.filter.connect_search_changed(move |entry| {
            if let Some(window) = weak.upgrade() {
                window.sessions.set_query(&entry.text());
                window.status();
            }
        });

        self.filter.connect_stop_search(|entry| entry.set_text(""));

        let weak = Rc::downgrade(self);
        self.window.connect_close_request(move |_| {
            if let Some(window) = weak.upgrade() {
                window.remember();
            }

            glib::Propagation::Proceed
        });

        self.sessions.shortcut("Delete", "win.remove");
        self.sessions.shortcut("<Control>c", "win.copy-url");
    }

    fn pump(self: &Rc<Self>) {
        let events = self.service.store().subscribe();
        let notices = self.service.subscribe();
        self.sessions.seed(self.service.store().entries());

        let weak = Rc::downgrade(self);
        glib::timeout_add_local(PUMP, move || {
            let Some(window) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };

            window.drain(&events, &notices);
            glib::ControlFlow::Continue
        });
    }

    fn drain(self: &Rc<Self>, events: &Receiver<Event>, notices: &Receiver<Notice>) {
        let dirty = self.sessions.drain(events);
        self.inspector.refresh(&dirty);
        self.settle();
        self.breakpoints();
        self.rejections();

        let mut capture = false;
        let mut targets = false;

        while let Ok(notice) = notices.try_recv() {
            match notice {
                Notice::Capture => capture = true,
                Notice::Targets => targets = true,
                Notice::Rules => self.sync_rules(),
                Notice::Trust => {
                    if let Some(toast) = self.warning.take() {
                        toast.dismiss();
                    }
                }
            }
        }

        if targets {
            self.targets.update();
        }

        if capture {
            self.sync();
        }

        self.status();
    }

    fn sync(&self) {
        let capturing = self.service.is_capturing();

        self.capture.set_state(&capturing.to_variant());
        self.toggle.show(capturing);
        self.targets.update();
        self.sync_rules();
        self.status();
    }

    fn sync_rules(&self) {
        let settings = self.service.settings();
        let active = settings
            .responders
            .iter()
            .filter(|rule| rule.enabled)
            .count()
            + settings
                .breakpoints
                .iter()
                .filter(|rule| rule.enabled)
                .count();

        if active > 0 {
            self.rules.add_css_class("accent");
            self.rules
                .set_tooltip_text(Some(&format!("Rules · {active} active (Ctrl+B)")));
        } else {
            self.rules.remove_css_class("accent");
            self.rules.set_tooltip_text(Some("Rules (Ctrl+B)"));
        }
    }

    fn breakpoints(&self) {
        let paused: HashSet<u64> = self.service.tamper().paused().into_iter().collect();
        let fresh: Vec<u64> = paused.difference(&self.waiting.borrow()).copied().collect();

        self.paused.set_revealed(!paused.is_empty());
        self.paused.set_title(&match paused.len() {
            1 => String::from("1 request is paused at a breakpoint"),
            count => format!("{count} requests are paused at breakpoints"),
        });

        if paused.is_empty()
            && let Some(toast) = self.announcement.take()
        {
            toast.dismiss();
        }

        self.waiting.replace(paused);

        let Some(&latest) = fresh.iter().max() else {
            return;
        };

        if self.sessions.selected_count() == 0 {
            self.reveal(latest);
        }

        let title = match fresh.len() {
            1 => self
                .service
                .store()
                .get(latest)
                .and_then(|entry| {
                    entry
                        .read()
                        .request
                        .as_ref()
                        .map(|request| format!("Paused {} {}", request.method, request.url()))
                })
                .unwrap_or_else(|| String::from("A request paused at a breakpoint")),
            count => format!("{count} requests paused at breakpoints"),
        };

        let toast = self.announcement.borrow().clone();

        let toast = toast.unwrap_or_else(|| {
            let toast = adw::Toast::builder()
                .use_markup(false)
                .button_label("Inspect")
                .action_name("win.inspect")
                .action_target(&latest.to_variant())
                .timeout(0)
                .build();

            let weak = Rc::downgrade(&self.announcement);
            toast.connect_dismissed(move |_| {
                if let Some(announcement) = weak.upgrade() {
                    announcement.replace(None);
                }
            });

            self.toasts.add_toast(toast.clone());
            self.announcement.replace(Some(toast.clone()));
            toast
        });

        toast.set_title(&format::printable(&title));
        toast.set_action_target_value(Some(&latest.to_variant()));
    }

    fn rejections(self: &Rc<Self>) {
        let revision = self.service.policy().revision();

        if self.rejections.replace(revision) == revision {
            return;
        }

        let bypassed = self.service.policy().bypassed();

        let fresh = bypassed
            .iter()
            .filter(|(_, rejection)| rejection.stale)
            .filter(|(_, rejection)| {
                self.restarts
                    .borrow_mut()
                    .insert(rejection.client.name.clone())
            })
            .count();

        if fresh > 0 {
            self.remind();
        }

        let native = bypassed
            .iter()
            .any(|(_, rejection)| !rejection.stale && !rejection.client.is_windows());

        if self.warned.get() || !native {
            return;
        }

        self.warned.set(true);

        let service = self.service.clone();
        let window = self.clone();

        glib::spawn_future_local(async move {
            if window
                .service
                .background(move || service.system_trust())
                .await
                == Some(true)
            {
                return;
            }

            let toast = adw::Toast::builder()
                .title("Linux applications do not trust the Asura certificate")
                .button_label("Set Up…")
                .action_name("win.trust")
                .timeout(0)
                .build();

            window.toasts.add_toast(toast.clone());
            window.warning.replace(Some(toast));
        });
    }

    fn remind(&self) {
        let names: Vec<String> = self.restarts.borrow().iter().cloned().collect();
        let title = format!(
            "Started before the certificate was installed, restart to decrypt: {}",
            names.join(", ")
        );

        let toast = self.reminder.borrow().clone();

        let toast = toast.unwrap_or_else(|| {
            let toast = adw::Toast::builder().use_markup(false).timeout(0).build();

            let weak = Rc::downgrade(&self.reminder);
            toast.connect_dismissed(move |_| {
                if let Some(reminder) = weak.upgrade() {
                    reminder.replace(None);
                }
            });

            self.toasts.add_toast(toast.clone());
            self.reminder.replace(Some(toast.clone()));
            toast
        });

        toast.set_title(&format::printable(&title));
    }

    fn reveal(&self, id: u64) {
        if !self.sessions.select(id) {
            self.pending.set(Some(id));
        }
    }

    fn settle(&self) {
        let Some(id) = self.pending.get() else {
            return;
        };

        if self.sessions.select(id)
            || self.sessions.contains(id)
            || self.service.store().get(id).is_none()
        {
            self.pending.set(None);
        }
    }

    fn status(&self) {
        let total = self.sessions.total();
        let shown = self.sessions.shown();

        let count = match (shown, total) {
            (_, 1) if shown == total => String::from("1 request"),
            _ if shown == total => format!("{total} requests"),
            _ => format!("{shown} of {total} requests"),
        };

        self.heading.show(self.service.is_capturing(), &count);
        self.lists.set_visible_child_name(match (total, shown) {
            (0, _) => "welcome",
            (_, 0) => "unmatched",
            _ => "sessions",
        });

        for name in RECORDED {
            if let Some(action) = self
                .window
                .lookup_action(name)
                .and_downcast::<gio::SimpleAction>()
            {
                action.set_enabled(total > 0);
            }
        }
    }

    fn selected(&self) {
        let count = self.sessions.selected_count();

        match count {
            0 => self.inspector.show(None),
            1 => self
                .inspector
                .show(self.sessions.first_selected().as_ref().map(Exchange::entry)),
            many => self.inspector.show_many(many),
        }

        for name in SELECTION {
            if let Some(action) = self
                .window
                .lookup_action(name)
                .and_downcast::<gio::SimpleAction>()
            {
                action.set_enabled(count > 0);
            }
        }
    }

    fn toggle(self: &Rc<Self>, action: &gio::SimpleAction, desired: bool) {
        action.set_enabled(false);
        self.toggle.busy();

        let window = self.clone();
        let action = action.clone();

        glib::spawn_future_local(async move {
            let service = window.service.clone();
            let result = window
                .service
                .background(move || {
                    if desired {
                        service.start_capture()
                    } else {
                        service.stop_capture()
                    }
                })
                .await;

            action.set_enabled(true);

            if let Err(error) = result {
                window.notify(&format!("{error:#}"));
            }

            window.sync();
        });
    }

    fn start(self: &Rc<Self>) {
        if !self.service.is_capturing() {
            self.capture.change_state(&true.to_variant());
        }
    }

    fn shortcuts(self: &Rc<Self>) {
        chrome::shortcuts().present(Some(&self.window));
    }

    fn remember(&self) {
        let (width, height) = self.window.default_size();
        let split = match self.paned.orientation() {
            gtk::Orientation::Horizontal => self.paned.position(),
            _ => self.split.get(),
        };

        let geometry = Geometry {
            width,
            height,
            maximized: self.window.is_maximized(),
            split,
        };

        if let Err(error) = self.service.update(|settings| settings.window = geometry) {
            eprintln!("asura: {error:#}");
        }
    }

    fn choose(self: &Rc<Self>) {
        let window = self.clone();

        glib::spawn_future_local(async move {
            if let Some(path) = files::open(
                &window.window,
                "Open Session",
                &[&files::CAPTURES, &files::ANY],
            )
            .await
            {
                window.open(path);
            }
        });
    }

    fn save(self: &Rc<Self>) {
        let entries = self.service.store().entries();

        if entries.is_empty() {
            return self.notify("There are no requests to save");
        }

        let name = format!("asura-{}.asura", files::stamp());
        self.write("Save Session", name, &files::SESSIONS, move |writer| {
            archive::save(&entries, writer)
        });
    }

    fn export_visible(self: &Rc<Self>) {
        self.export(self.sessions.visible());
    }

    fn export_selected(self: &Rc<Self>) {
        self.export(self.selection());
    }

    fn export(self: &Rc<Self>, entries: Vec<Arc<Entry>>) {
        if entries.is_empty() {
            return self.notify("There are no requests to export");
        }

        let name = format!("asura-{}.har", files::stamp());
        self.write("Export as HAR", name, &files::HAR, move |writer| {
            serde_json::to_writer(writer, &har::export(&entries)).map_err(io::Error::from)
        });
    }

    fn write(
        self: &Rc<Self>,
        title: &'static str,
        name: String,
        kind: &'static files::Kind,
        content: impl FnOnce(&mut BufWriter<File>) -> io::Result<()> + Send + 'static,
    ) {
        let window = self.clone();

        glib::spawn_future_local(async move {
            let Some(path) = files::save(&window.window, title, &name, &[kind]).await else {
                return;
            };

            let display = path.display().to_string();

            match window
                .service
                .background(move || store(&path, content))
                .await
            {
                Ok(()) => window.notify(&format!("Saved {display}")),
                Err(error) => window.notify(&format!("Could not save {display}: {error}")),
            }
        });
    }

    fn clear(self: &Rc<Self>) {
        let body = match self.sessions.total() {
            0 => return,
            1 => String::from(
                "The captured request will be removed. Save the session first to keep it.",
            ),
            count => format!(
                "All {count} captured requests will be removed. Save the session first to keep them."
            ),
        };

        let dialog = adw::AlertDialog::builder()
            .heading("Clear Session?")
            .body(body)
            .close_response("cancel")
            .default_response("clear")
            .build();

        dialog.add_responses(&[("cancel", "_Cancel"), ("clear", "C_lear")]);
        dialog.set_response_appearance("clear", adw::ResponseAppearance::Destructive);

        let service = self.service.clone();
        dialog.connect_response(Some("clear"), move |_, _| service.clear());
        dialog.present(Some(&self.window));
    }

    fn remove(self: &Rc<Self>) {
        let ids: Vec<u64> = self.sessions.selected().iter().map(Exchange::id).collect();
        self.service.remove(&ids);
    }

    fn replay(self: &Rc<Self>) {
        let replays: Vec<_> = self
            .selection()
            .iter()
            .filter_map(|entry| self.service.replay(entry))
            .collect();

        if replays.is_empty() {
            return self.notify("The selected requests have no HTTP request");
        }

        let single = replays.len() == 1;
        let window = self.clone();

        glib::spawn_future_local(async move {
            let mut sent = 0;

            for replay in replays {
                match replay.await {
                    Ok(entry) if single => window.reveal(entry.id()),
                    Ok(_) => sent += 1,
                    Err(error) => window.notify(&format!("Could not replay the request: {error}")),
                }
            }

            if sent > 0 {
                window.notify(&format!("Replayed {sent} requests"));
            }
        });
    }

    fn resend(self: &Rc<Self>) {
        let source = self.selection().into_iter().find_map(|entry| {
            let record = entry.read();
            let request = record.request.as_ref()?;

            let mut draft = RequestDraft::capture(request);
            draft.target = request.url();

            let origin = service::address(entry.connection().server).map(|address| Origin {
                authority: request.authority.clone(),
                address,
            });

            Some((draft, origin))
        });

        match source {
            Some(source) => self.composer(Some(source)),
            None => self.notify("The selected request has no HTTP request"),
        }
    }

    fn compose(self: &Rc<Self>) {
        self.composer(None);
    }

    fn composer(self: &Rc<Self>, source: Option<(RequestDraft, Option<Origin>)>) {
        let weak = Rc::downgrade(self);

        compose::present(&self.window, self.service.clone(), source, move |id| {
            if let Some(window) = weak.upgrade() {
                window.reveal(id);
            }
        });
    }

    fn respond_with(self: &Rc<Self>) {
        let Some(entry) = self
            .selection()
            .into_iter()
            .find(|entry| entry.read().request.is_some())
        else {
            return self.notify("The selected request has no HTTP request");
        };

        let (pattern, method, answered) = {
            let record = entry.read();
            let request = record.request.as_ref().expect("checked above");
            (
                format!("exact:{}", request.url()),
                request.method.clone(),
                record.response.is_some(),
            )
        };

        let window = self.clone();

        glib::spawn_future_local(async move {
            let action = if answered {
                match window.service.snapshot(entry).await {
                    Ok(path) => Action::File(path),
                    Err(error) => {
                        return window.notify(&format!("Could not save the response: {error}"));
                    }
                }
            } else {
                Action::Mock {
                    status: 200,
                    content_type: String::new(),
                    body: String::new(),
                }
            };

            let rule = Responder {
                enabled: true,
                pattern,
                method,
                action,
            };

            rules::respond(&window.window, &window.service, rule);
        });
    }

    fn break_here(self: &Rc<Self>) {
        let rule = self.selection().into_iter().find_map(|entry| {
            let record = entry.read();
            let request = record.request.as_ref()?;
            let url = request.url();

            Some(Breakpoint {
                enabled: true,
                pattern: url.split(['?', '#']).next().unwrap_or_default().to_owned(),
                method: request.method.clone(),
                request: true,
                response: false,
            })
        });

        match rule {
            Some(rule) => rules::pause(&self.window, &self.service, rule),
            None => self.notify("The selected request has no HTTP request"),
        }
    }

    fn rules(self: &Rc<Self>) {
        rules::present(&self.window, &self.service);
    }

    fn search(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);

        search::present(
            &self.window,
            self.service.clone(),
            self.query.borrow().clone(),
            move |query, matches| {
                if let Some(window) = weak.upgrade() {
                    window.found(query, matches);
                }
            },
        );
    }

    fn found(&self, query: Query, matches: HashSet<u64>) {
        let count = matches.len();

        self.found.set_title(&format::printable(&match count {
            1 => format!("1 request contains “{}”", query.text),
            _ => format!("{count} requests contain “{}”", query.text),
        }));
        self.found.set_revealed(true);
        self.sessions.set_matches(Some(matches));
        self.inspector.highlight(Some(&query));
        self.query.replace(Some(query));
        self.status();
    }

    fn clear_search(self: &Rc<Self>) {
        self.found.set_revealed(false);
        self.sessions.set_matches(None);
        self.inspector.highlight(None);
        self.status();
    }

    fn resume_all(self: &Rc<Self>) {
        self.service.resume_all();
    }

    fn copy_url(self: &Rc<Self>) {
        let urls: Vec<String> = self
            .sessions
            .selected()
            .iter()
            .map(|exchange| exchange.summary().url.clone())
            .collect();

        self.window.clipboard().set_text(&urls.join("\n"));
    }

    fn copy_curl(self: &Rc<Self>) {
        let command = self
            .selection()
            .iter()
            .find_map(|entry| entry.read().request.as_ref().map(curl::command));

        match command {
            Some(command) => {
                self.window.clipboard().set_text(&command);
                self.notify("Copied as cURL");
            }
            None => self.notify("The selected request has no HTTP request"),
        }
    }

    fn skip_host(self: &Rc<Self>) {
        let hosts: BTreeSet<String> = self
            .selection()
            .iter()
            .map(|entry| {
                let connection = entry.connection();

                connection
                    .tls
                    .as_ref()
                    .and_then(|tls| tls.server_name.clone())
                    .unwrap_or_else(|| connection.server.ip().to_string())
            })
            .collect();

        let result = self.service.update(|settings| {
            for host in &hosts {
                if !settings.undecrypted.contains(host) {
                    settings.undecrypted.push(host.clone());
                }
            }
        });

        match result {
            Ok(()) => self.notify(&format!(
                "{} will no longer be decrypted",
                hosts.into_iter().collect::<Vec<_>>().join(", ")
            )),
            Err(error) => self.notify(&format!("{error:#}")),
        }
    }

    fn launch(self: &Rc<Self>) {
        launch::present(&self.window, self.service.clone());
    }

    fn attach(self: &Rc<Self>) {
        attach::present(&self.window, self.service.clone());
    }

    fn find(self: &Rc<Self>) {
        let focus = gtk::prelude::GtkWindowExt::focus(&self.window);

        if !self.inspector.find(focus.as_ref()) {
            self.filter.grab_focus();
        }
    }

    fn selection(&self) -> Vec<Arc<Entry>> {
        self.sessions
            .selected()
            .iter()
            .map(|exchange| exchange.entry().clone())
            .collect()
    }

    fn trust(self: &Rc<Self>) {
        super::preferences::present_decryption(&self.window, &self.service);
    }

    fn notify(&self, message: &str) {
        super::notify(&self.window, message);
    }
}

fn load(store: &Store, path: &Path) -> io::Result<usize> {
    let mut file = File::open(path)?;
    let mut magic = [0u8; 4];
    let compressed = file.read_exact(&mut magic).is_ok() && magic == ZSTD;
    file.rewind()?;

    if compressed {
        archive::load(store, BufReader::new(file))
    } else {
        har::import(store, &fs::read(path)?)
    }
}

fn store(
    path: &Path,
    content: impl FnOnce(&mut BufWriter<File>) -> io::Result<()>,
) -> io::Result<()> {
    let partial = path.with_extension("partial");
    let mut writer = BufWriter::new(File::create(&partial)?);

    content(&mut writer)?;
    writer.flush()?;
    drop(writer);

    fs::rename(partial, path)
}

fn narrow(
    window: &adw::ApplicationWindow,
    paned: &gtk::Paned,
    split: &Rc<Cell<i32>>,
) -> adw::Breakpoint {
    let breakpoint = adw::Breakpoint::new(
        adw::BreakpointCondition::parse(NARROW).expect("valid breakpoint condition"),
    );

    breakpoint.add_setters(&[
        (paned, "orientation", gtk::Orientation::Vertical.to_value()),
        (paned, "shrink-start-child", true.to_value()),
        (paned, "shrink-end-child", true.to_value()),
    ]);

    breakpoint.connect_apply(glib::clone!(
        #[weak]
        window,
        #[weak]
        paned,
        #[strong]
        split,
        move |_| {
            split.set(paned.position());
            paned.set_position(window.height() * 2 / 5);
        }
    ));

    breakpoint.connect_unapply(glib::clone!(
        #[weak]
        paned,
        #[strong]
        split,
        move |_| paned.set_position(split.get())
    ));

    breakpoint
}
