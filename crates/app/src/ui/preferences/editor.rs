use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use gtk::glib;

use crate::ui::keep;

pub struct Editor {
    group: adw::PreferencesGroup,
    entry: adw::EntryRow,
    rows: RefCell<Vec<adw::ActionRow>>,
    items: RefCell<Vec<String>>,
    valid: fn(&str) -> bool,
    changed: Box<dyn Fn(Vec<String>)>,
}

impl Editor {
    pub fn attach(
        dialog: &adw::PreferencesDialog,
        group: &adw::PreferencesGroup,
        placeholder: &str,
        items: Vec<String>,
        valid: fn(&str) -> bool,
        changed: impl Fn(Vec<String>) + 'static,
    ) -> adw::EntryRow {
        let entry = adw::EntryRow::builder()
            .title(placeholder)
            .show_apply_button(true)
            .build();

        group.add(&entry);

        let editor = Rc::new(Self {
            group: group.clone(),
            entry,
            rows: RefCell::default(),
            items: RefCell::new(items),
            valid,
            changed: Box::new(changed),
        });

        editor.entry.connect_apply(glib::clone!(
            #[weak]
            editor,
            move |_| editor.add()
        ));

        editor.entry.connect_changed(glib::clone!(
            #[weak]
            editor,
            move |entry| {
                let text = entry.text();
                let invalid = !text.trim().is_empty() && !(editor.valid)(text.trim());

                if invalid {
                    entry.add_css_class("error");
                } else {
                    entry.remove_css_class("error");
                }
            }
        ));

        editor.render();

        let entry = editor.entry.clone();
        keep(dialog, editor);
        entry
    }

    fn add(self: &Rc<Self>) {
        let text = self.entry.text().trim().to_owned();

        if !(self.valid)(&text) || self.items.borrow().contains(&text) {
            return self.entry.add_css_class("error");
        }

        self.items.borrow_mut().push(text);
        self.entry.set_text("");
        self.commit();
    }

    fn remove(self: &Rc<Self>, item: &str) {
        self.items.borrow_mut().retain(|existing| existing != item);
        self.commit();
    }

    fn commit(self: &Rc<Self>) {
        (self.changed)(self.items.borrow().clone());
        self.render();
    }

    fn render(self: &Rc<Self>) {
        for row in self.rows.borrow_mut().drain(..) {
            self.group.remove(&row);
        }

        for item in self.items.borrow().iter() {
            let row = adw::ActionRow::builder().use_markup(false).build();
            row.set_title(item);
            let remove = gtk::Button::builder()
                .icon_name("user-trash-symbolic")
                .tooltip_text("Remove")
                .valign(gtk::Align::Center)
                .css_classes(["flat"])
                .build();

            let weak = Rc::downgrade(self);
            let item = item.clone();
            remove.connect_clicked(move |_| {
                if let Some(editor) = weak.upgrade() {
                    editor.remove(&item);
                }
            });

            row.add_suffix(&remove);
            self.group.add(&row);
            self.rows.borrow_mut().push(row);
        }
    }
}
