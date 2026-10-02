use std::borrow::Cow;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use adw::prelude::*;
use gtk::{gdk, gio, glib, pango};
use session::{Entry, Query};
use sourceview::prelude::*;

use super::content::{self, Content, Part, Signature, TEXT_LIMIT, View};
use super::finder::Finder;
use super::table::TableView;
use crate::ui::{copied, files, format, notify, scheme};

const HEX_LIMIT: usize = 256 << 10;
const MODES: [(&str, &str); 4] = [
    ("text", "Text"),
    ("hex", "Hex"),
    ("image", "Image"),
    ("structured", "Structured"),
];

struct Shown {
    entry: Arc<Entry>,
    part: Part,
    signature: Option<Signature>,
}

pub struct BodyView {
    pub widget: gtk::Box,
    info: gtk::Label,
    modes: adw::ToggleGroup,
    pretty: gtk::ToggleButton,
    banner: adw::Banner,
    stack: gtk::Stack,
    view: sourceview::View,
    buffer: sourceview::Buffer,
    picture: gtk::Picture,
    table: Rc<TableView>,
    finder: Rc<Finder>,
    empty: adw::StatusPage,
    copy: gtk::Button,
    save: gtk::Button,
    shown: RefCell<Option<Shown>>,
    content: RefCell<Option<Arc<Content>>>,
    epoch: Cell<u64>,
    quiet: Cell<bool>,
}

impl BodyView {
    pub fn new(title: &str) -> Rc<Self> {
        let buffer = sourceview::Buffer::new(None);
        buffer.set_highlight_matching_brackets(false);
        scheme(&buffer);

        let view = sourceview::View::builder()
            .buffer(&buffer)
            .editable(false)
            .monospace(true)
            .show_line_numbers(true)
            .wrap_mode(gtk::WrapMode::WordChar)
            .top_margin(6)
            .bottom_margin(6)
            .left_margin(6)
            .right_margin(6)
            .css_classes(["body-view"])
            .build();

        let picture = gtk::Picture::builder()
            .content_fit(gtk::ContentFit::ScaleDown)
            .can_shrink(true)
            .margin_top(12)
            .margin_bottom(12)
            .build();

        let empty = adw::StatusPage::builder()
            .icon_name("text-x-generic-symbolic")
            .title("No Body")
            .css_classes(["compact"])
            .build();

        let table = TableView::new();
        let finder = Finder::new(&view, &buffer);

        let stack = gtk::Stack::builder()
            .transition_type(gtk::StackTransitionType::Crossfade)
            .transition_duration(120)
            .build();
        stack.add_named(
            &gtk::ScrolledWindow::builder()
                .child(&view)
                .vexpand(true)
                .build(),
            Some("text"),
        );
        stack.add_named(
            &gtk::ScrolledWindow::builder()
                .child(&picture)
                .vexpand(true)
                .build(),
            Some("image"),
        );
        stack.add_named(&table.widget, Some("table"));
        stack.add_named(&empty, Some("empty"));
        stack.set_visible_child_name("empty");

        let modes = adw::ToggleGroup::builder()
            .can_shrink(true)
            .css_classes(["flat", "round"])
            .build();
        for (name, label) in MODES {
            modes.add(adw::Toggle::builder().name(name).label(label).build());
        }
        modes.set_active_name(Some("text"));

        let info = gtk::Label::builder()
            .hexpand(true)
            .xalign(1.0)
            .ellipsize(pango::EllipsizeMode::Start)
            .css_classes(["dimmed", "caption"])
            .build();

        let pretty = gtk::ToggleButton::builder()
            .icon_name("format-justify-left-symbolic")
            .tooltip_text("Pretty Print")
            .active(true)
            .visible(false)
            .css_classes(["flat"])
            .build();

        let search = gtk::Button::builder()
            .icon_name("edit-find-symbolic")
            .tooltip_text("Find in Body (Ctrl+F)")
            .css_classes(["flat"])
            .build();

        let copy = gtk::Button::builder()
            .icon_name("edit-copy-symbolic")
            .tooltip_text("Copy Body")
            .css_classes(["flat", "copy-button"])
            .sensitive(false)
            .build();

        let save = gtk::Button::builder()
            .icon_name("folder-download-symbolic")
            .tooltip_text("Save Body…")
            .css_classes(["flat"])
            .sensitive(false)
            .build();

        let toolbar = gtk::Box::builder()
            .spacing(4)
            .margin_start(6)
            .margin_end(6)
            .margin_top(4)
            .margin_bottom(4)
            .css_classes(["body-toolbar"])
            .build();

        if !title.is_empty() {
            toolbar.append(
                &gtk::Label::builder()
                    .label(title)
                    .css_classes(["heading"])
                    .build(),
            );
        }

        toolbar.append(&modes);
        toolbar.append(&info);
        toolbar.append(&pretty);
        toolbar.append(&search);
        toolbar.append(&copy);
        toolbar.append(&save);

        let banner = adw::Banner::new("");

        let widget = gtk::Box::new(gtk::Orientation::Vertical, 0);
        widget.append(&toolbar);
        widget.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        widget.append(&banner);
        widget.append(&finder.bar);
        widget.append(&stack);

        let body = Rc::new(Self {
            widget,
            info,
            modes,
            pretty,
            banner,
            stack,
            view,
            buffer,
            picture,
            table,
            finder,
            empty,
            copy,
            save,
            shown: RefCell::default(),
            content: RefCell::default(),
            epoch: Cell::new(0),
            quiet: Cell::new(false),
        });

        let weak = Rc::downgrade(&body);
        body.modes.connect_active_name_notify(move |_| {
            if let Some(body) = weak.upgrade().filter(|body| !body.quiet.get()) {
                body.render();
            }
        });

        let weak = Rc::downgrade(&body);
        body.pretty.connect_toggled(move |_| {
            if let Some(body) = weak.upgrade() {
                body.render();
            }
        });

        let weak = Rc::downgrade(&body);
        search.connect_clicked(move |_| {
            if let Some(body) = weak.upgrade() {
                body.find();
            }
        });

        let weak = Rc::downgrade(&body);
        body.copy.connect_clicked(move |_| {
            if let Some(body) = weak.upgrade() {
                body.copy();
            }
        });

        let weak = Rc::downgrade(&body);
        body.save.connect_clicked(move |_| {
            if let Some(body) = weak.upgrade() {
                body.export();
            }
        });

        body
    }

