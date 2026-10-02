use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use adw::prelude::*;
use proxy::{RequestDraft, ResponseDraft, Verdict};
use session::{Entry, Record, Stage};

use crate::service::Service;
use crate::ui::editor::{Editor, Kind};
use crate::ui::notify;

struct Paused {
    entry: Arc<Entry>,
    stage: Stage,
    request: Option<RequestDraft>,
    response: Option<ResponseDraft>,
}

pub struct BreakpointPage {
    pub widget: gtk::Box,
    title: gtk::Label,
    editors: gtk::Stack,
    request: Rc<Editor>,
    response: Rc<Editor>,
    follow: gtk::CheckButton,
    service: Arc<Service>,
    paused: RefCell<Option<Paused>>,
}

impl BreakpointPage {
    pub fn new(service: Arc<Service>) -> Rc<Self> {
        let request = Editor::new(Kind::Request);
        let response = Editor::new(Kind::Response);

        let editors = gtk::Stack::builder().vexpand(true).build();
        editors.add_named(&request.widget, Some("request"));
        editors.add_named(&response.widget, Some("response"));

        let title = gtk::Label::builder()
            .xalign(0.0)
            .wrap(true)
            .css_classes(["heading"])
            .build();

        let heading = gtk::Box::builder()
            .spacing(10)
            .margin_start(12)
            .margin_end(12)
            .margin_top(10)
            .build();
        heading.append(&gtk::Image::from_icon_name("media-playback-pause-symbolic"));
        heading.append(&title);

        let follow = gtk::CheckButton::builder()
            .child(
                &gtk::Label::builder()
                    .label("Break on the response too")
                    .wrap(true)
                    .xalign(0.0)
                    .build(),
            )
            .tooltip_text("Pause again when the response arrives")
            .build();

        let abort = gtk::Button::builder()
            .label("Abort")
            .tooltip_text("Close the request without sending it on")
            .css_classes(["destructive-action"])
            .build();

        let resume = gtk::Button::builder()
            .label("Resume")
            .tooltip_text("Send the message on, including any edits")
            .css_classes(["suggested-action"])
            .build();

        let actions = gtk::ActionBar::new();
        actions.pack_start(&follow);
        actions.pack_end(&resume);
        actions.pack_end(&abort);

        let widget = gtk::Box::new(gtk::Orientation::Vertical, 0);
        widget.append(&heading);
        widget.append(&editors);
        widget.append(&actions);

        let page = Rc::new(Self {
            widget,
            title,
            editors,
            request,
            response,
            follow,
            service,
            paused: RefCell::default(),
        });

        let weak = Rc::downgrade(&page);
        resume.connect_clicked(move |_| {
            if let Some(page) = weak.upgrade() {
                page.resume();
            }
        });

        let weak = Rc::downgrade(&page);
        abort.connect_clicked(move |_| {
            if let Some(page) = weak.upgrade() {
                page.abort();
            }
        });

        page
    }

    pub fn show(&self, entry: &Arc<Entry>, record: &Record) {
        let Some(stage) = record.paused else {
            self.paused.replace(None);
            return;
        };

        let current = self
            .paused
            .borrow()
            .as_ref()
            .is_some_and(|paused| Arc::ptr_eq(&paused.entry, entry) && paused.stage == stage);

        if current {
            return;
        }

        let mut paused = Paused {
            entry: entry.clone(),
            stage,
            request: None,
            response: None,
        };

        match stage {
            Stage::Request => {
                let draft = record.request.as_ref().map(RequestDraft::capture);

                if let Some(draft) = &draft {
                    self.request.load_request(draft);
                }

                self.title
                    .set_text("Paused before the request was sent to the server");
                self.editors.set_visible_child_name("request");
                self.follow.set_active(false);
                self.follow.set_visible(true);
                paused.request = draft;
            }
            Stage::Response => {
                let draft = record.response.as_ref().map(ResponseDraft::capture);

                if let Some(draft) = &draft {
                    self.response.load_response(draft);
                }

                self.title
                    .set_text("Paused before the response was delivered to the application");
                self.editors.set_visible_child_name("response");
                self.follow.set_visible(false);
                paused.response = draft;
            }
        }

        self.paused.replace(Some(paused));
    }

    pub fn clear(&self) {
        self.paused.replace(None);
    }

    fn resume(&self) {
        let Some((id, verdict)) = self
            .paused
            .borrow()
            .as_ref()
            .map(|paused| (paused.entry.id(), self.verdict(paused)))
        else {
            return;
        };

        match verdict {
            Ok(verdict) => self.service.resume(id, verdict),
            Err(message) => notify(&self.widget, &message),
        }
    }

    fn abort(&self) {
        if let Some(id) = self
            .paused
            .borrow()
            .as_ref()
            .map(|paused| paused.entry.id())
        {
            self.service.resume(id, Verdict::Abort);
        }
    }

    fn verdict(&self, paused: &Paused) -> Result<Verdict, String> {
        Ok(match paused.stage {
            Stage::Request => {
                let draft = self.request.request()?;

                Verdict::Request {
                    draft: (paused.request.as_ref() != Some(&draft)).then_some(draft),
                    break_response: self.follow.is_active(),
                }
            }
            Stage::Response => {
                let draft = self.response.response()?;

                Verdict::Response {
                    draft: (paused.response.as_ref() != Some(&draft)).then_some(draft),
                }
            }
        })
    }
}
