use std::path::PathBuf;

use gtk::prelude::*;
use gtk::{gio, glib};

pub struct Kind {
    pub name: &'static str,
    pub patterns: &'static [&'static str],
}

pub const CAPTURES: Kind = Kind {
    name: "Sessions and HTTP archives",
    patterns: &["*.asura", "*.har"],
};

pub const SESSIONS: Kind = Kind {
    name: "Asura sessions",
    patterns: &["*.asura"],
};

pub const HAR: Kind = Kind {
    name: "HTTP archives",
    patterns: &["*.har"],
};

pub const CERTIFICATES: Kind = Kind {
    name: "Certificates",
    patterns: &["*.pem", "*.crt", "*.cer"],
};

pub const ANY: Kind = Kind {
    name: "All files",
    patterns: &["*"],
};

pub async fn open(parent: &impl IsA<gtk::Widget>, title: &str, kinds: &[&Kind]) -> Option<PathBuf> {
    dialog(title, kinds)
        .open_future(window(parent).as_ref())
        .await
        .ok()?
        .path()
}

pub async fn save(
    parent: &impl IsA<gtk::Widget>,
    title: &str,
    name: &str,
    kinds: &[&Kind],
) -> Option<PathBuf> {
    let dialog = dialog(title, kinds);
    dialog.set_initial_name(Some(name));
    dialog
        .save_future(window(parent).as_ref())
        .await
        .ok()?
        .path()
}

pub async fn folder(parent: &impl IsA<gtk::Widget>, title: &str) -> Option<PathBuf> {
    dialog(title, &[])
        .select_folder_future(window(parent).as_ref())
        .await
        .ok()?
        .path()
}

fn dialog(title: &str, kinds: &[&Kind]) -> gtk::FileDialog {
    let filters = gio::ListStore::new::<gtk::FileFilter>();

    for kind in kinds {
        let filter = gtk::FileFilter::new();
        filter.set_name(Some(kind.name));

        for pattern in kind.patterns {
            filter.add_pattern(pattern);
        }

        filters.append(&filter);
    }

    let dialog = gtk::FileDialog::builder().title(title).modal(true).build();

    if filters.n_items() > 0 {
        dialog.set_filters(Some(&filters));
    }

    dialog
}

fn window(widget: &impl IsA<gtk::Widget>) -> Option<gtk::Window> {
    widget.root().and_downcast::<gtk::Window>()
}

pub fn stamp() -> String {
    glib::DateTime::now_local()
        .and_then(|now| now.format("%Y-%m-%d_%H-%M-%S"))
        .map(Into::into)
        .unwrap_or_default()
}