    pub fn show(self: &Rc<Self>, entry: &Arc<Entry>, part: Part) {
        let signature = part.signature(&entry.read());

        let (same_part, unchanged) = match &*self.shown.borrow() {
            Some(shown) if Arc::ptr_eq(&shown.entry, entry) && shown.part == part => {
                (true, shown.signature == signature)
            }
            _ => (false, false),
        };

        if unchanged {
            return;
        }

        self.shown.replace(Some(Shown {
            entry: entry.clone(),
            part,
            signature,
        }));

        let epoch = self.epoch.get() + 1;
        self.epoch.set(epoch);

        if signature.is_none() {
            self.content.replace(None);
            self.adapt(None);
            return self.render();
        }

        let weak = Rc::downgrade(self);
        let entry = entry.clone();

        glib::spawn_future_local(async move {
            let content = gio::spawn_blocking(move || content::load(&entry, part))
                .await
                .ok()
                .flatten();

            let Some(body) = weak.upgrade().filter(|body| body.epoch.get() == epoch) else {
                return;
            };

            let mode = (!same_part).then(|| content.as_ref().map_or("text", automatic));
            body.content.replace(content.map(Arc::new));
            body.adapt(mode);
            body.render();
        });
    }

    pub fn clear(&self) {
        self.shown.replace(None);
        self.content.replace(None);
        self.epoch.set(self.epoch.get() + 1);
        self.buffer.set_text("");
        self.table.clear();
        self.picture.set_paintable(None::<&gdk::Paintable>);
    }

