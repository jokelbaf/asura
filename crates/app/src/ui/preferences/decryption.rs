use std::cell::{Cell, RefCell};
use std::fs;
use std::rc::Rc;
use std::sync::Arc;

use adw::prelude::*;
use gtk::glib;
use proxy::Policy;

use super::editor::Editor;
use crate::service::Service;
use crate::ui::completion::Completion;
use crate::ui::{certificate, files, format, hints, keep, notify, row};

pub const NAME: &str = "decryption";

pub fn page(dialog: &adw::PreferencesDialog, service: &Arc<Service>) -> adw::PreferencesPage {
    let page = adw::PreferencesPage::builder()
        .name(NAME)
        .title("Decryption")
        .icon_name("channel-secure-symbolic")
        .build();

    let bypassed = Bypassed::new(service.policy().clone());

    page.add(&authority(dialog, service, &bypassed));
    page.add(&rules(dialog, service));
    page.add(&bypassed.group);

    keep(dialog, bypassed);
    page
}

fn authority(
    dialog: &adw::PreferencesDialog,
    service: &Arc<Service>,
    bypassed: &Rc<Bypassed>,
) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title("Certificate Authority")
        .description("Applications must trust this certificate for Asura to decrypt their traffic. Wine and Proton prefixes are set up on the Wine Prefixes page.")
        .build();

    if let Some(details) = certificate::inspect(service.authority().certificate()) {
        group.add(&row("Name", &details.name));
        group.add(&row("Valid until", &details.valid_until));
        group.add(&row("SHA-1 thumbprint", &details.thumbprint));
    }

    group.add(&row("Stored in", &control::data().to_string_lossy()));
    group.add(&System::attach(dialog, service, bypassed));

    let pem = service.authority().pem().to_owned();

    let export = adw::ButtonRow::builder()
        .title("Export Certificate…")
        .start_icon_name("send-to-symbolic")
        .build();

    export.connect_activated(glib::clone!(
        #[weak]
        dialog,
        #[strong]
        pem,
        move |_| {
            let pem = pem.clone();

            glib::spawn_future_local(async move {
                let Some(path) = files::save(
                    &dialog,
                    "Export Certificate",
                    "asura-authority.pem",
                    &[&files::CERTIFICATES],
                )
                .await
                else {
                    return;
                };

                match fs::write(&path, pem) {
                    Ok(()) => notify(&dialog, "Certificate exported"),
                    Err(error) => notify(
                        &dialog,
                        &format!("Could not export the certificate: {error}"),
                    ),
                }
            });
        }
    ));

    let copy = adw::ButtonRow::builder()
        .title("Copy Certificate")
        .start_icon_name("edit-copy-symbolic")
        .build();

    copy.connect_activated(move |row| {
        row.clipboard().set_text(&pem);
        notify(row, "Certificate copied as PEM");
    });

    group.add(&export);
    group.add(&copy);
    group
}

fn rules(dialog: &adw::PreferencesDialog, service: &Arc<Service>) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title("Do Not Decrypt")
        .description("Connections to these hosts pass through untouched. Use *.example.com to include every subdomain.")
        .build();

    let shared = service.clone();
    let target = group.clone();

    let entry = Editor::attach(
        dialog,
        &group,
        "Add host",
        service.settings().undecrypted,
        valid,
        move |items| {
            if let Err(error) = shared.update(|settings| settings.undecrypted = items) {
                notify(&target, &format!("{error:#}"));
            }
        },
    );

    Completion::attach(&entry, hints::host(hints::snapshot(service.store())));

    group
}

fn valid(text: &str) -> bool {
    !text.is_empty() && !text.contains(char::is_whitespace) && !text.contains('/')
}

struct System {
    dialog: adw::PreferencesDialog,
    row: adw::ActionRow,
    status: gtk::Label,
    button: gtk::Button,
    service: Arc<Service>,
    bypassed: Rc<Bypassed>,
    trusted: Cell<bool>,
}

impl System {
    fn attach(
        dialog: &adw::PreferencesDialog,
        service: &Arc<Service>,
        bypassed: &Rc<Bypassed>,
    ) -> adw::ActionRow {
        let status = gtk::Label::builder()
            .label("Installed")
            .css_classes(["success"])
            .visible(false)
            .build();

        let button = gtk::Button::builder()
            .valign(gtk::Align::Center)
            .sensitive(false)
            .build();

        let row = adw::ActionRow::builder()
            .title("System Trust Store")
            .subtitle("Makes Linux applications such as Chrome, Electron apps and curl trust the certificate")
            .subtitle_lines(3)
            .build();
        row.add_suffix(&status);
        row.add_suffix(&button);

        let system = Rc::new(Self {
            dialog: dialog.clone(),
            row: row.clone(),
            status,
            button: button.clone(),
            service: service.clone(),
            bypassed: bypassed.clone(),
            trusted: Cell::new(false),
        });

        button.connect_clicked(glib::clone!(
            #[weak]
            system,
            move |_| system.change()
        ));

        system.scan();
        keep(dialog, system);
        row
    }

