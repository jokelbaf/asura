use std::cell::RefCell;
use std::fs;
use std::rc::Rc;

use adw::prelude::*;
use gtk::glib;
use proxy::{RequestDraft, ResponseDraft};
use session::Headers;
use sourceview::prelude::*;

use super::completion::Completion;
use super::hints::{self, Catalog};
use super::inspector::{TEXT_LIMIT, language};
use super::{files, format, notify, scheme};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Request,
    Url,
    Response,
}

pub struct Editor {
    pub widget: gtk::Box,
    kind: Kind,
    method: gtk::Entry,
    target: gtk::Entry,
    status: gtk::Entry,
    headers: sourceview::Buffer,
    body: sourceview::Buffer,
    stack: gtk::Stack,
    note: adw::StatusPage,
    binary: RefCell<Option<Vec<u8>>>,
}

impl Editor {
    pub fn new(kind: Kind) -> Rc<Self> {
        let method = gtk::Entry::builder()
            .placeholder_text("GET")
            .width_chars(9)
            .max_width_chars(12)
            .css_classes(["monospace"])
            .build();

        let target = gtk::Entry::builder()
            .placeholder_text(if kind == Kind::Url {
                "https://example.com/path"
            } else {
                "/path?query"
            })
            .hexpand(true)
            .css_classes(["monospace"])
            .build();

        let status = gtk::Entry::builder()
            .width_chars(6)
            .max_width_chars(6)
            .input_purpose(gtk::InputPurpose::Digits)
            .css_classes(["monospace"])
            .build();
        Completion::attach(&status, hints::status());

        let line = gtk::Box::builder()
            .spacing(6)
            .margin_start(12)
            .margin_end(12)
            .margin_top(8)
            .margin_bottom(8)
            .build();

        if kind == Kind::Response {
            line.append(
                &gtk::Label::builder()
                    .label("Status")
                    .css_classes(["heading"])
                    .build(),
            );
            line.append(&status);
        } else {
            line.append(&method);
            line.append(&target);
        }

        let headers = buffer();
        let body = buffer();

        let note = adw::StatusPage::builder()
            .icon_name("application-x-executable-symbolic")
            .title("Body Not Editable as Text")
            .css_classes(["compact"])
            .build();

        let stack = gtk::Stack::new();
        stack.add_named(&scrolled(&body, true), Some("text"));
        stack.add_named(&note, Some("binary"));

        let load = gtk::Button::builder()
            .label("Load File…")
            .css_classes(["flat"])
            .build();

        let body_header = gtk::Box::builder().spacing(6).margin_end(12).build();
        body_header.append(&caption("Body"));
        body_header.append(&load);

        let upper = gtk::Box::new(gtk::Orientation::Vertical, 4);
        upper.append(&caption("Headers"));
        upper.append(&scrolled(&headers, false));

        let lower = gtk::Box::new(gtk::Orientation::Vertical, 4);
        lower.append(&body_header);
        lower.append(&stack);

        let paned = gtk::Paned::builder()
            .orientation(gtk::Orientation::Vertical)
            .start_child(&upper)
            .end_child(&lower)
            .shrink_start_child(false)
            .shrink_end_child(false)
            .position(200)
            .wide_handle(true)
            .vexpand(true)
            .build();

        let widget = gtk::Box::new(gtk::Orientation::Vertical, 0);
        widget.append(&line);
        widget.append(&paned);

        let editor = Rc::new(Self {
            widget,
            kind,
            method,
            target,
            status,
            headers,
            body,
            stack,
            note,
            binary: RefCell::default(),
        });

        let weak = Rc::downgrade(&editor);
        load.connect_clicked(move |_| {
            if let Some(editor) = weak.upgrade() {
                editor.browse();
            }
        });

        editor
    }

    pub fn load_request(&self, draft: &RequestDraft) {
        self.method.set_text(&draft.method);
        self.target.set_text(&format::printable(&draft.target));
        self.load(&draft.headers, &draft.body);
    }

    pub fn load_response(&self, draft: &ResponseDraft) {
        self.status.set_text(&draft.status.to_string());
        self.load(&draft.headers, &draft.body);
    }

