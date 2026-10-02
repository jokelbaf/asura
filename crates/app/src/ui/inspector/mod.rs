mod body;
mod breakpoint;
mod content;
mod fields;
mod finder;
mod frames;
mod message;
mod overview;
mod stream;
mod table;

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use adw::prelude::*;
use gtk::pango;
use session::{Entry, Query, Record, Stage, State};

use self::body::BodyView;
use self::breakpoint::BreakpointPage;
use self::content::{Part, Signature};
use self::frames::FramesPage;
use self::message::MessagePage;
use self::overview::Overview;
use self::stream::StreamPage;
use crate::service::Service;
use crate::ui::format;

pub use self::content::{TEXT_LIMIT, language};

const THROTTLE: Duration = Duration::from_secs(1);
const PREFERRED: [&str; 3] = ["request", "stream", "overview"];

#[derive(Clone, PartialEq)]
struct Heads {
    state: State,
    paused: Option<Stage>,
    request: Option<(usize, usize)>,
    response: Option<(u16, usize, usize)>,
    upgraded: bool,
}

#[derive(Clone, PartialEq)]
struct Contents {
    request: Option<Signature>,
    response: Option<Signature>,
    frames: usize,
    tunnel: Option<(u64, u64)>,
}

struct Current {
    entry: Arc<Entry>,
    heads: Heads,
    contents: Contents,
    rendered: Instant,
}

struct Pages {
    breakpoint: adw::ViewStackPage,
    request: adw::ViewStackPage,
    response: adw::ViewStackPage,
    messages: adw::ViewStackPage,
    stream: adw::ViewStackPage,
}

pub struct Inspector {
    pub widget: gtk::Stack,
    many: adw::StatusPage,
    method: gtk::Label,
    url: gtk::Label,
    badge: gtk::Label,
    stack: adw::ViewStack,
    pages: Pages,
    breakpoint: Rc<BreakpointPage>,
    overview: Overview,
    request: MessagePage,
    response: MessagePage,
    frames: Rc<FramesPage>,
    stream: StreamPage,
    current: RefCell<Option<Current>>,
    stale: Cell<bool>,
}

impl Inspector {
    pub fn new(service: Arc<Service>) -> Self {
        let breakpoint = BreakpointPage::new(service);
        let overview = Overview::new();
        let request = MessagePage::new(Part::Request);
        let response = MessagePage::new(Part::Response);
        let frames = FramesPage::new();
        let stream = StreamPage::new();

        let stack = adw::ViewStack::builder()
            .vexpand(true)
            .enable_transitions(true)
            .transition_duration(150)
            .build();
        let paused = stack.add_titled_with_icon(
            &breakpoint.widget,
            Some("breakpoint"),
            "Breakpoint",
            "media-playback-pause-symbolic",
        );
        paused.set_visible(false);
        stack.add_titled_with_icon(
            &overview.widget,
            Some("overview"),
            "Overview",
            "dialog-information-symbolic",
        );

        let pages = Pages {
            breakpoint: paused,
            request: stack.add_titled_with_icon(
                &request.widget,
                Some("request"),
                "Request",
                "go-up-symbolic",
            ),
            response: stack.add_titled_with_icon(
                &response.widget,
                Some("response"),
                "Response",
                "go-down-symbolic",
            ),
            messages: stack.add_titled_with_icon(
                &frames.widget,
                Some("messages"),
                "Messages",
                "network-transmit-receive-symbolic",
            ),
            stream: stack.add_titled_with_icon(
                &stream.widget,
                Some("stream"),
                "Stream",
                "network-wired-symbolic",
            ),
        };

        stack.set_visible_child_name("request");

        let switcher = adw::InlineViewSwitcher::builder()
            .stack(&stack)
            .halign(gtk::Align::Center)
            .can_shrink(true)
            .margin_start(6)
            .margin_end(6)
            .margin_bottom(6)
            .css_classes(["flat"])
            .build();

        let method = gtk::Label::builder().css_classes(["heading"]).build();
        let url = gtk::Label::builder()
            .hexpand(true)
            .xalign(0.0)
            .selectable(true)
            .ellipsize(pango::EllipsizeMode::Middle)
            .css_classes(["inspector-title"])
            .build();
        let badge = gtk::Label::builder().css_classes(["status-badge"]).build();

        let header = gtk::Box::builder()
            .spacing(10)
            .margin_start(12)
            .margin_end(12)
            .margin_top(8)
            .margin_bottom(8)
            .build();
        header.append(&method);
        header.append(&url);
        header.append(&badge);

        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content.append(&header);
        content.append(&switcher);
        content.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        content.append(&stack);

        let none = adw::StatusPage::builder()
            .icon_name("edit-find-symbolic")
            .title("No Request Selected")
            .description("Select a request to inspect it and its response")
            .build();
        let many = adw::StatusPage::builder()
            .icon_name("edit-select-all-symbolic")
            .build();

        let widget = gtk::Stack::builder()
            .transition_type(gtk::StackTransitionType::Crossfade)
            .transition_duration(120)
            .build();
        widget.add_named(&none, Some("none"));
        widget.add_named(&many, Some("many"));
        widget.add_named(&content, Some("entry"));

        Self {
            widget,
            many,
            method,
            url,
            badge,
            stack,
            pages,
            breakpoint,
            overview,
            request,
            response,
            frames,
            stream,
            current: RefCell::default(),
            stale: Cell::new(false),
        }
    }

