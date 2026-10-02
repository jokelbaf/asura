use std::rc::Rc;

use gtk::prelude::*;
use sourceview::prelude::*;

pub struct Finder {
    pub bar: gtk::SearchBar,
    entry: gtk::SearchEntry,
    count: gtk::Label,
    view: sourceview::View,
    settings: sourceview::SearchSettings,
    context: sourceview::SearchContext,
}

impl Finder {
    pub fn new(view: &sourceview::View, buffer: &sourceview::Buffer) -> Rc<Self> {
        let settings = sourceview::SearchSettings::new();
        settings.set_wrap_around(true);

        let context = sourceview::SearchContext::new(buffer, Some(&settings));
        context.set_highlight(true);

        let entry = gtk::SearchEntry::builder()
            .placeholder_text("Find in body")
            .hexpand(true)
            .build();

        let count = gtk::Label::builder()
            .width_chars(12)
            .xalign(1.0)
            .css_classes(["dimmed", "caption", "numeric"])
            .build();

        let previous = gtk::Button::builder()
            .icon_name("go-up-symbolic")
            .tooltip_text("Previous Match (Shift+Enter)")
            .css_classes(["flat"])
            .build();

        let next = gtk::Button::builder()
            .icon_name("go-down-symbolic")
            .tooltip_text("Next Match (Enter)")
            .css_classes(["flat"])
            .build();

        let content = gtk::Box::builder().spacing(6).build();
        content.append(&entry);
        content.append(&count);
        content.append(&previous);
        content.append(&next);

        let bar = gtk::SearchBar::builder()
            .child(&content)
            .show_close_button(true)
            .build();
        bar.connect_entry(&entry);

        let finder = Rc::new(Self {
            bar,
            entry,
            count,
            view: view.clone(),
            settings,
            context,
        });

        let weak = Rc::downgrade(&finder);
        finder.entry.connect_search_changed(move |entry| {
            if let Some(finder) = weak.upgrade() {
                let text = entry.text();
                finder
                    .settings
                    .set_search_text((!text.is_empty()).then_some(text.as_str()));
            }
        });

        let forward = |finder: &Rc<Self>| {
            let weak = Rc::downgrade(finder);
            move || {
                if let Some(finder) = weak.upgrade() {
                    finder.next();
                }
            }
        };

        let backward = |finder: &Rc<Self>| {
            let weak = Rc::downgrade(finder);
            move || {
                if let Some(finder) = weak.upgrade() {
                    finder.previous();
                }
            }
        };

        let step = forward(&finder);
        finder.entry.connect_activate(move |_| step());
        let step = forward(&finder);
        finder.entry.connect_next_match(move |_| step());
        let step = forward(&finder);
        next.connect_clicked(move |_| step());

        let step = backward(&finder);
        finder.entry.connect_previous_match(move |_| step());
        let step = backward(&finder);
        previous.connect_clicked(move |_| step());

        let step = backward(&finder);
        let shortcuts = gtk::ShortcutController::new();
        shortcuts.add_shortcut(gtk::Shortcut::new(
            gtk::ShortcutTrigger::parse_string("<Shift>Return"),
            Some(gtk::CallbackAction::new(move |_, _| {
                step();
                gtk::glib::Propagation::Stop
            })),
        ));
        finder.entry.add_controller(shortcuts);

        let weak = Rc::downgrade(&finder);
        finder.context.connect_occurrences_count_notify(move |_| {
            if let Some(finder) = weak.upgrade() {
                finder.count();
            }
        });

        let weak = Rc::downgrade(&finder);
        finder.bar.connect_search_mode_enabled_notify(move |bar| {
            if let Some(finder) = weak.upgrade().filter(|_| !bar.is_search_mode()) {
                finder.settings.set_regex_enabled(false);
                finder.settings.set_case_sensitive(false);
                finder.entry.set_text("");
            }
        });

        finder
    }

    pub fn open(&self) {
        self.bar.set_search_mode(true);
        self.entry.grab_focus();
        self.entry.select_region(0, -1);
    }

    pub fn query(&self, text: &str, regex: bool, case_sensitive: bool) {
        self.bar.set_search_mode(true);
        self.settings.set_regex_enabled(regex);
        self.settings.set_case_sensitive(case_sensitive);
        self.entry.set_text(text);
    }

    pub fn close(&self) {
        self.bar.set_search_mode(false);
    }

    fn next(&self) {
        let buffer = self.view.buffer();
        let from = buffer
            .selection_bounds()
            .map(|(_, end)| end)
            .unwrap_or_else(|| buffer.iter_at_mark(&buffer.get_insert()));

        if let Some((start, end, _)) = self.context.forward(&from) {
            self.select(start, end);
        }
    }

    fn previous(&self) {
        let buffer = self.view.buffer();
        let from = buffer
            .selection_bounds()
            .map(|(start, _)| start)
            .unwrap_or_else(|| buffer.iter_at_mark(&buffer.get_insert()));

        if let Some((start, end, _)) = self.context.backward(&from) {
            self.select(start, end);
        }
    }

    fn select(&self, mut start: gtk::TextIter, end: gtk::TextIter) {
        self.view.buffer().select_range(&start, &end);
        self.view.scroll_to_iter(&mut start, 0.2, false, 0.0, 0.0);
        self.count();
    }

    fn count(&self) {
        let total = self.context.occurrences_count();
        let searching = self.settings.search_text().is_some();

        let position = self
            .view
            .buffer()
            .selection_bounds()
            .map(|(start, end)| self.context.occurrence_position(&start, &end))
            .filter(|position| *position > 0);

        let text = match (total, position) {
            (0, _) if searching => String::from("No matches"),
            (..1, _) => String::new(),
            (1, _) => String::from("1 match"),
            (total, Some(position)) => format!("{position} of {total}"),
            (total, None) => format!("{total} matches"),
        };

        self.count.set_text(&text);
    }
}
