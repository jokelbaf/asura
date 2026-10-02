use std::collections::HashSet;
use std::rc::Rc;
use std::sync::Arc;

use adw::prelude::*;
use gtk::glib;
use session::{Parts, Query, Search, Sides};

use super::keep;
use crate::service::Service;

const SIDES: [(&str, Sides); 3] = [
    ("Requests and responses", Sides::Both),
    ("Requests", Sides::Requests),
    ("Responses", Sides::Responses),
];

const PARTS: [(&str, Parts); 3] = [
    ("Headers and bodies", Parts::All),
    ("Headers and URLs", Parts::Headers),
    ("Bodies and messages", Parts::Bodies),
];

struct Finder {
    dialog: adw::Dialog,
    toasts: adw::ToastOverlay,
    text: adw::EntryRow,
    sides: adw::ComboRow,
    parts: adw::ComboRow,
    case: adw::SwitchRow,
    regex: adw::SwitchRow,
    search: gtk::Button,
    service: Arc<Service>,
    found: Box<dyn Fn(Query, HashSet<u64>)>,
}

pub fn present(
    parent: &impl IsA<gtk::Widget>,
    service: Arc<Service>,
    last: Option<Query>,
    found: impl Fn(Query, HashSet<u64>) + 'static,
) {
    let last = last.unwrap_or_default();

    let text = adw::EntryRow::builder()
        .title("Text")
        .text(&last.text)
        .build();

    let sides = adw::ComboRow::builder()
        .title("Look in")
        .model(&gtk::StringList::new(&SIDES.map(|(label, _)| label)))
        .selected(
            SIDES
                .iter()
                .position(|(_, sides)| *sides == last.sides)
                .unwrap_or_default() as u32,
        )
        .build();

    let parts = adw::ComboRow::builder()
        .title("Examine")
        .model(&gtk::StringList::new(&PARTS.map(|(label, _)| label)))
        .selected(
            PARTS
                .iter()
                .position(|(_, parts)| *parts == last.parts)
                .unwrap_or_default() as u32,
        )
        .build();

    let case = adw::SwitchRow::builder()
        .title("Match case")
        .active(last.case_sensitive)
        .build();
    let regex = adw::SwitchRow::builder()
        .title("Regular expression")
        .active(last.regex)
        .build();

    let query = adw::PreferencesGroup::builder()
        .description("Searches every captured request and response. Bodies are decompressed first, and the matching requests are shown in the list.")
        .build();
    query.add(&text);

    let options = adw::PreferencesGroup::builder().title("Options").build();
    options.add(&sides);
    options.add(&parts);
    options.add(&case);
    options.add(&regex);

    let page = adw::PreferencesPage::new();
    page.add(&query);
    page.add(&options);

    let search = gtk::Button::builder()
        .label("Search")
        .css_classes(["suggested-action"])
        .build();

    let header = adw::HeaderBar::new();
    header.pack_end(&search);

    let toasts = adw::ToastOverlay::new();
    toasts.set_child(Some(&page));

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&toasts));

    let dialog = adw::Dialog::builder()
        .title("Find in Session")
        .content_width(480)
        .content_height(520)
        .child(&toolbar)
        .build();

    let finder = Rc::new(Finder {
        dialog,
        toasts,
        text,
        sides,
        parts,
        case,
        regex,
        search,
        service,
        found: Box::new(found),
    });

    finder.search.connect_clicked(glib::clone!(
        #[weak]
        finder,
        move |_| finder.run()
    ));

    finder.text.connect_entry_activated(glib::clone!(
        #[weak]
        finder,
        move |_| finder.run()
    ));

    finder.dialog.present(Some(parent));
    finder.text.grab_focus();
    keep(&finder.dialog, finder.clone());
}

impl Finder {
    fn run(self: &Rc<Self>) {
        let query = Query {
            text: self.text.text().into(),
            regex: self.regex.is_active(),
            case_sensitive: self.case.is_active(),
            sides: SIDES[self.sides.selected() as usize % SIDES.len()].1,
            parts: PARTS[self.parts.selected() as usize % PARTS.len()].1,
        };

        if query.text.is_empty() {
            return self.toast("Enter the text to search for");
        }

        let search = match Search::new(&query) {
            Ok(search) => search,
            Err(error) => return self.toast(&format!("Invalid regular expression: {error}")),
        };

        let entries = self.service.store().entries();
        let finder = self.clone();
        self.search.set_sensitive(false);

        glib::spawn_future_local(async move {
            let found: HashSet<u64> = finder
                .service
                .background(move || {
                    entries
                        .iter()
                        .filter(|entry| search.matches(entry))
                        .map(|entry| entry.id())
                        .collect()
                })
                .await;

            (finder.found)(query, found);
            finder.dialog.close();
        });
    }

    fn toast(&self, message: &str) {
        self.toasts.add_toast(
            adw::Toast::builder()
                .use_markup(false)
                .title(message)
                .timeout(5)
                .build(),
        );
    }
}
