use std::sync::Arc;

use adw::prelude::*;

use super::editor::Editor;
use crate::service::{Service, network};
use crate::ui::{notify, row};

pub fn page(dialog: &adw::PreferencesDialog, service: &Arc<Service>) -> adw::PreferencesPage {
    let settings = service.settings();
    let page = adw::PreferencesPage::builder()
        .title("Capture")
        .icon_name("network-wired-symbolic")
        .build();

    let quic = adw::SwitchRow::builder()
        .title("Block QUIC")
        .subtitle(
            "Reject HTTP/3 so applications fall back to HTTPS over TCP, which Asura can decrypt",
        )
        .active(settings.block_quic)
        .build();

    let shared = service.clone();
    quic.connect_active_notify(move |row| {
        let enabled = row.is_active();

        if let Err(error) = shared.update(|settings| settings.block_quic = enabled) {
            notify(row, &format!("{error:#}"));
        }
    });

    let network_group = adw::PreferencesGroup::builder().title("Network").build();
    network_group.add(&quic);
    network_group.add(&row("Proxy port", &service.port().to_string()));

    let exclusions = adw::PreferencesGroup::builder()
        .title("Excluded Destinations")
        .description("Connections to these addresses or networks are never redirected, for example 10.0.0.0/8")
        .build();

    let shared = service.clone();
    let group = exclusions.clone();
    Editor::attach(
        dialog,
        &exclusions,
        "Add address or network",
        settings.exclusions,
        valid,
        move |items| {
            if let Err(error) = shared.update(|settings| settings.exclusions = items) {
                notify(&group, &format!("{error:#}"));
            }
        },
    );

    page.add(&network_group);
    page.add(&exclusions);
    page
}

fn valid(text: &str) -> bool {
    network(text).is_some()
}
