use std::rc::Rc;

use adw::prelude::*;
use gtk::glib;
use proxy::Breakpoint;

use super::form::{self, Matcher};
use super::list::Rule;
use crate::service::Settings;
use crate::ui::hints::Catalog;
use crate::ui::keep;

impl Rule for Breakpoint {
    const ADDED: &'static str = "Added a breakpoint";

    fn pattern(&self) -> &str {
        &self.pattern
    }

    fn enabled(&self) -> bool {
        self.enabled
    }

    fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    fn summary(&self) -> String {
        let stages = match (self.request, self.response) {
            (true, true) => "Before the request and after the response",
            (true, false) => "Before the request",
            (false, true) => "After the response",
            (false, false) => "Never pauses",
        };

        format!("{} · {stages}", form::method(&self.method))
    }

    fn all(settings: &Settings) -> Vec<Self> {
        settings.breakpoints.clone()
    }

    fn store(settings: &mut Settings, rules: Vec<Self>) {
        settings.breakpoints = rules;
    }

    fn edit(
        parent: &gtk::Widget,
        rule: Option<Self>,
        catalog: Rc<Catalog>,
        done: Rc<dyn Fn(Self)>,
    ) {
        Form::present(parent, rule, catalog, done);
    }
}

struct Form {
    dialog: adw::Dialog,
    matcher: Matcher,
    request: adw::SwitchRow,
    response: adw::SwitchRow,
    save: gtk::Button,
    enabled: bool,
    done: Rc<dyn Fn(Breakpoint)>,
}

impl Form {
    fn present(
        parent: &gtk::Widget,
        rule: Option<Breakpoint>,
        catalog: Rc<Catalog>,
        done: Rc<dyn Fn(Breakpoint)>,
    ) {
        let rule = rule.unwrap_or_else(|| Breakpoint {
            enabled: true,
            pattern: String::new(),
            method: String::new(),
            request: true,
            response: false,
        });

        let matcher = Matcher::new(&rule.pattern, &rule.method, catalog);

        let request = adw::SwitchRow::builder()
            .title("Before the request is sent")
            .subtitle("Edit the method, path, headers or body, or abort the request")
            .active(rule.request)
            .build();

        let response = adw::SwitchRow::builder()
            .title("After the response arrives")
            .subtitle("Edit the status, headers or body before the application sees them")
            .active(rule.response)
            .build();

        let pause = adw::PreferencesGroup::builder().title("Pause").build();
        pause.add(&request);
        pause.add(&response);

        let page = adw::PreferencesPage::new();
        page.add(&matcher.group);
        page.add(&pause);

        let (dialog, save) = form::dialog("Breakpoint", 460, &page);

        let form = Rc::new(Self {
            dialog,
            matcher,
            request,
            response,
            save,
            enabled: rule.enabled,
            done,
        });

        let weak = Rc::downgrade(&form);
        let validate = move || {
            if let Some(form) = weak.upgrade() {
                form.validate();
            }
        };

        form.matcher.connect_changed(validate.clone());

        let changed = validate.clone();
        form.request.connect_active_notify(move |_| changed());
        form.response.connect_active_notify(move |_| validate());

        form.save.connect_clicked(glib::clone!(
            #[weak]
            form,
            move |_| {
                (form.done)(Breakpoint {
                    enabled: form.enabled,
                    pattern: form.matcher.pattern(),
                    method: form.matcher.method(),
                    request: form.request.is_active(),
                    response: form.response.is_active(),
                });
                form.dialog.close();
            }
        ));

        form.validate();
        form.dialog.present(Some(parent));
        keep(&form.dialog, form.clone());
    }

    fn validate(&self) {
        let valid = self.matcher.valid();
        self.save
            .set_sensitive(valid && (self.request.is_active() || self.response.is_active()));
    }
}
