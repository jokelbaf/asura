use std::path::PathBuf;
use std::rc::Rc;

use adw::prelude::*;
use gtk::glib;
use proxy::{Action, Responder};

use super::form::{self, Matcher};
use super::list::Rule;
use crate::service::Settings;
use crate::ui::completion::Completion;
use crate::ui::hints::{self, Catalog};
use crate::ui::{files, keep, scheme};

const KINDS: [&str; 2] = ["Mock response", "File"];
const DEFAULT_TYPE: &str = "application/json";

impl Rule for Responder {
    const ADDED: &'static str = "Added an auto-responder rule";

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
        let action = match &self.action {
            Action::Mock {
                status,
                content_type,
                ..
            } if content_type.is_empty() => format!("Respond with {status}"),
            Action::Mock {
                status,
                content_type,
                ..
            } => format!("Respond with {status} {content_type}"),
            Action::File(path) => format!(
                "Respond with {}",
                path.file_name().map_or_else(
                    || path.display().to_string(),
                    |name| name.to_string_lossy().into_owned()
                )
            ),
        };

        format!("{} · {action}", form::method(&self.method))
    }

    fn all(settings: &Settings) -> Vec<Self> {
        settings.responders.clone()
    }

    fn store(settings: &mut Settings, rules: Vec<Self>) {
        settings.responders = rules;
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
    kind: adw::ComboRow,
    status: adw::EntryRow,
    content_type: adw::EntryRow,
    file: adw::EntryRow,
    body: sourceview::Buffer,
    body_group: adw::PreferencesGroup,
    save: gtk::Button,
    enabled: bool,
    done: Rc<dyn Fn(Responder)>,
}

impl Form {
    fn present(
        parent: &gtk::Widget,
        rule: Option<Responder>,
        catalog: Rc<Catalog>,
        done: Rc<dyn Fn(Responder)>,
    ) {
        let rule = rule.unwrap_or_else(|| Responder {
            enabled: true,
            pattern: String::new(),
            method: String::new(),
            action: Action::Mock {
                status: 200,
                content_type: String::from(DEFAULT_TYPE),
                body: String::new(),
            },
        });

        let matcher = Matcher::new(&rule.pattern, &rule.method, catalog);

        let kind = adw::ComboRow::builder()
            .title("Respond with")
            .model(&gtk::StringList::new(&KINDS))
            .build();

        let status = adw::EntryRow::builder()
            .title("Status")
            .text("200")
            .input_purpose(gtk::InputPurpose::Digits)
            .build();
        Completion::attach(&status, hints::status());

        let content_type = adw::EntryRow::builder()
            .title("Content type")
            .text(DEFAULT_TYPE)
            .build();
        let file = adw::EntryRow::builder().title("File").build();

        let browse = gtk::Button::builder()
            .icon_name("document-open-symbolic")
            .tooltip_text("Choose File")
            .valign(gtk::Align::Center)
            .css_classes(["flat"])
            .build();
        file.add_suffix(&browse);

        let response = adw::PreferencesGroup::builder()
            .title("Response")
            .description("A file that starts with a status line such as HTTP/1.1 200 OK is sent as a complete response, headers included.")
            .build();
        response.add(&kind);
        response.add(&status);
        response.add(&content_type);
        response.add(&file);

        let body = sourceview::Buffer::new(None);
        scheme(&body);

        let view = sourceview::View::builder()
            .buffer(&body)
            .monospace(true)
            .wrap_mode(gtk::WrapMode::WordChar)
            .top_margin(6)
            .bottom_margin(6)
            .left_margin(6)
            .right_margin(6)
            .css_classes(["body-view"])
            .build();

        let frame = gtk::Frame::builder()
            .child(
                &gtk::ScrolledWindow::builder()
                    .child(&view)
                    .height_request(200)
                    .build(),
            )
            .build();

        let body_group = adw::PreferencesGroup::builder().title("Body").build();
        body_group.add(&frame);

        match &rule.action {
            Action::Mock {
                status: code,
                content_type: kind_text,
                body: text,
            } => {
                status.set_text(&code.to_string());
                content_type.set_text(kind_text);
                body.set_text(text);
            }
            Action::File(path) => {
                kind.set_selected(1);
                file.set_text(&path.to_string_lossy());
            }
        }

        let page = adw::PreferencesPage::new();
        page.add(&matcher.group);
        page.add(&response);
        page.add(&body_group);

        let (dialog, save) = form::dialog("Auto-Responder Rule", 680, &page);

        let form = Rc::new(Self {
            dialog,
            matcher,
            kind,
            status,
            content_type,
            file,
            body,
            body_group,
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
        form.kind.connect_selected_notify(move |_| changed());

        let changed = validate.clone();
        form.status.connect_changed(move |_| changed());
        form.file.connect_changed(move |_| validate());

        browse.connect_clicked(glib::clone!(
            #[weak]
            form,
            move |_| form.browse()
        ));

        form.save.connect_clicked(glib::clone!(
            #[weak]
            form,
            move |_| {
                (form.done)(form.rule());
                form.dialog.close();
            }
        ));

        form.validate();
        form.dialog.present(Some(parent));
        keep(&form.dialog, form.clone());
    }

    fn mock(&self) -> bool {
        self.kind.selected() == 0
    }

    fn validate(&self) {
        let mock = self.mock();

        self.status.set_visible(mock);
        self.content_type.set_visible(mock);
        self.body_group.set_visible(mock);
        self.file.set_visible(!mock);

        let status = hints::code(&self.status.text()).is_some();

        if status || !mock {
            self.status.remove_css_class("error");
        } else {
            self.status.add_css_class("error");
        }

        let valid = self.matcher.valid();
        let ready = if mock {
            status
        } else {
            !self.file.text().trim().is_empty()
        };
        self.save.set_sensitive(valid && ready);
    }

    fn rule(&self) -> Responder {
        let action = if self.mock() {
            let (start, end) = self.body.bounds();

            Action::Mock {
                status: hints::code(&self.status.text()).unwrap_or(200),
                content_type: self.content_type.text().trim().to_owned(),
                body: self.body.text(&start, &end, true).into(),
            }
        } else {
            Action::File(PathBuf::from(self.file.text().trim()))
        };

        Responder {
            enabled: self.enabled,
            pattern: self.matcher.pattern(),
            method: self.matcher.method(),
            action,
        }
    }

    fn browse(self: &Rc<Self>) {
        let form = self.clone();

        glib::spawn_future_local(async move {
            if let Some(path) =
                files::open(&form.dialog, "Choose Response File", &[&files::ANY]).await
            {
                form.file.set_text(&path.to_string_lossy());
            }
        });
    }
}