    pub fn contains(&self, widget: &gtk::Widget) -> bool {
        widget == self.widget.upcast_ref::<gtk::Widget>() || widget.is_ancestor(&self.widget)
    }

    pub fn find(&self) {
        if self.stack.visible_child_name().as_deref() != Some("text") {
            self.modes.set_active_name(Some("text"));
        }

        self.finder.open();
    }

    pub fn highlight(&self, query: Option<&Query>) {
        match query {
            Some(query) => self
                .finder
                .query(&query.text, query.regex, query.case_sensitive),
            None => self.finder.close(),
        }
    }

    fn adapt(&self, mode: Option<&str>) {
        let content = self.content.borrow();
        let image = content
            .as_ref()
            .is_some_and(|content| content::is_image(&content.kind));
        let structure = content
            .as_ref()
            .and_then(|content| content.structure.as_ref());

        let active = self.modes.active_name();
        let unavailable = match active.as_deref() {
            Some("image") => !image,
            Some("structured") => structure.is_none(),
            _ => false,
        };

        self.quiet.set(true);
        self.offer(image, structure.is_some());

        if let Some(toggle) = self.modes.toggle_by_name("structured") {
            toggle.set_label(Some(
                structure.map_or("Structured", |structure| structure.label),
            ));
        }

        match mode {
            Some(mode) => self.modes.set_active_name(Some(mode)),
            None if unavailable => self.modes.set_active_name(Some("text")),
            None => self.modes.set_active_name(active.as_deref()),
        }

        self.quiet.set(false);
    }

    fn offer(&self, image: bool, structured: bool) {
        let wanted: Vec<(&str, &str)> = MODES
            .into_iter()
            .filter(|(name, _)| match *name {
                "image" => image,
                "structured" => structured,
                _ => true,
            })
            .collect();

        let current: Vec<glib::GString> = (0..self.modes.n_toggles())
            .filter_map(|index| self.modes.toggle(index))
            .map(|toggle| toggle.name())
            .collect();

        if current
            .iter()
            .map(|name| name.as_str())
            .eq(wanted.iter().map(|(name, _)| *name))
        {
            return;
        }

        self.modes.remove_all();

        for (name, label) in wanted {
            self.modes
                .add(adw::Toggle::builder().name(name).label(label).build());
        }
    }

    fn render(&self) {
        let content = self.content.borrow().clone();
        self.banner.set_revealed(false);
        self.pretty.set_visible(false);

        let Some(content) = content else {
            self.info.set_text("");
            self.copy.set_sensitive(false);
            self.save.set_sensitive(false);
            self.empty.set_title("No Body");
            return self.stack.set_visible_child_name("empty");
        };

        self.info.set_text(&format::printable(&info(&content)));
        self.copy.set_sensitive(!content.bytes.is_empty());
        self.save.set_sensitive(true);

        if content.bytes.is_empty() {
            self.empty.set_title("Empty Body");
            return self.stack.set_visible_child_name("empty");
        }

        if let Some(problem) = &content.problem {
            self.warn(&format!(
                "Could not decode the body ({problem}); showing it as received"
            ));
        }

        let structure = content.structure.as_ref().map(|structure| &structure.view);

        match (self.modes.active_name().as_deref(), structure) {
            (Some("image"), _) if content::is_image(&content.kind) => self.image(&content),
            (Some("hex"), _) => self.hex(&content),
            (Some("structured"), Some(View::Table(_))) => {
                self.table.show(&content);
                self.stack.set_visible_child_name("table");
            }
            (Some("structured"), Some(View::Tree(tree))) => {
                self.display(tree.trim_end(), None, gtk::WrapMode::WordChar)
            }
            _ => self.text(&content),
        }
    }

