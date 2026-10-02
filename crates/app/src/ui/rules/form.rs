use std::rc::Rc;

use adw::prelude::*;
use proxy::Pattern;

use crate::ui::completion::Completion;
use crate::ui::hints::{self, Catalog};

pub struct Matcher {
    pub group: adw::PreferencesGroup,
    pub pattern: adw::EntryRow,
    pub method: adw::EntryRow,
}

impl Matcher {
    pub fn new(pattern: &str, method: &str, catalog: Rc<Catalog>) -> Self {
        let pattern = adw::EntryRow::builder()
            .title("URL pattern")
            .text(pattern)
            .build();
        let method = adw::EntryRow::builder()
            .title("Method (empty matches any)")
            .text(method)
            .build();

        Completion::attach(&pattern, hints::pattern(catalog.clone()));
        Completion::attach(&method, hints::method(catalog));

        let group = adw::PreferencesGroup::builder().title("Match").build();
        group.add(&pattern);
        group.add(&method);

        Self {
            group,
            pattern,
            method,
        }
    }

    pub fn valid(&self) -> bool {
        let text = self.pattern.text();

        match Pattern::parse(&text) {
            Ok(_) => {
                self.pattern.remove_css_class("error");
                self.pattern.set_tooltip_text(None);
                true
            }
            Err(error) => {
                if !text.trim().is_empty() {
                    self.pattern.add_css_class("error");
                    self.pattern.set_tooltip_text(Some(&error));
                }
                false
            }
        }
    }

    pub fn pattern(&self) -> String {
        self.pattern.text().trim().to_owned()
    }

    pub fn method(&self) -> String {
        self.method.text().trim().to_ascii_uppercase()
    }

    pub fn connect_changed(&self, callback: impl Fn() + Clone + 'static) {
        let changed = callback.clone();
        self.pattern.connect_changed(move |_| changed());
        self.method.connect_changed(move |_| callback());
    }
}

pub fn dialog(title: &str, height: i32, page: &adw::PreferencesPage) -> (adw::Dialog, gtk::Button) {
    let save = gtk::Button::builder()
        .label("Save")
        .sensitive(false)
        .css_classes(["suggested-action"])
        .build();

    let header = adw::HeaderBar::new();
    header.pack_end(&save);

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(page));

    let dialog = adw::Dialog::builder()
        .title(title)
        .content_width(560)
        .content_height(height)
        .child(&toolbar)
        .build();

    (dialog, save)
}

pub fn method(method: &str) -> &str {
    if method.is_empty() {
        "Any method"
    } else {
        method
    }
}
