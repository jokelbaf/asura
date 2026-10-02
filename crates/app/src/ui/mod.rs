mod attach;
mod certificate;
mod chrome;
mod completion;
mod compose;
mod curl;
mod editor;
mod files;
mod format;
mod hints;
mod inspector;
mod launch;
mod preferences;
mod rules;
mod search;
mod sessions;
mod targets;
mod window;

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;
use gtk::{gdk, glib};
use sourceview::prelude::*;

pub use preferences::present as preferences;
pub use window::Window;

const STYLE: &str = include_str!("style.css");
const COPY: &str = "edit-copy-symbolic";
const COPIED: &str = "object-select-symbolic";
const FEEDBACK: Duration = Duration::from_millis(1400);

pub fn load_style() {
    let provider = gtk::CssProvider::new();
    provider.load_from_string(STYLE);

    if let Some(display) = gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

pub fn notify(widget: &impl IsA<gtk::Widget>, message: &str) {
    present(
        widget,
        adw::Toast::builder()
            .use_markup(false)
            .title(message)
            .timeout(5)
            .build(),
    );
}

pub fn offer(widget: &impl IsA<gtk::Widget>, message: &str, label: &str, action: &str) {
    present(
        widget,
        adw::Toast::builder()
            .use_markup(false)
            .title(message)
            .button_label(label)
            .action_name(action)
            .timeout(8)
            .build(),
    );
}

fn present(widget: &impl IsA<gtk::Widget>, toast: adw::Toast) {
    if let Some(dialog) = widget.ancestor(adw::PreferencesDialog::static_type()) {
        return dialog
            .downcast::<adw::PreferencesDialog>()
            .expect("preferences dialog")
            .add_toast(toast);
    }

    let overlay = widget
        .root()
        .and_downcast::<adw::ApplicationWindow>()
        .and_then(|window| window.content())
        .and_downcast::<adw::ToastOverlay>();

    match overlay {
        Some(overlay) => overlay.add_toast(toast),
        None => eprintln!("asura: {}", toast.title().unwrap_or_default()),
    }
}

pub fn row(title: &str, subtitle: &str) -> adw::ActionRow {
    let value = format::printable(subtitle).into_owned();

    let row = adw::ActionRow::builder()
        .use_markup(false)
        .subtitle_selectable(true)
        .css_classes(["property"])
        .build();
    row.set_title(&format::printable(title));
    row.set_subtitle(&value);

    row.add_suffix(&copy_button("Copy", move || value.clone()));
    row
}

pub fn copy_button(tooltip: &str, text: impl Fn() -> String + 'static) -> gtk::Button {
    let button = gtk::Button::builder()
        .icon_name(COPY)
        .tooltip_text(tooltip)
        .valign(gtk::Align::Center)
        .css_classes(["flat", "copy-button"])
        .build();

    button.connect_clicked(move |button| {
        button.clipboard().set_text(&text());
        copied(button);
    });

    button
}

pub fn copied(button: &gtk::Button) {
    button.set_icon_name(COPIED);
    button.add_css_class("copied");

    let weak = button.downgrade();
    glib::timeout_add_local_once(FEEDBACK, move || {
        if let Some(button) = weak.upgrade() {
            button.set_icon_name(COPY);
            button.remove_css_class("copied");
        }
    });
}

pub fn clear(container: &impl IsA<gtk::Widget>) {
    while let Some(child) = container.first_child() {
        child.unparent();
    }
}

pub fn scheme(buffer: &sourceview::Buffer) {
    let manager = adw::StyleManager::default();

    let apply = glib::clone!(
        #[weak]
        buffer,
        move |manager: &adw::StyleManager| {
            let name = if manager.is_dark() {
                "Adwaita-dark"
            } else {
                "Adwaita"
            };
            buffer.set_style_scheme(
                sourceview::StyleSchemeManager::default()
                    .scheme(name)
                    .as_ref(),
            );
        }
    );

    apply(&manager);
    manager.connect_dark_notify(apply);
}

pub fn keep<T: 'static>(dialog: &impl IsA<adw::Dialog>, value: Rc<T>) {
    let holder = RefCell::new(Some(value));
    dialog.as_ref().connect_closed(move |_| drop(holder.take()));
}
