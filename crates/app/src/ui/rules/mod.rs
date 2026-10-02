mod breakpoint;
mod form;
mod list;
mod responder;

use std::rc::Rc;
use std::sync::Arc;

use adw::prelude::*;
use gtk::glib;
use proxy::{Breakpoint, Responder};

use self::list::{List, Rule};
use crate::service::Service;
use crate::ui::{hints, notify};

const SYNTAX: [(&str, &str); 5] = [
    ("example.com/api", "The URL contains the text"),
    ("https://example.com/api", "The URL starts with the text"),
    (
        "https://*.example.com/*.png",
        "Wildcards; the whole URL must match",
    ),
    ("exact:https://example.com/", "The URL equals the text"),
    ("regex:^https://.+\\.json$", "Regular expression"),
];

pub fn present(parent: &impl IsA<gtk::Widget>, service: &Arc<Service>) {
    let dialog = adw::PreferencesDialog::builder()
        .title("Rules")
        .search_enabled(false)
        .build();

    dialog.add(&page::<Responder>(
        &dialog,
        service,
        "Auto Responder",
        "mail-replied-symbolic",
        "Matching requests are answered by Asura and never reach the server. The first matching rule wins.",
    ));

    dialog.add(&page::<Breakpoint>(
        &dialog,
        service,
        "Breakpoints",
        "media-playback-pause-symbolic",
        "Matching requests pause so you can edit, resume or abort them.",
    ));

    dialog.present(Some(parent));
}

pub fn respond(parent: &impl IsA<gtk::Widget>, service: &Arc<Service>, rule: Responder) {
    add(parent, service, rule);
}

pub fn pause(parent: &impl IsA<gtk::Widget>, service: &Arc<Service>, rule: Breakpoint) {
    add(parent, service, rule);
}

fn add<T: Rule>(parent: &impl IsA<gtk::Widget>, service: &Arc<Service>, rule: T) {
    let shared = service.clone();
    let anchor = parent.as_ref().clone();

    T::edit(
        parent.upcast_ref(),
        Some(rule),
        hints::snapshot(service.store()),
        Rc::new(move |rule| {
            match shared.update(|settings| {
                let mut rules = T::all(settings);
                rules.push(rule);
                T::store(settings, rules);
            }) {
                Ok(()) => notify(&anchor, T::ADDED),
                Err(error) => notify(&anchor, &format!("{error:#}")),
            }
        }),
    );
}

fn page<T: Rule>(
    dialog: &adw::PreferencesDialog,
    service: &Arc<Service>,
    title: &str,
    icon: &str,
    description: &str,
) -> adw::PreferencesPage {
    let page = adw::PreferencesPage::builder()
        .title(title)
        .icon_name(icon)
        .build();

    let rules = adw::PreferencesGroup::builder()
        .title("Rules")
        .description(description)
        .build();
    List::<T>::attach(dialog, &rules, service.clone());

    let help = adw::PreferencesGroup::builder()
        .title("URL Patterns")
        .description("Patterns are matched against the full URL, ignoring case. Rules apply to connections opened after they are enabled.")
        .build();

    for (example, meaning) in SYNTAX {
        help.add(
            &adw::ActionRow::builder()
                .title(format!("<tt>{}</tt>", glib::markup_escape_text(example)))
                .subtitle(meaning)
                .title_selectable(true)
                .build(),
        );
    }

    page.add(&rules);
    page.add(&help);
    page
}
