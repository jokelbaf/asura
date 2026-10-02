use std::net::SocketAddr;
use std::rc::Rc;
use std::sync::Arc;

use adw::prelude::*;
use gtk::glib;
use proxy::{Outgoing, RequestDraft};
use session::Headers;

use super::editor::{Editor, Kind};
use super::{hints, keep};
use crate::service::Service;

pub struct Origin {
    pub authority: String,
    pub address: SocketAddr,
}

struct Composer {
    dialog: adw::Dialog,
    toasts: adw::ToastOverlay,
    editor: Rc<Editor>,
    send: gtk::Button,
    service: Arc<Service>,
    origin: Option<Origin>,
    sent: Box<dyn Fn(u64)>,
}

pub fn present(
    parent: &impl IsA<gtk::Widget>,
    service: Arc<Service>,
    source: Option<(RequestDraft, Option<Origin>)>,
    sent: impl Fn(u64) + 'static,
) {
    let editor = Editor::new(Kind::Url);
    let title = if source.is_some() {
        "Edit and Resend"
    } else {
        "Compose Request"
    };

    let (draft, origin) = source.unwrap_or_else(|| (blank(), None));
    editor.load_request(&draft);
    editor.suggest(hints::snapshot(service.store()));

    let send = gtk::Button::builder()
        .label("Send")
        .css_classes(["suggested-action"])
        .build();

    let header = adw::HeaderBar::new();
    header.pack_end(&send);

    let toasts = adw::ToastOverlay::new();
    toasts.set_child(Some(&editor.widget));

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&toasts));

    let dialog = adw::Dialog::builder()
        .title(title)
        .content_width(820)
        .content_height(680)
        .child(&toolbar)
        .build();

    let composer = Rc::new(Composer {
        dialog,
        toasts,
        editor,
        send,
        service,
        origin,
        sent: Box::new(sent),
    });

    composer.send.connect_clicked(glib::clone!(
        #[weak]
        composer,
        move |_| composer.submit()
    ));

    composer.dialog.present(Some(parent));
    composer.editor.focus();
    keep(&composer.dialog, composer.clone());
}

impl Composer {
    fn submit(self: &Rc<Self>) {
        let draft = match self.editor.request() {
            Ok(draft) => draft,
            Err(message) => return self.toast(&message),
        };

        let address = self
            .origin
            .as_ref()
            .filter(|origin| authority(&draft.target).eq_ignore_ascii_case(&origin.authority))
            .map(|origin| origin.address);

        let outgoing = Outgoing {
            method: draft.method,
            url: draft.target,
            headers: draft.headers,
            body: draft.body,
            address,
        };

        self.send.set_sensitive(false);
        let composer = self.clone();

        glib::spawn_future_local(async move {
            match composer.service.send(outgoing).await {
                Ok(entry) => {
                    (composer.sent)(entry.id());
                    composer.dialog.close();
                }
                Err(error) => {
                    composer.send.set_sensitive(true);
                    composer.toast(&format!("Could not send the request: {error}"));
                }
            }
        });
    }

    fn toast(&self, message: &str) {
        self.toasts.add_toast(
            adw::Toast::builder()
                .use_markup(false)
                .title(message)
                .timeout(5)
                .build(),
        );
    }
}

fn blank() -> RequestDraft {
    let mut headers = Headers::default();
    headers.push("accept", "*/*");

    RequestDraft {
        method: String::from("GET"),
        target: String::from("https://"),
        headers,
        body: Vec::new(),
    }
}

fn authority(url: &str) -> &str {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    rest.split(['/', '?', '#']).next().unwrap_or_default()
}
