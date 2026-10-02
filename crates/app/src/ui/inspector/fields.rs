use std::cell::Cell;

use gtk::pango;
use gtk::prelude::*;

use crate::ui::{clear, copy_button, format};

pub struct Fields {
    pub widget: gtk::ScrolledWindow,
    grid: gtk::Grid,
    row: Cell<i32>,
}

impl Fields {
    pub fn new() -> Self {
        let grid = gtk::Grid::builder()
            .column_spacing(18)
            .row_spacing(4)
            .margin_start(12)
            .margin_end(12)
            .margin_top(10)
            .margin_bottom(12)
            .build();

        let widget = gtk::ScrolledWindow::builder()
            .child(&grid)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .build();

        Self {
            widget,
            grid,
            row: Cell::new(0),
        }
    }

    pub fn clear(&self) {
        clear(&self.grid);
        self.row.set(0);
    }

    pub fn line(&self, text: &str) {
        let label = selectable(text);
        label.add_css_class("field-name");
        self.attach(&label, 0, 2);
    }

    pub fn section(&self, title: &str, fields: &[(String, String)], separator: &str) {
        if fields.is_empty() {
            return;
        }

        let label = gtk::Label::builder()
            .label(format!("{title} ({})", fields.len()))
            .xalign(0.0)
            .css_classes(["heading"])
            .build();

        let text: String = fields
            .iter()
            .map(|(name, value)| {
                format!(
                    "{}{separator}{}\n",
                    format::printable(name),
                    format::printable(value)
                )
            })
            .collect();

        let header = gtk::Box::builder()
            .spacing(6)
            .css_classes(["section-title"])
            .build();
        header.append(&label);
        header.append(&copy_button(&format!("Copy {title}"), move || text.clone()));

        self.attach(&header, 0, 2);

        for (name, value) in fields {
            self.field(name, value);
        }
    }

    fn field(&self, name: &str, value: &str) {
        let row = self.row.get();

        let name = selectable(name);
        name.set_css_classes(&["field-name", "dimmed"]);

        let value = selectable(value);
        value.add_css_class("field-value");
        value.set_hexpand(true);

        self.grid.attach(&name, 0, row, 1, 1);
        self.grid.attach(&value, 1, row, 1, 1);
        self.row.set(row + 1);
    }

    fn attach(&self, widget: &impl IsA<gtk::Widget>, column: i32, width: i32) {
        let row = self.row.get();
        self.grid.attach(widget, column, row, width, 1);
        self.row.set(row + 1);
    }
}

fn selectable(text: &str) -> gtk::Label {
    gtk::Label::builder()
        .label(format::printable(text))
        .selectable(true)
        .wrap(true)
        .wrap_mode(pango::WrapMode::WordChar)
        .xalign(0.0)
        .yalign(0.0)
        .build()
}
