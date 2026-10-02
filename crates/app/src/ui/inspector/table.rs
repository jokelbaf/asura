use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use gtk::prelude::*;
use gtk::{gio, glib, pango};
use sourceview::prelude::*;

use super::content::{self, Content, Row, TEXT_LIMIT, View};
use crate::ui::{format, scheme};

const HEX_LIMIT: usize = 64 << 10;

type Shared = Rc<RefCell<Option<Arc<Content>>>>;

pub struct TableView {
    pub widget: gtk::Paned,
    view: gtk::ColumnView,
    list: gio::ListStore,
    selection: gtk::SingleSelection,
    detail: sourceview::Buffer,
    content: Shared,
    columns: RefCell<Vec<&'static str>>,
}

impl TableView {
    pub fn new() -> Rc<Self> {
        let list = gio::ListStore::new::<glib::BoxedAnyObject>();
        let selection = gtk::SingleSelection::builder()
            .model(&list)
            .autoselect(true)
            .build();

        let view = gtk::ColumnView::builder()
            .model(&selection)
            .show_column_separators(true)
            .build();

        let detail = sourceview::Buffer::new(None);
        detail.set_highlight_matching_brackets(false);
        scheme(&detail);

        let text = sourceview::View::builder()
            .buffer(&detail)
            .editable(false)
            .monospace(true)
            .wrap_mode(gtk::WrapMode::WordChar)
            .top_margin(6)
            .bottom_margin(6)
            .left_margin(6)
            .right_margin(6)
            .css_classes(["body-view"])
            .build();

        let widget = gtk::Paned::builder()
            .orientation(gtk::Orientation::Vertical)
            .start_child(
                &gtk::ScrolledWindow::builder()
                    .child(&view)
                    .vexpand(true)
                    .build(),
            )
            .end_child(
                &gtk::ScrolledWindow::builder()
                    .child(&text)
                    .vexpand(true)
                    .build(),
            )
            .shrink_start_child(false)
            .shrink_end_child(false)
            .position(200)
            .wide_handle(true)
            .build();

        let table = Rc::new(Self {
            widget,
            view,
            list,
            selection,
            detail,
            content: Shared::default(),
            columns: RefCell::default(),
        });

        let weak = Rc::downgrade(&table);
        table.selection.connect_selected_notify(move |_| {
            if let Some(table) = weak.upgrade() {
                table.select();
            }
        });

        table
    }

    pub fn show(&self, content: &Arc<Content>) {
        let Some(View::Table(table)) = content.structure.as_ref().map(|structure| &structure.view)
        else {
            return;
        };

        let selected = self.selection.selected();
        self.content.replace(Some(content.clone()));

        if *self.columns.borrow() != table.columns {
            self.rebuild(&table.columns);
        }

        let items: Vec<glib::BoxedAnyObject> = (0..table.rows.len())
            .map(glib::BoxedAnyObject::new)
            .collect();
        self.list.splice(0, self.list.n_items(), &items);

        if (selected as usize) < items.len() {
            self.selection.set_selected(selected);
        }

        self.select();
    }

    pub fn clear(&self) {
        self.content.replace(None);
        self.list.remove_all();
        self.detail.set_text("");
    }

    fn select(&self) {
        let content = self.content.borrow();
        let index = self.selection.selected() as usize;

        match content.as_deref().and_then(|content| row(content, index)) {
            Some(row) => self.detail(row),
            None => self.detail.set_text(""),
        }
    }

    fn detail(&self, row: &Row) {
        let shown = &row.detail[..row.detail.len().min(TEXT_LIMIT)];

        let (text, language) = if content::textual(&row.kind, shown) {
            let language = content::language(&row.kind, shown);
            let text = content::pretty(language, shown)
                .unwrap_or_else(|| String::from_utf8_lossy(shown).into_owned());
            (text, language)
        } else {
            (
                formats::hexdump(&row.detail[..row.detail.len().min(HEX_LIMIT)]),
                None,
            )
        };

        self.detail.set_language(
            language
                .and_then(|id| sourceview::LanguageManager::default().language(id))
                .as_ref(),
        );
        self.detail.set_text(&format::printable(&text));
    }

    fn rebuild(&self, columns: &[&'static str]) {
        while let Some(column) = self
            .view
            .columns()
            .item(0)
            .and_downcast::<gtk::ColumnViewColumn>()
        {
            self.view.remove_column(&column);
        }

        for (index, title) in columns.iter().enumerate() {
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

            let shared = self.content.clone();
            factory.connect_bind(move |_, item| {
                let item = item.downcast_ref::<gtk::ListItem>().expect("list item");
                let (Some(label), Some(position)) = (
                    item.child().and_downcast::<gtk::Label>(),
                    item.item().and_downcast::<glib::BoxedAnyObject>(),
                ) else {
                    return;
                };

                let content = shared.borrow();
                let text = content
                    .as_deref()
                    .and_then(|content| row(content, *position.borrow::<usize>()))
                    .and_then(|row| row.cells.get(index))
                    .map(String::as_str)
                    .unwrap_or_default();

                label.set_text(text);
            });

            let column = gtk::ColumnViewColumn::builder()
                .title(*title)
                .factory(&factory)
                .resizable(true)
                .expand(index + 1 == columns.len())
                .build();

            self.view.append_column(&column);
        }

        self.columns.replace(columns.to_vec());
    }
}

fn row(content: &Content, index: usize) -> Option<&Row> {
    match &content.structure.as_ref()?.view {
        View::Table(table) => table.rows.get(index),
        View::Tree(_) => None,
    }
}