    pub fn request(&self) -> Result<RequestDraft, String> {
        let method = self.method.text().trim().to_owned();
        let target = self.target.text().trim().to_owned();

        if method.is_empty() || method.contains(char::is_whitespace) {
            return Err(String::from(
                "Enter a request method without spaces, for example GET",
            ));
        }

        if target.is_empty() {
            return Err(String::from(match self.kind {
                Kind::Url => "Enter the URL to send the request to",
                _ => "Enter the request path",
            }));
        }

        if self.kind == Kind::Url
            && !["http://", "https://"]
                .iter()
                .any(|scheme| target.to_ascii_lowercase().starts_with(scheme))
        {
            return Err(String::from("The URL must start with http:// or https://"));
        }

        Ok(RequestDraft {
            method,
            target,
            headers: Headers::parse(&text(&self.headers)),
            body: self.content(),
        })
    }

    pub fn response(&self) -> Result<ResponseDraft, String> {
        let status = hints::code(&self.status.text())
            .ok_or_else(|| String::from("Enter a status code between 100 and 999"))?;

        Ok(ResponseDraft {
            status,
            headers: Headers::parse(&text(&self.headers)),
            body: self.content(),
        })
    }

    pub fn suggest(&self, catalog: Rc<Catalog>) {
        Completion::attach(&self.method, hints::method(catalog.clone()));

        if self.kind == Kind::Url {
            Completion::attach(&self.target, hints::url(catalog));
        }
    }

    pub fn focus(&self) {
        let field = if self.kind == Kind::Response {
            self.status.upcast_ref::<gtk::Widget>()
        } else {
            self.target.upcast_ref()
        };
        field.grab_focus();
    }

    fn load(&self, headers: &Headers, body: &[u8]) {
        self.headers.set_text(&format::printable(&headers.lines()));
        self.fill(body, headers.text("content-type").unwrap_or_default());
    }

    fn fill(&self, body: &[u8], kind: &str) {
        match std::str::from_utf8(body) {
            Ok(text) if !text.contains('\0') && body.len() <= TEXT_LIMIT => {
                self.binary.replace(None);
                self.body.set_language(
                    language(kind, body)
                        .and_then(|id| sourceview::LanguageManager::default().language(id))
                        .as_ref(),
                );
                self.body.set_text(text);
                self.stack.set_visible_child_name("text");
            }
            _ => {
                self.note.set_description(Some(&format!(
                    "{} that cannot be edited here. It is sent unchanged unless you load a file.",
                    format::size(body.len() as u64)
                )));
                self.binary.replace(Some(body.to_vec()));
                self.stack.set_visible_child_name("binary");
            }
        }
    }

    fn content(&self) -> Vec<u8> {
        match &*self.binary.borrow() {
            Some(bytes) => bytes.clone(),
            None => text(&self.body).into_bytes(),
        }
    }

    fn browse(self: &Rc<Self>) {
        let editor = self.clone();

        glib::spawn_future_local(async move {
            let Some(path) = files::open(&editor.widget, "Load Body", &[&files::ANY]).await else {
                return;
            };

            match fs::read(&path) {
                Ok(bytes) => {
                    let kind = Headers::parse(&text(&editor.headers))
                        .text("content-type")
                        .unwrap_or_default()
                        .to_owned();
                    editor.fill(&bytes, &kind);
                }
                Err(error) => notify(
                    &editor.widget,
                    &format!("Could not read {}: {error}", path.display()),
                ),
            }
        });
    }
}

fn buffer() -> sourceview::Buffer {
    let buffer = sourceview::Buffer::new(None);
    buffer.set_highlight_matching_brackets(false);
    scheme(&buffer);
    buffer
}

fn scrolled(buffer: &sourceview::Buffer, numbered: bool) -> gtk::ScrolledWindow {
    let view = sourceview::View::builder()
        .buffer(buffer)
        .monospace(true)
        .show_line_numbers(numbered)
        .wrap_mode(gtk::WrapMode::WordChar)
        .top_margin(6)
        .bottom_margin(6)
        .left_margin(6)
        .right_margin(6)
        .css_classes(["body-view", "editor"])
        .build();

    gtk::ScrolledWindow::builder()
        .child(&view)
        .vexpand(true)
        .has_frame(true)
        .margin_start(12)
        .margin_end(12)
        .margin_bottom(12)
        .css_classes(["editor-frame"])
        .build()
}

fn caption(text: &str) -> gtk::Label {
    gtk::Label::builder()
        .label(text)
        .xalign(0.0)
        .hexpand(true)
        .margin_start(12)
        .margin_top(6)
        .css_classes(["heading"])
        .build()
}

fn text(buffer: &sourceview::Buffer) -> String {
    let (start, end) = buffer.bounds();
    buffer.text(&start, &end, true).into()
}
