mod capture;
mod decryption;
mod editor;
mod prefixes;

use std::sync::Arc;

use adw::prelude::*;

use crate::service::Service;

pub use prefixes::prefixes;

pub fn present(parent: &impl IsA<gtk::Widget>, service: &Arc<Service>) -> adw::PreferencesDialog {
    let dialog = adw::PreferencesDialog::builder()
        .search_enabled(true)
        .build();

    dialog.add(&capture::page(&dialog, service));
    dialog.add(&decryption::page(&dialog, service));
    dialog.add(&prefixes::page(&dialog, service));

    dialog.present(Some(parent));
    dialog
}

pub fn present_decryption(parent: &impl IsA<gtk::Widget>, service: &Arc<Service>) {
    present(parent, service).set_visible_page_name(decryption::NAME);
}