    fn scan(self: &Rc<Self>) {
        let service = self.service.clone();
        let system = self.clone();

        glib::spawn_future_local(async move {
            let trusted = system
                .service
                .background(move || service.system_trust())
                .await;
            system.render(trusted);
        });
    }

    fn render(&self, trusted: Option<bool>) {
        let Some(trusted) = trusted else {
            self.row.set_subtitle("No supported system trust store was found. Export the certificate and add it manually.");
            self.button.set_visible(false);
            return;
        };

        self.trusted.set(trusted);
        self.status.set_visible(trusted);
        self.button.set_sensitive(true);
        self.button
            .set_label(if trusted { "Remove" } else { "Install" });
        self.button.set_css_classes(if trusted {
            &["flat"]
        } else {
            &["suggested-action"]
        });
    }

    fn change(self: &Rc<Self>) {
        let install = !self.trusted.get();
        let service = self.service.clone();
        let system = self.clone();

        self.button.set_sensitive(false);

        glib::spawn_future_local(async move {
            let (result, trusted) = system
                .service
                .background(move || (service.set_system_trust(install), service.system_trust()))
                .await;

            let message = match (result, install, trusted) {
                (Err(error), _, _) => format!("{error:#}"),
                (Ok(()), true, _) => String::from(
                    "Installed the certificate. Restart running applications to apply it.",
                ),
                (Ok(()), false, Some(true)) => String::from(
                    "Removed Asura's copy, but the system still trusts the certificate from another source",
                ),
                (Ok(()), false, _) => {
                    String::from("Removed the certificate from the system trust store")
                }
            };

            notify(&system.dialog, &message);
            system.bypassed.render();
            system.render(trusted);
        });
    }
}

struct Bypassed {
    group: adw::PreferencesGroup,
    rows: RefCell<Vec<adw::ActionRow>>,
    policy: Arc<Policy>,
}

impl Bypassed {
    fn new(policy: Arc<Policy>) -> Rc<Self> {
        let forget = gtk::Button::builder()
            .label("Forget All")
            .valign(gtk::Align::Center)
            .css_classes(["flat"])
            .build();

        let group = adw::PreferencesGroup::builder()
            .title("Passed Through Automatically")
            .description("Hosts whose clients rejected the Asura certificate. Forget a host to try decrypting it again.")
            .header_suffix(&forget)
            .build();

        let bypassed = Rc::new(Self {
            group,
            rows: RefCell::default(),
            policy,
        });

        forget.connect_clicked(glib::clone!(
            #[weak]
            bypassed,
            move |_| {
                bypassed.policy.clear();
                bypassed.render();
            }
        ));

        bypassed.render();
        bypassed
    }

    fn render(self: &Rc<Self>) {
        for row in self.rows.borrow_mut().drain(..) {
            self.group.remove(&row);
        }

        let entries = self.policy.bypassed();

        if entries.is_empty() {
            let row = adw::ActionRow::builder()
                .title("No hosts")
                .css_classes(["dimmed"])
                .build();
            self.group.add(&row);
            self.rows.borrow_mut().push(row);
        }

        for (key, rejection) in entries {
            let process = match rejection.stale {
                true => format!(
                    "{} (this process only; restart it to decrypt)",
                    rejection.client.name
                ),
                false => rejection.client.name.clone(),
            };

            let row = adw::ActionRow::builder().use_markup(false).build();
            row.set_title(&key);
            row.set_subtitle(&format::printable(&format!(
                "{process} · {}",
                rejection.reason
            )));

            let forget = gtk::Button::builder()
                .icon_name("edit-undo-symbolic")
                .tooltip_text("Forget")
                .valign(gtk::Align::Center)
                .css_classes(["flat"])
                .build();

            let weak = Rc::downgrade(self);
            forget.connect_clicked(move |_| {
                if let Some(bypassed) = weak.upgrade() {
                    bypassed.policy.forget(&key);
                    bypassed.render();
                }
            });

            row.add_suffix(&forget);
            self.group.add(&row);
            self.rows.borrow_mut().push(row);
        }
    }
}
