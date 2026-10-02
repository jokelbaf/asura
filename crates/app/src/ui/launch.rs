use std::cell::RefCell;
use std::env;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use adw::prelude::*;
use gtk::{gio, glib};
use trust::Prefix;

use crate::service::Service;
use crate::ui::{files, keep, notify};

const LAUNCHER: &str = "asura-run";
const WINE: &str = "wine";

struct Form {
    dialog: adw::Dialog,
    program: adw::EntryRow,
    arguments: adw::EntryRow,
    name: adw::EntryRow,
    wine_group: adw::PreferencesGroup,
    prefix: adw::ComboRow,
    wine: adw::EntryRow,
    launch: gtk::Button,
    prefixes: RefCell<Vec<Prefix>>,
}

pub fn present(parent: &impl IsA<gtk::Widget>, service: Arc<Service>) {
    let program = adw::EntryRow::builder().title("Program").build();
    let browse = gtk::Button::builder()
        .icon_name("document-open-symbolic")
        .tooltip_text("Choose Program")
        .valign(gtk::Align::Center)
        .css_classes(["flat"])
        .build();
    program.add_suffix(&browse);

    let arguments = adw::EntryRow::builder().title("Arguments").build();
    let name = adw::EntryRow::builder()
        .title("Name in Asura (optional)")
        .build();

    let general = adw::PreferencesGroup::builder()
        .description(
            "The program and everything it starts is captured. Windows programs run through Wine.",
        )
        .build();
    general.add(&program);
    general.add(&arguments);
    general.add(&name);

    let prefix = adw::ComboRow::builder().title("Prefix").build();
    let wine = adw::EntryRow::builder()
        .title("Wine executable")
        .text(WINE)
        .build();

    let wine_group = adw::PreferencesGroup::builder()
        .title("Wine")
        .description("The Asura certificate is installed into the prefix before the program starts")
        .visible(false)
        .build();
    wine_group.add(&prefix);
    wine_group.add(&wine);

    let page = adw::PreferencesPage::new();
    page.add(&general);
    page.add(&wine_group);

    let launch = gtk::Button::builder()
        .label("Launch")
        .sensitive(false)
        .css_classes(["suggested-action"])
        .build();

    let header = adw::HeaderBar::new();
    header.pack_end(&launch);

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&page));

    let dialog = adw::Dialog::builder()
        .title("Launch Program")
        .content_width(560)
        .content_height(560)
        .child(&toolbar)
        .build();

    let form = Rc::new(Form {
        dialog,
        program,
        arguments,
        name,
        wine_group,
        prefix,
        wine,
        launch,
        prefixes: RefCell::default(),
    });

    form.program.connect_changed(glib::clone!(
        #[weak]
        form,
        move |_| form.validate()
    ));

    browse.connect_clicked(glib::clone!(
        #[weak]
        form,
        move |_| form.browse()
    ));

    form.launch.connect_clicked(glib::clone!(
        #[weak]
        form,
        move |_| form.start()
    ));

    form.load(service);
    form.dialog.present(Some(parent));
    keep(&form.dialog, form.clone());
}

impl Form {
    fn load(self: &Rc<Self>, service: Arc<Service>) {
        let custom = service.settings().prefixes;
        let form = self.clone();

        glib::spawn_future_local(async move {
            let prefixes = service
                .background(move || super::preferences::prefixes(&custom))
                .await;
            let names: Vec<String> = prefixes
                .iter()
                .map(|prefix| format!("{} — {}", prefix.name, prefix.path.display()))
                .collect();

            form.prefix.set_model(Some(&gtk::StringList::new(
                &names.iter().map(String::as_str).collect::<Vec<_>>(),
            )));
            form.prefixes.replace(prefixes);
            form.validate();
        });
    }

    fn browse(self: &Rc<Self>) {
        let form = self.clone();

        glib::spawn_future_local(async move {
            if let Some(path) = files::open(&form.dialog, "Choose Program", &[&files::ANY]).await {
                form.program.set_text(&path.to_string_lossy());
            }
        });
    }

    fn validate(&self) {
        let program = self.program.text();
        let windows = program.to_ascii_lowercase().ends_with(".exe");

        self.wine_group.set_visible(windows);
        self.launch.set_sensitive(
            !program.trim().is_empty() && (!windows || !self.prefixes.borrow().is_empty()),
        );
    }

    fn start(&self) {
        match self.spawn() {
            Ok(name) => {
                notify(&self.dialog, &format!("Launching {name}"));
                self.dialog.close();
            }
            Err(error) => notify(
                &self.dialog,
                &format!("Could not launch the program: {error}"),
            ),
        }
    }

    fn spawn(&self) -> Result<String, glib::Error> {
        let program = PathBuf::from(self.program.text().trim());
        let windows = program
            .to_string_lossy()
            .to_ascii_lowercase()
            .ends_with(".exe");
        let arguments = match self.arguments.text().trim() {
            "" => Vec::new(),
            text => glib::shell_parse_argv(text)?,
        };

        let name = match self.name.text().trim() {
            "" => stem(&program),
            text => text.to_owned(),
        };

        let launcher = gio::SubprocessLauncher::new(gio::SubprocessFlags::NONE);

        if let Some(directory) = program.parent().filter(|directory| directory.is_dir()) {
            launcher.set_cwd(directory);
        }

        let mut command: Vec<&OsStr> = Vec::new();
        let wine = self.wine.text();
        let tool = locate();

        command.extend([
            tool.as_os_str(),
            OsStr::new("--name"),
            OsStr::new(&name),
            OsStr::new("--"),
        ]);

        if windows {
            if let Some(prefix) = self.prefixes.borrow().get(self.prefix.selected() as usize) {
                launcher.setenv("WINEPREFIX", &prefix.path, true);
            }

            command.push(OsStr::new(wine.trim()));
        }

        command.push(program.as_os_str());
        command.extend(arguments.iter().map(|argument| argument.as_os_str()));

        let process = launcher.spawn(&command)?;
        glib::spawn_future_local(async move {
            let _ = process.wait_future().await;
        });

        Ok(name)
    }
}

fn locate() -> PathBuf {
    env::current_exe()
        .map(|executable| executable.with_file_name(LAUNCHER))
        .ok()
        .filter(|candidate| candidate.exists())
        .unwrap_or_else(|| PathBuf::from(LAUNCHER))
}

fn stem(program: &Path) -> String {
    program.file_stem().map_or_else(
        || program.to_string_lossy().into_owned(),
        |stem| stem.to_string_lossy().into_owned(),
    )
}
