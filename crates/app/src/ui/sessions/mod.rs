mod columns;
mod exchange;

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet, hash_map};
use std::mem;
use std::rc::Rc;
use std::sync::Arc;

use async_channel::Receiver;
use gtk::prelude::*;
use gtk::{gio, glib};
use session::{Entry, Event, Filter};

use super::hints::{Builder, Catalog};

pub use exchange::Exchange;

const FOLLOW_SLACK: f64 = 24.0;

pub struct Sessions {
    pub widget: gtk::ScrolledWindow,
    view: gtk::ColumnView,
    list: gio::ListStore,
    filtered: gtk::FilterListModel,
    sorted: gtk::SortListModel,
    selection: gtk::MultiSelection,
    filter: gtk::CustomFilter,
    query: RefCell<Filter>,
    matches: RefCell<Option<HashSet<u64>>>,
    index: RefCell<HashMap<u64, Exchange>>,
    catalog: RefCell<Option<Rc<Catalog>>>,
    following: Cell<bool>,
    sticking: Cell<bool>,
}

impl Sessions {
    pub fn new(menu: &gio::MenuModel) -> Rc<Self> {
        let list = gio::ListStore::new::<Exchange>();
        let filtered = gtk::FilterListModel::new(Some(list.clone()), None::<gtk::Filter>);

        let view = gtk::ColumnView::builder()
            .show_column_separators(true)
            .reorderable(true)
            .css_classes(["sessions"])
            .build();

        let sorted = gtk::SortListModel::new(Some(filtered.clone()), view.sorter());
        let selection = gtk::MultiSelection::new(Some(sorted.clone()));
        view.set_model(Some(&selection));

        let popover = gtk::PopoverMenu::from_model(Some(menu));
        popover.set_parent(&view);
        popover.set_has_arrow(false);
        popover.set_halign(gtk::Align::Start);
        view.connect_destroy(glib::clone!(
            #[weak]
            popover,
            move |_| popover.unparent()
        ));

        columns::build(&view, &popover, &selection);

        let widget = gtk::ScrolledWindow::builder()
            .child(&view)
            .vexpand(true)
            .hexpand(true)
            .build();

        let sessions = Rc::new(Self {
            widget,
            view,
            list,
            filtered,
            sorted,
            selection,
            filter: gtk::CustomFilter::new(|item| {
                item.downcast_ref::<Exchange>()
                    .is_some_and(Exchange::is_visible)
            }),
            query: RefCell::default(),
            matches: RefCell::default(),
            index: RefCell::default(),
            catalog: RefCell::default(),
            following: Cell::new(true),
            sticking: Cell::new(false),
        });

        let adjustment = sessions.widget.vadjustment();

        adjustment.connect_value_changed(glib::clone!(
            #[weak]
            sessions,
            move |adjustment| sessions.following.set(at_end(adjustment))
        ));

        adjustment.connect_upper_notify(glib::clone!(
            #[weak]
            sessions,
            move |_| sessions.stick()
        ));

        adjustment.connect_page_size_notify(glib::clone!(
            #[weak]
            sessions,
            move |_| sessions.stick()
        ));

        sessions
    }

    fn stick(self: &Rc<Self>) {
        if !self.following.get() || self.sticking.replace(true) {
            return;
        }

        let weak = Rc::downgrade(self);

        glib::idle_add_local_once(move || {
            let Some(sessions) = weak.upgrade() else {
                return;
            };

            sessions.sticking.set(false);

            if sessions.following.get() {
                let adjustment = sessions.widget.vadjustment();
                adjustment.set_value(adjustment.upper() - adjustment.page_size());
            }
        });
    }

    pub fn connect_selection(&self, callback: impl Fn() + 'static) {
        let callback = Rc::new(callback);
        let changed = callback.clone();

        self.selection
            .connect_selection_changed(move |_, _, _| changed());
        self.selection
            .connect_items_changed(move |_, _, removed, _| {
                if removed > 0 {
                    callback();
                }
            });
    }

    pub fn selected_count(&self) -> u64 {
        self.selection.selection().size()
    }

    pub fn first_selected(&self) -> Option<Exchange> {
        let selected = self.selection.selection();

        (!selected.is_empty())
            .then(|| self.selection.item(selected.minimum()))
            .flatten()
            .and_downcast::<Exchange>()
    }

    pub fn set_query(&self, text: &str) {
        self.query.replace(Filter::parse(text));
        self.refilter();
    }

    pub fn set_matches(&self, matches: Option<HashSet<u64>>) {
        self.matches.replace(matches);
        self.refilter();
    }

    pub fn contains(&self, id: u64) -> bool {
        self.index.borrow().contains_key(&id)
    }

