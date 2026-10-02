use std::cell::{Ref, RefCell};
use std::sync::Arc;

use gtk::glib;
use gtk::glib::prelude::*;
use gtk::glib::subclass::prelude::*;
use session::{Entry, Outcome, Summary};

use crate::ui::format;

mod imp {
    use std::cell::{Cell, OnceCell, RefCell};
    use std::sync::Arc;

    use gtk::glib;
    use gtk::glib::prelude::*;
    use gtk::glib::subclass::prelude::*;
    use session::{Entry, Summary};

    #[derive(Default, glib::Properties)]
    #[properties(wrapper_type = super::Exchange)]
    pub struct Exchange {
        #[property(get)]
        pub id: RefCell<u64>,
        #[property(get)]
        pub number: RefCell<String>,
        #[property(get)]
        pub icon: RefCell<String>,
        #[property(get)]
        pub tone: RefCell<String>,
        #[property(get)]
        pub status: RefCell<String>,
        #[property(get)]
        pub protocol: RefCell<String>,
        #[property(get)]
        pub method: RefCell<String>,
        #[property(get)]
        pub host: RefCell<String>,
        #[property(get)]
        pub path: RefCell<String>,
        #[property(get)]
        pub kind: RefCell<String>,
        #[property(get)]
        pub size: RefCell<u64>,
        #[property(get)]
        pub size_text: RefCell<String>,
        #[property(get)]
        pub duration: RefCell<i64>,
        #[property(get)]
        pub duration_text: RefCell<String>,
        #[property(get)]
        pub process: RefCell<String>,
        #[property(get)]
        pub started: RefCell<String>,
        pub entry: OnceCell<Arc<Entry>>,
        pub summary: RefCell<Option<Summary>>,
        pub visible: Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Exchange {
        const NAME: &'static str = "AsuraExchange";
        type Type = super::Exchange;
    }

    #[glib::derived_properties]
    impl ObjectImpl for Exchange {}
}

glib::wrapper! {
    pub struct Exchange(ObjectSubclass<imp::Exchange>);
}

impl Exchange {
    pub fn new(entry: Arc<Entry>) -> Self {
        let exchange: Self = glib::Object::new();
        let state = exchange.imp();

        state.id.replace(entry.id());
        state.number.replace(entry.id().to_string());
        let _ = state.entry.set(entry);

        exchange.refresh();
        exchange
    }

    pub fn entry(&self) -> &Arc<Entry> {
        self.imp().entry.get().expect("exchange without entry")
    }

    pub fn summary(&self) -> Ref<'_, Summary> {
        Ref::map(self.imp().summary.borrow(), |summary| {
            summary.as_ref().expect("exchange without summary")
        })
    }

    pub fn is_visible(&self) -> bool {
        self.imp().visible.get()
    }

    pub fn set_visible(&self, visible: bool) -> bool {
        self.imp().visible.replace(visible) != visible
    }

    pub fn refresh(&self) {
        let summary = self.entry().summary();
        let state = self.imp();

        let texts = [
            ("icon", &state.icon, icon(&summary).to_owned()),
            ("tone", &state.tone, tone(&summary).to_owned()),
            ("status", &state.status, status(&summary)),
            ("protocol", &state.protocol, summary.protocol.clone()),
            (
                "method",
                &state.method,
                format::printable(&summary.method).into_owned(),
            ),
            (
                "host",
                &state.host,
                format::printable(&summary.host).into_owned(),
            ),
            (
                "path",
                &state.path,
                format::printable(&summary.path).into_owned(),
            ),
            (
                "kind",
                &state.kind,
                format::printable(&summary.content_type).into_owned(),
            ),
            ("size-text", &state.size_text, format::size(summary.size)),
            (
                "duration-text",
                &state.duration_text,
                summary.duration.map(format::duration).unwrap_or_default(),
            ),
            ("process", &state.process, process(&summary)),
            ("started", &state.started, format::clock(summary.started)),
        ];

        let size = summary.size;
        let duration = summary
            .duration
            .map_or(-1, |duration| duration.whole_microseconds() as i64);

        state.summary.replace(Some(summary));

        for (name, cell, value) in texts {
            self.assign(name, cell, value);
        }

        self.assign("size", &state.size, size);
        self.assign("duration", &state.duration, duration);
    }

    fn assign<T: PartialEq>(&self, name: &str, cell: &RefCell<T>, value: T) {
        if *cell.borrow() != value {
            cell.replace(value);
            self.notify(name);
        }
    }
}

fn icon(summary: &Summary) -> &'static str {
    match &summary.outcome {
        _ if summary.paused => "media-playback-pause-symbolic",
        Outcome::Failed(_) => "dialog-error-symbolic",
        _ if summary.mocked => "mail-replied-symbolic",
        _ if summary.edited => "document-edit-symbolic",
        _ if summary.composed => "mail-send-symbolic",
        _ if summary.websocket => "network-transmit-receive-symbolic",
        _ if !summary.decrypted => "channel-secure-symbolic",
        _ if summary.tunnel => "network-wired-symbolic",
        Outcome::Pending => "content-loading-symbolic",
        Outcome::Complete if summary.content_type.starts_with("image/") => {
            "image-x-generic-symbolic"
        }
        Outcome::Complete => "text-x-generic-symbolic",
    }
}

fn tone(summary: &Summary) -> &'static str {
    match (&summary.outcome, summary.status) {
        _ if summary.paused => "accent",
        (Outcome::Failed(_), _) => "error",
        (_, Some(status)) if status >= 400 => "warning",
        (Outcome::Pending, _) => "dimmed",
        _ if !summary.decrypted => "dimmed",
        _ => "",
    }
}

fn status(summary: &Summary) -> String {
    match (&summary.outcome, summary.status) {
        _ if summary.paused => String::from("Paused"),
        (_, Some(status)) => status.to_string(),
        (Outcome::Failed(_), None) => String::from("Failed"),
        (Outcome::Pending, None) => String::from("…"),
        (Outcome::Complete, None) => String::from("—"),
    }
}

fn process(summary: &Summary) -> String {
    match summary.pid {
        0 => summary.process.clone(),
        pid => format!("{}:{pid}", summary.process),
    }
}
