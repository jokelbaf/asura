use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use gtk::prelude::*;
use gtk::{gio, glib, pango};
use session::{Direction, Entry, Frame, Opcode, Record};

use super::body::BodyView;
use super::content::Part;
use crate::ui::format;

const PREVIEW: usize = 160;

struct Message {
    index: usize,
    outbound: bool,
    time: String,
    kind: String,
    size: String,
    preview: String,
}

pub struct FramesPage {
    pub widget: gtk::Paned,
    list: gio::ListStore,
    selection: gtk::SingleSelection,
    pub body: Rc<BodyView>,
    entry: RefCell<Option<Arc<Entry>>>,
}

impl FramesPage {
    pub fn new() -> Rc<Self> {
        let list = gio::ListStore::new::<glib::BoxedAnyObject>();
        let selection = gtk::SingleSelection::builder()
            .model(&list)
            .autoselect(false)
            .can_unselect(true)
            .build();

        let view = gtk::ColumnView::builder()
            .model(&selection)
            .show_column_separators(true)
            .build();

        view.append_column(&icon_column());
        view.append_column(&column("Time", false, |message| message.time.clone()));
        view.append_column(&column("Type", false, |message| message.kind.clone()));
        view.append_column(&column("Size", false, |message| message.size.clone()));
        view.append_column(&column("Data", true, |message| message.preview.clone()));

        let body = BodyView::new("");

        let widget = gtk::Paned::builder()
            .orientation(gtk::Orientation::Vertical)
            .start_child(
                &gtk::ScrolledWindow::builder()
                    .child(&view)
                    .vexpand(true)
                    .build(),
            )
            .end_child(&body.widget)
            .shrink_start_child(false)
            .shrink_end_child(false)
            .position(260)
            .wide_handle(true)
            .build();

        let page = Rc::new(Self {
            widget,
            list,
            selection,
            body,
            entry: RefCell::default(),
        });

        let weak = Rc::downgrade(&page);
        page.selection.connect_selected_notify(move |_| {
            if let Some(page) = weak.upgrade() {
                page.select();
            }
        });

        page
    }

    pub fn show(&self, entry: &Arc<Entry>, record: &Record) {
        let same = self
            .entry
            .borrow()
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, entry));

        if !same {
            self.entry.replace(Some(entry.clone()));
            self.list.remove_all();
            self.body.clear();
        }

        let start = self.list.n_items() as usize;
        let messages: Vec<glib::BoxedAnyObject> = record
            .frames
            .iter()
            .enumerate()
            .skip(start)
            .map(|(index, frame)| glib::BoxedAnyObject::new(message(index, frame)))
            .collect();

        self.list.extend_from_slice(&messages);
    }

    pub fn clear(&self) {
        self.entry.replace(None);
        self.list.remove_all();
        self.body.clear();
    }

    fn select(&self) {
        let Some(entry) = self.entry.borrow().clone() else {
            return;
        };

        match self
            .selection
            .selected_item()
            .and_downcast::<glib::BoxedAnyObject>()
        {
            Some(item) => self
                .body
                .show(&entry, Part::Frame(item.borrow::<Message>().index)),
            None => self.body.clear(),
        }
    }
}

fn message(index: usize, frame: &Frame) -> Message {
    let kind = match frame.opcode {
        Opcode::Text => String::from("Text"),
        Opcode::Binary => String::from("Binary"),
        Opcode::Close => String::from("Close"),
        Opcode::Ping => String::from("Ping"),
        Opcode::Pong => String::from("Pong"),
        Opcode::Continuation => String::from("Continuation"),
        Opcode::Reserved(code) => format!("Reserved {code:#x}"),
    };

    let head = &frame.payload[..frame.payload.len().min(PREVIEW)];
    let preview = match frame.opcode {
        Opcode::Text => String::from_utf8_lossy(head)
            .replace(['\n', '\r'], " ")
            .replace('\0', "\u{2400}"),
        Opcode::Close if head.len() >= 2 => format!(
            "{} {}",
            u16::from_be_bytes([head[0], head[1]]),
            format::printable(&String::from_utf8_lossy(&head[2..]))
        ),
        _ => head
            .iter()
            .take(32)
            .map(|byte| format!("{byte:02x}"))
            .collect::<Vec<_>>()
            .join(" "),
    };

    Message {
        index,
        outbound: frame.direction == Direction::Outbound,
        time: format::clock(frame.time),
        kind,
        size: format::size(frame.payload.len() as u64),
        preview,
    }
}

fn icon_column() -> gtk::ColumnViewColumn {
    let factory = gtk::SignalListItemFactory::new();

    factory.connect_setup(|_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().expect("list item");
        item.set_child(Some(&gtk::Image::new()));
    });

    factory.connect_bind(|_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().expect("list item");
        let (Some(image), Some(message)) = (
            item.child().and_downcast::<gtk::Image>(),
            item.item().and_downcast::<glib::BoxedAnyObject>(),
        ) else {
            return;
        };

        let outbound = message.borrow::<Message>().outbound;
        image.set_icon_name(Some(if outbound {
            "go-up-symbolic"
        } else {
            "go-down-symbolic"
        }));
        image.set_tooltip_text(Some(if outbound { "Sent" } else { "Received" }));
    });

    gtk::ColumnViewColumn::builder()
        .factory(&factory)
        .fixed_width(36)
        .build()
}

fn column(title: &str, expand: bool, text: fn(&Message) -> String) -> gtk::ColumnViewColumn {
    let factory = gtk::SignalListItemFactory::new();

    factory.connect_setup(|_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().expect("list item");
        let label = gtk::Label::builder()
            .xalign(0.0)
            .single_line_mode(true)
            .ellipsize(pango::EllipsizeMode::End)
            .build();
        item.set_child(Some(&label));
    });

    factory.connect_bind(move |_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().expect("list item");
        let (Some(label), Some(message)) = (
            item.child().and_downcast::<gtk::Label>(),
            item.item().and_downcast::<glib::BoxedAnyObject>(),
        ) else {
            return;
        };

        label.set_text(&text(&message.borrow::<Message>()));
    });

    gtk::ColumnViewColumn::builder()
        .title(title)
        .factory(&factory)
        .expand(expand)
        .resizable(true)
        .build()
}
