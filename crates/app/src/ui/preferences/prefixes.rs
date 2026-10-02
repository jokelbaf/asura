use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use adw::prelude::*;
use gtk::glib;
use trust::{Prefix, Source, Status};

use crate::service::Service;
use crate::ui::{files, format, keep, notify};

struct Scanned {
    prefix: Prefix,
    status: Status,
    running: bool,
}

struct Prefixes {
    dialog: adw::PreferencesDialog,
    group: adw::PreferencesGroup,
    rows: RefCell<Vec<adw::ActionRow>>,
    service: Arc<Service>,
    certificate: Arc<Vec<u8>>,
}

pub fn prefixes(custom: &[PathBuf]) -> Vec<Prefix> {
    let mut found = trust::discover();

    for path in custom {
        if !found.iter().any(|prefix| same(&prefix.path, path)) {
            let name = path
                .file_name()
                .map_or_else(|| path.to_string_lossy(), |name| name.to_string_lossy());
            found.push(Prefix::new(path.clone(), name, Source::Custom));
        }
    }

    found
}

pub fn page(dialog: &adw::PreferencesDialog, service: &Arc<Service>) -> adw::PreferencesPage {
    let refresh = gtk::Button::builder()
        .icon_name("view-refresh-symbolic")
        .tooltip_text("Refresh")
        .valign(gtk::Align::Center)
        .css_classes(["flat"])
        .build();

    let group = adw::PreferencesGroup::builder()
        .title("Wine Prefixes")
        .description("Windows programs trust the Asura certificate once it is installed into their prefix. Close every program running in a prefix before changing it.")
        .header_suffix(&refresh)
        .build();

    let add = adw::ButtonRow::builder().title("Add Prefix…").build();

    let extra = adw::PreferencesGroup::new();
    extra.add(&add);

    let page = adw::PreferencesPage::builder()
        .title("Wine Prefixes")
        .icon_name("applications-games-symbolic")
        .build();
    page.add(&group);
    page.add(&extra);

    let state = Rc::new(Prefixes {
        dialog: dialog.clone(),
        group,
        rows: RefCell::default(),
        service: service.clone(),
        certificate: Arc::new(service.authority().certificate().to_vec()),
    });

    refresh.connect_clicked(glib::clone!(
        #[weak]
        state,
        move |_| state.scan()
    ));

    add.connect_activated(glib::clone!(
        #[weak]
        state,
        move |_| state.add()
    ));

    state.scan();
    keep(dialog, state);
    page
}

impl Prefixes {
    fn scan(self: &Rc<Self>) {
        let custom = self.service.settings().prefixes;
        let certificate = self.certificate.clone();
        let state = self.clone();

        glib::spawn_future_local(async move {
            let scanned = state
                .service
                .background(move || {
                    prefixes(&custom)
                        .into_iter()
                        .map(|prefix| Scanned {
                            status: prefix.status(&certificate),
                            running: prefix.in_use(),
                            prefix,
                        })
                        .collect::<Vec<_>>()
                })
                .await;

            state.render(scanned);
        });
    }

    fn render(self: &Rc<Self>, scanned: Vec<Scanned>) {
        for row in self.rows.borrow_mut().drain(..) {
            self.group.remove(&row);
        }

        if scanned.is_empty() {
            let row = adw::ActionRow::builder()
                .title("No prefixes found")
                .css_classes(["dimmed"])
                .build();
            self.group.add(&row);
            self.rows.borrow_mut().push(row);
        }

        for item in scanned {
            let row = adw::ActionRow::builder()
                .use_markup(false)
                .subtitle_lines(2)
                .build();
            row.set_title(&format::printable(&item.prefix.name));
            row.set_subtitle(&format!(
                "{} · {}",
                source(&item.prefix.source),
                item.prefix.path.display()
            ));

            let (label, action) = match item.status {
                Status::Installed => ("Installed", Some(("Remove", false))),
                Status::Missing => ("", Some(("Install", true))),
                Status::Uninitialised => ("Not initialised", None),
            };

            if !label.is_empty() {
                row.add_suffix(
                    &gtk::Label::builder()
                        .label(label)
                        .css_classes([if item.status == Status::Installed {
                            "success"
                        } else {
                            "dimmed"
                        }])
                        .build(),
                );
            }

            if let Some((title, install)) = action {
                let button = gtk::Button::builder()
                    .label(title)
                    .valign(gtk::Align::Center)
                    .sensitive(!item.running)
                    .css_classes(if install {
                        ["suggested-action"]
                    } else {
                        ["flat"]
                    })
                    .build();

                if item.running {
                    button
                        .set_tooltip_text(Some("Close every program running in this prefix first"));
                }

                let weak = Rc::downgrade(self);
                let prefix = item.prefix.clone();
                button.connect_clicked(move |_| {
                    if let Some(state) = weak.upgrade() {
                        state.change(prefix.clone(), install);
                    }
                });

                row.add_suffix(&button);
            }

            self.group.add(&row);
            self.rows.borrow_mut().push(row);
        }
    }

    fn change(self: &Rc<Self>, prefix: Prefix, install: bool) {
        let certificate = self.certificate.clone();
        let state = self.clone();

        glib::spawn_future_local(async move {
            let name = prefix.name.clone();
            let result = state
                .service
                .background(move || match install {
                    true => prefix.install(&certificate),
                    false => prefix.uninstall(&certificate),
                })
                .await;

            let message = match (result, install) {
                (Ok(_), true) => format!("Installed the certificate into {name}"),
                (Ok(_), false) => format!("Removed the certificate from {name}"),
                (Err(error), _) => error.to_string(),
            };

            notify(&state.dialog, &message);
            state.scan();
        });
    }

    fn add(self: &Rc<Self>) {
        let state = self.clone();

        glib::spawn_future_local(async move {
            let Some(path) = files::folder(&state.dialog, "Add Wine Prefix").await else {
                return;
            };

            let result = state.service.update(|settings| {
                if !settings.prefixes.contains(&path) {
                    settings.prefixes.push(path);
                }
            });

            if let Err(error) = result {
                notify(&state.dialog, &format!("{error:#}"));
            }

            state.scan();
        });
    }
}

fn source(source: &Source) -> String {
    match source {
        Source::Wine => String::from("Wine"),
        Source::Steam { app } => format!("Steam app {app}"),
        Source::Lutris => String::from("Lutris"),
        Source::Heroic => String::from("Heroic"),
        Source::Bottles => String::from("Bottles"),
        Source::Custom => String::from("Custom"),
    }
}

fn same(left: &Path, right: &Path) -> bool {
    let canonical = |path: &Path| fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    canonical(left) == canonical(right)
}
