use std::cell::RefCell;
use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::Arc;

use adw::prelude::*;

use crate::service::{Service, Settings};
use crate::ui::hints::{self, Catalog};
use crate::ui::{format, keep, notify};

pub trait Rule: Clone + 'static {
    const ADDED: &'static str;

    fn pattern(&self) -> &str;

    fn enabled(&self) -> bool;

    fn set_enabled(&mut self, enabled: bool);

    fn summary(&self) -> String;

    fn all(settings: &Settings) -> Vec<Self>;

    fn store(settings: &mut Settings, rules: Vec<Self>);

    fn edit(parent: &gtk::Widget, rule: Option<Self>, catalog: Rc<Catalog>, done: Rc<dyn Fn(Self)>);
}

pub struct List<T: Rule> {
    group: adw::PreferencesGroup,
    add: adw::ButtonRow,
    rows: RefCell<Vec<adw::ActionRow>>,
    service: Arc<Service>,
    kind: PhantomData<T>,
}

impl<T: Rule> List<T> {
    pub fn attach(
        dialog: &adw::PreferencesDialog,
        group: &adw::PreferencesGroup,
        service: Arc<Service>,
    ) {
        let add = adw::ButtonRow::builder().title("Add Rule…").build();

        let list = Rc::new(Self {
            group: group.clone(),
            add,
            rows: RefCell::default(),
            service,
            kind: PhantomData,
        });

        let weak = Rc::downgrade(&list);
        list.add.connect_activated(move |_| {
            if let Some(list) = weak.upgrade() {
                list.edit(None);
            }
        });

        list.render();
        keep(dialog, list);
    }

    fn render(self: &Rc<Self>) {
        for row in self.rows.borrow_mut().drain(..) {
            self.group.remove(&row);
        }

        if self.add.parent().is_some() {
            self.group.remove(&self.add);
        }

        let rules = T::all(&self.service.settings());

        for (index, rule) in rules.iter().enumerate() {
            let row = adw::ActionRow::builder()
                .use_markup(false)
                .activatable(true)
                .build();
            row.set_title(&format::printable(rule.pattern()));
            row.set_subtitle(&rule.summary());

            let enabled = gtk::Switch::builder()
                .active(rule.enabled())
                .valign(gtk::Align::Center)
                .tooltip_text("Enabled")
                .build();

            let edit = button("document-edit-symbolic", "Edit");
            let remove = button("user-trash-symbolic", "Remove");

            let weak = Rc::downgrade(self);
            enabled.connect_active_notify(move |switch| {
                let active = switch.is_active();

                if let Some(list) = weak.upgrade() {
                    list.change(false, |rules| {
                        if let Some(rule) = rules.get_mut(index) {
                            rule.set_enabled(active);
                        }
                    });
                }
            });

            let weak = Rc::downgrade(self);
            edit.connect_clicked(move |_| {
                if let Some(list) = weak.upgrade() {
                    list.edit(Some(index));
                }
            });

            let weak = Rc::downgrade(self);
            row.connect_activated(move |_| {
                if let Some(list) = weak.upgrade() {
                    list.edit(Some(index));
                }
            });

            let weak = Rc::downgrade(self);
            remove.connect_clicked(move |_| {
                if let Some(list) = weak.upgrade() {
                    list.change(true, |rules| {
                        if index < rules.len() {
                            rules.remove(index);
                        }
                    });
                }
            });

            row.add_prefix(&enabled);
            row.add_suffix(&edit);
            row.add_suffix(&remove);
            self.group.add(&row);
            self.rows.borrow_mut().push(row);
        }

        self.group.add(&self.add);
    }

    fn edit(self: &Rc<Self>, index: Option<usize>) {
        let current = index.and_then(|index| T::all(&self.service.settings()).get(index).cloned());
        let weak = Rc::downgrade(self);

        T::edit(
            self.group.upcast_ref(),
            current,
            hints::snapshot(self.service.store()),
            Rc::new(move |rule| {
                let Some(list) = weak.upgrade() else {
                    return;
                };

                list.change(true, |rules| match index {
                    Some(index) if index < rules.len() => rules[index] = rule,
                    _ => rules.push(rule),
                });
            }),
        );
    }

    fn change(self: &Rc<Self>, render: bool, change: impl FnOnce(&mut Vec<T>)) {
        let result = self.service.update(|settings| {
            let mut rules = T::all(settings);
            change(&mut rules);
            T::store(settings, rules);
        });

        if let Err(error) = result {
            notify(&self.group, &format!("{error:#}"));
        }

        if render {
            self.render();
        }
    }
}

fn button(icon: &str, tooltip: &str) -> gtk::Button {
    gtk::Button::builder()
        .icon_name(icon)
        .tooltip_text(tooltip)
        .valign(gtk::Align::Center)
        .css_classes(["flat"])
        .build()
}