    pub fn show(&self, entry: Option<&Arc<Entry>>) {
        let Some(entry) = entry else {
            self.reset();
            return self.widget.set_visible_child_name("none");
        };

        let unchanged = self
            .current
            .borrow()
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(&current.entry, entry));

        if !unchanged {
            self.render(entry, false);
        }

        self.widget.set_visible_child_name("entry");
    }

    pub fn show_many(&self, count: u64) {
        self.reset();
        self.many.set_title(&format!("{count} Requests Selected"));
        self.many
            .set_description(Some("Use the context menu to export, copy or remove them"));
        self.widget.set_visible_child_name("many");
    }

    pub fn refresh(&self, dirty: &HashSet<u64>) {
        let Some((entry, heads, contents, rendered)) =
            self.current.borrow().as_ref().map(|current| {
                (
                    current.entry.clone(),
                    current.heads.clone(),
                    current.contents.clone(),
                    current.rendered,
                )
            })
        else {
            return;
        };

        if dirty.contains(&entry.id()) {
            self.stale.set(true);
        }

        if !self.stale.get() {
            return;
        }

        let (fresh_heads, fresh_contents) = {
            let record = entry.read();
            (heads_of(&record), contents_of(&record))
        };

        let moved = fresh_heads != heads;
        let grown = fresh_contents != contents;

        if moved || (grown && rendered.elapsed() >= THROTTLE) {
            self.render(&entry, !moved);
        }

        self.stale
            .set(!moved && grown && rendered.elapsed() < THROTTLE);
    }

    pub fn find(&self, focus: Option<&gtk::Widget>) -> bool {
        let Some(focus) = focus.filter(|focus| focus.is_ancestor(&self.widget)) else {
            return false;
        };

        if let Some(body) = self.bodies().into_iter().find(|body| body.contains(focus)) {
            body.find();
            return true;
        }

        let body = match self.stack.visible_child_name().as_deref() {
            Some("request") => &self.request.body,
            Some("response") => &self.response.body,
            Some("messages") => &self.frames.body,
            Some("stream") => &self.stream.received,
            _ => return false,
        };

        body.find();
        true
    }

    pub fn highlight(&self, query: Option<&Query>) {
        for body in self.bodies() {
            body.highlight(query);
        }
    }

    fn bodies(&self) -> [&Rc<BodyView>; 5] {
        [
            &self.request.body,
            &self.response.body,
            &self.frames.body,
            &self.stream.sent,
            &self.stream.received,
        ]
    }

    fn render(&self, entry: &Arc<Entry>, refresh: bool) {
        let record = entry.read();
        let connection = entry.connection();

        let previous = self
            .current
            .borrow()
            .as_ref()
            .filter(|current| Arc::ptr_eq(&current.entry, entry))
            .and_then(|current| current.heads.paused);

        if !refresh {
            self.header(&record, connection);
            self.overview.show(connection, &record);
            self.request.render(&record);
            self.response.render(&record);
        }

        let websocket = !record.frames.is_empty()
            || record
                .response
                .as_ref()
                .is_some_and(|response| response.status == 101);

        self.breakpoint.show(entry, &record);
        self.pages.breakpoint.set_visible(record.paused.is_some());
        self.pages.request.set_visible(record.request.is_some());
        self.pages.response.set_visible(record.response.is_some());
        self.pages.messages.set_visible(websocket);
        self.pages.stream.set_visible(record.tunnel.is_some());

        let heads = heads_of(&record);

        self.current.replace(Some(Current {
            entry: entry.clone(),
            heads: heads.clone(),
            contents: contents_of(&record),
            rendered: Instant::now(),
        }));

        drop(record);

        self.request.show(entry);
        self.response.show(entry);
        self.frames.show(entry, &entry.read());
        self.stream.show(entry);

        if heads.paused.is_some() && heads.paused != previous {
            self.stack.set_visible_child_name("breakpoint");
        }

        self.choose_page();
    }

    fn header(&self, record: &Record, connection: &session::Connection) {
        let (method, url) = match &record.request {
            Some(request) => (request.method.clone(), request.url()),
            None => (
                String::from("TUNNEL"),
                connection
                    .tls
                    .as_ref()
                    .and_then(|tls| tls.server_name.clone())
                    .unwrap_or_else(|| connection.server.to_string()),
            ),
        };

        let (badge, tone) = match (&record.state, &record.response) {
            (State::Failed(_), _) => (String::from("Failed"), "error"),
            (_, Some(response)) if response.status >= 400 => {
                (response.status.to_string(), "warning")
            }
            (_, Some(response)) => (response.status.to_string(), "success"),
            (State::Open, None) => (String::from("Pending"), ""),
            (State::Complete, None) => (String::from("Closed"), ""),
        };

        let url = format::printable(&url);

        self.method.set_text(&format::printable(&method));
        self.url.set_text(&url);
        self.url.set_tooltip_text(Some(&url));
        self.badge.set_text(&badge);
        self.badge.set_css_classes(&["status-badge", tone]);
    }

    fn choose_page(&self) {
        let current = self.stack.visible_child_name();
        let visible = |name: &str| {
            self.stack
                .child_by_name(name)
                .is_some_and(|child| self.stack.page(&child).is_visible())
        };

        if current.as_deref().is_some_and(visible) {
            return;
        }

        if let Some(name) = PREFERRED.into_iter().find(|name| visible(name)) {
            self.stack.set_visible_child_name(name);
        }
    }

    fn reset(&self) {
        self.current.replace(None);
        self.stale.set(false);
        self.breakpoint.clear();
        self.request.clear();
        self.response.clear();
        self.frames.clear();
        self.stream.clear();
    }
}

fn heads_of(record: &Record) -> Heads {
    Heads {
        state: record.state.clone(),
        paused: record.paused,
        request: record
            .request
            .as_ref()
            .map(|request| (request.headers.len(), request.trailers.len())),
        response: record.response.as_ref().map(|response| {
            (
                response.status,
                response.headers.len(),
                response.trailers.len(),
            )
        }),
        upgraded: record.tunnel.is_some(),
    }
}

fn contents_of(record: &Record) -> Contents {
    Contents {
        request: Part::Request.signature(record),
        response: Part::Response.signature(record),
        frames: record.frames.len(),
        tunnel: record
            .tunnel
            .as_ref()
            .map(|tunnel| (tunnel.sent_bytes, tunnel.received_bytes)),
    }
}
