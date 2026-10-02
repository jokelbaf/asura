use std::cell::{OnceCell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use adw::prelude::*;
use gtk::{gio, glib};
use tokio::runtime::Handle;

use crate::service::Service;
use crate::ui::{self, Window};

pub const ID: &str = "app.nekolab.asura";

const ACCELERATORS: [(&str, &[&str]); 18] = [
    ("win.capture", &["F12"]),
    ("win.open", &["<Control>o"]),
    ("win.save", &["<Control>s"]),
    ("win.export", &["<Control>e"]),
    ("win.find", &["<Control>f"]),
    ("win.launch", &["<Control>l"]),
    ("win.attach", &["<Control><Shift>a"]),
    ("win.clear", &["<Control><Shift>Delete"]),
    ("win.copy-curl", &["<Control><Shift>c"]),
    ("win.replay", &["<Control>r"]),
    ("win.resend", &["<Control><Shift>r"]),
    ("win.compose", &["<Control>n"]),
    ("win.search", &["<Control><Shift>f"]),
    ("win.rules", &["<Control>b"]),
    ("win.shortcuts", &["<Control>question"]),
    ("app.preferences", &["<Control>comma"]),
    ("app.quit", &["<Control>q"]),
    ("window.close", &["<Control>w"]),
];

#[derive(Default)]
struct State {
    service: OnceCell<Arc<Service>>,
    window: RefCell<Option<Rc<Window>>>,
}

impl State {
    fn window(&self, application: &adw::Application) -> Option<Rc<Window>> {
        let service = self.service.get()?;

        Some(
            self.window
                .borrow_mut()
                .get_or_insert_with(|| Window::new(application, service.clone()))
                .clone(),
        )
    }
}

pub fn run(runtime: Handle) -> glib::ExitCode {
    let application = adw::Application::builder()
        .application_id(ID)
        .flags(gio::ApplicationFlags::HANDLES_OPEN)
        .build();

    let state = Rc::new(State::default());

    application.connect_startup(glib::clone!(
        #[strong]
        state,
        move |application| {
            ui::load_style();
            gtk::Window::set_default_icon_name(ID);

            match Service::start(runtime.clone()) {
                Ok(service) => {
                    let _ = state.service.set(service);
                    actions(application, &state);
                }
                Err(error) => fail(application, &error),
            }
        }
    ));

    application.connect_activate(glib::clone!(
        #[strong]
        state,
        move |application| {
            if let Some(window) = state.window(application) {
                window.present();
            }
        }
    ));

    application.connect_open(glib::clone!(
        #[strong]
        state,
        move |application, files, _| {
            let Some(window) = state.window(application) else {
                return;
            };

            window.present();

            for path in files.iter().filter_map(|file| file.path()) {
                window.open(path);
            }
        }
    ));

    application.run()
}

fn actions(application: &adw::Application, state: &Rc<State>) {
    let preferences = gio::SimpleAction::new("preferences", None);
    preferences.connect_activate(glib::clone!(
        #[weak]
        application,
        #[strong]
        state,
        move |_, _| {
            if let (Some(window), Some(service)) = (state.window(&application), state.service.get())
            {
                ui::preferences(&window.window, service);
            }
        }
    ));

    let about = gio::SimpleAction::new("about", None);
    about.connect_activate(glib::clone!(
        #[weak]
        application,
        move |_, _| {
            adw::AboutDialog::builder()
                .application_name("Asura")
                .application_icon(ID)
                .version(env!("CARGO_PKG_VERSION"))
                .license_type(gtk::License::Gpl30Only)
                .website(env!("CARGO_PKG_HOMEPAGE"))
                .comments("Inspect the HTTP and HTTPS traffic of Windows applications running under Wine and Proton")
                .build()
                .present(application.active_window().as_ref());
        }
    ));

    let quit = gio::SimpleAction::new("quit", None);
    quit.connect_activate(glib::clone!(
        #[weak]
        application,
        move |_, _| {
            for window in application.windows() {
                window.close();
            }
        }
    ));

    application.add_action(&preferences);
    application.add_action(&about);
    application.add_action(&quit);

    for (action, accelerators) in ACCELERATORS {
        application.set_accels_for_action(action, accelerators);
    }
}

fn fail(application: &adw::Application, error: &anyhow::Error) {
    let guard = application.hold();

    let dialog = adw::AlertDialog::builder()
        .heading("Asura Could Not Start")
        .body(format!("{error:#}"))
        .build();

    dialog.add_response("close", "Close");
    dialog.connect_response(
        None,
        glib::clone!(
            #[weak]
            application,
            move |_, _| {
                let _ = &guard;
                application.quit();
            }
        ),
    );

    dialog.present(None::<&gtk::Widget>);
}