    fn text(&self, content: &Content) {
        let shown = &content.bytes[..content.bytes.len().min(TEXT_LIMIT)];

        let text = match &content.pretty {
            Some(pretty) if self.pretty.is_active() => Cow::Borrowed(pretty.as_str()),
            _ => String::from_utf8_lossy(shown),
        };

        self.pretty.set_visible(content.pretty.is_some());
        self.truncated(shown.len(), content.total);
        self.display(&text, content.language, gtk::WrapMode::WordChar);
    }

    fn hex(&self, content: &Content) {
        let shown = &content.bytes[..content.bytes.len().min(HEX_LIMIT)];

        self.truncated(shown.len(), content.total);
        self.display(
            formats::hexdump(shown).trim_end(),
            None,
            gtk::WrapMode::None,
        );
    }

    fn display(&self, text: &str, language: Option<&str>, wrap: gtk::WrapMode) {
        self.view.set_wrap_mode(wrap);
        self.buffer.set_language(
            language
                .and_then(|id| sourceview::LanguageManager::default().language(id))
                .as_ref(),
        );
        self.buffer.set_text(&format::printable(text));
        self.stack.set_visible_child_name("text");
    }

    fn image(&self, content: &Content) {
        match gdk::Texture::from_bytes(&glib::Bytes::from(&content.bytes)) {
            Ok(texture) => {
                self.picture.set_paintable(Some(&texture));
                self.stack.set_visible_child_name("image");
            }
            Err(_) => {
                self.warn("The image could not be decoded");
                self.hex(content);
            }
        }
    }

    fn truncated(&self, shown: usize, total: u64) {
        if (shown as u64) < total {
            self.warn(&format!(
                "Showing the first {} of {}; save the body to see all of it",
                format::size(shown as u64),
                format::size(total)
            ));
        }
    }

    fn warn(&self, message: &str) {
        self.banner.set_title(message);
        self.banner.set_revealed(true);
    }

    fn copy(&self) {
        let clipboard = self.widget.clipboard();

        match self.stack.visible_child_name().as_deref() {
            Some("image") => {
                if let Some(texture) = self.picture.paintable().and_downcast::<gdk::Texture>() {
                    clipboard.set_texture(&texture);
                }
            }
            Some("text") => {
                let (start, end) = self.buffer.bounds();
                clipboard.set_text(&self.buffer.text(&start, &end, true));
            }
            _ => {
                if let Some(content) = self.content.borrow().as_ref() {
                    let shown = &content.bytes[..content.bytes.len().min(TEXT_LIMIT)];
                    clipboard.set_text(&format::printable(&String::from_utf8_lossy(shown)));
                }
            }
        }

        copied(&self.copy);
    }

    fn export(&self) {
        let Some((entry, part)) = self
            .shown
            .borrow()
            .as_ref()
            .map(|shown| (shown.entry.clone(), shown.part))
        else {
            return;
        };

        let widget = self.widget.clone();
        let name = content::file_name(&entry, part);

        glib::spawn_future_local(async move {
            let Some(path) = files::save(&widget, "Save Body", &name, &[]).await else {
                return;
            };

            let saved = gio::spawn_blocking(move || content::export(&entry, part, &path)).await;

            if let Ok(Err(error)) = saved {
                notify(&widget, &format!("Could not save the body: {error}"));
            }
        });
    }
}

fn automatic(content: &Content) -> &'static str {
    match () {
        _ if content
            .structure
            .as_ref()
            .is_some_and(|structure| structure.preferred) =>
        {
            "structured"
        }
        _ if content::is_image(&content.kind) => "image",
        _ if content.textual => "text",
        _ => "hex",
    }
}

fn info(content: &Content) -> String {
    let mut parts = vec![format::size(content.total)];

    if let Some(essence) = content
        .kind
        .split(';')
        .next()
        .map(str::trim)
        .filter(|essence| !essence.is_empty())
    {
        parts.push(essence.to_owned());
    }

    if let Some(encoding) = &content.encoding {
        parts.push(format!(
            "{encoding}, {} transferred",
            format::size(content.wire)
        ));
    }

    parts.join(" · ")
}
