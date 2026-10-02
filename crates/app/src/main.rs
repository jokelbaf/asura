mod application;
mod service;
mod ui;

use gtk::glib;

fn main() -> glib::ExitCode {
    let runtime = tokio::runtime::Runtime::new().expect("starting the async runtime");
    let code = application::run(runtime.handle().clone());

    runtime.shutdown_background();
    code
}