    pub fn select(&self, id: u64) -> bool {
        let position = (0..self.sorted.n_items()).find(|&position| {
            self.sorted
                .item(position)
                .and_downcast::<Exchange>()
                .is_some_and(|exchange| exchange.id() == id)
        });

        let Some(position) = position else {
            return false;
        };

        self.view.scroll_to(
            position,
            None::<&gtk::ColumnViewColumn>,
            gtk::ListScrollFlags::SELECT | gtk::ListScrollFlags::FOCUS,
            None,
        );
        true
    }

    fn refilter(&self) {
        for exchange in self.index.borrow().values() {
            exchange.set_visible(self.admits(exchange));
        }

        if self.filtering() {
            self.filtered.set_filter(Some(&self.filter));
            self.filter.changed(gtk::FilterChange::Different);
        } else {
            self.filtered.set_filter(None::<&gtk::Filter>);
        }
    }

    fn admits(&self, exchange: &Exchange) -> bool {
        self.matches
            .borrow()
            .as_ref()
            .is_none_or(|matches| matches.contains(&exchange.id()))
            && self.query.borrow().matches(&exchange.summary())
    }

    fn filtering(&self) -> bool {
        self.matches.borrow().is_some() || !self.query.borrow().is_empty()
    }

    pub fn total(&self) -> u32 {
        self.list.n_items()
    }

    pub fn shown(&self) -> u32 {
        self.sorted.n_items()
    }

    pub fn selected(&self) -> Vec<Exchange> {
        let selected = self.selection.selection();

        (0..selected.size())
            .filter_map(|index| self.selection.item(selected.nth(index as u32)))
            .filter_map(|item| item.downcast::<Exchange>().ok())
            .collect()
    }

    pub fn catalog(&self) -> Rc<Catalog> {
        if let Some(catalog) = self.catalog.borrow().clone() {
            return catalog;
        }

        let mut builder = Builder::default();

        for exchange in self.index.borrow().values() {
            builder.add(&exchange.summary());
        }

        let catalog = Rc::new(builder.build());
        self.catalog.replace(Some(catalog.clone()));
        catalog
    }

    pub fn visible(&self) -> Vec<Arc<Entry>> {
        (0..self.sorted.n_items())
            .filter_map(|position| self.sorted.item(position))
            .filter_map(|item| item.downcast::<Exchange>().ok())
            .map(|exchange| exchange.entry().clone())
            .collect()
    }

    pub fn shortcut(&self, trigger: &str, action: &str) {
        let controller = gtk::ShortcutController::new();

        controller.add_shortcut(gtk::Shortcut::new(
            gtk::ShortcutTrigger::parse_string(trigger),
            Some(gtk::NamedAction::new(action)),
        ));

        self.view.add_controller(controller);
    }

    pub fn seed(&self, entries: Vec<Arc<Entry>>) {
        self.append(entries.into_iter().map(Exchange::new).collect());
    }

    pub fn drain(&self, events: &Receiver<Event>) -> HashSet<u64> {
        let mut added = Vec::new();
        let mut dirty = HashSet::new();

        while let Ok(event) = events.try_recv() {
            self.catalog.replace(None);

            match event {
                Event::Added(entry) => added.push(Exchange::new(entry)),
                Event::Changed(id) => {
                    dirty.insert(id);
                }
                Event::Removed(ids) => {
                    self.append(mem::take(&mut added));
                    self.remove(&ids);
                }
                Event::Cleared => {
                    added.clear();
                    dirty.clear();
                    self.index.borrow_mut().clear();
                    self.list.remove_all();
                }
            }
        }

        self.append(added);

        let index = self.index.borrow();
        let mut flipped = false;

        for exchange in dirty.iter().filter_map(|id| index.get(id)) {
            exchange.refresh();
            flipped |= exchange.set_visible(self.admits(exchange));
        }

        if flipped && self.filtering() {
            self.filter.changed(gtk::FilterChange::Different);
        }

        dirty
    }

    fn append(&self, exchanges: Vec<Exchange>) {
        let mut index = self.index.borrow_mut();

        let exchanges: Vec<Exchange> = exchanges
            .into_iter()
            .filter(|exchange| match index.entry(exchange.id()) {
                hash_map::Entry::Occupied(_) => false,
                hash_map::Entry::Vacant(slot) => {
                    exchange.set_visible(self.admits(exchange));
                    slot.insert(exchange.clone());
                    true
                }
            })
            .collect();

        if exchanges.is_empty() {
            return;
        }

        self.list.extend_from_slice(&exchanges);
    }

    fn remove(&self, ids: &[u64]) {
        let mut index = self.index.borrow_mut();

        for id in ids {
            index.remove(id);
        }

        self.list.retain(|item| {
            item.downcast_ref::<Exchange>()
                .is_some_and(|exchange| ids.binary_search(&exchange.id()).is_err())
        });
    }
}

fn at_end(adjustment: &gtk::Adjustment) -> bool {
    adjustment.value() + adjustment.page_size() >= adjustment.upper() - FOLLOW_SLACK
}
