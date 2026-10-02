use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use gtk::prelude::*;
use gtk::{gdk, glib, pango};

use super::format;

const HEIGHT: i32 = 320;
const WIDTH: i32 = 300;

pub struct Suggestion {
    pub label: String,
    pub detail: String,
    pub insert: String,
    pub start: usize,
    pub end: usize,
    pub more: bool,
}

type Provider = dyn Fn(&str, usize) -> Vec<Suggestion>;

pub struct Completion {
    anchor: glib::WeakRef<gtk::Widget>,
    editable: glib::WeakRef<gtk::Editable>,
    focus: gtk::EventControllerFocus,
    popover: gtk::Popover,
    scroller: gtk::ScrolledWindow,
    list: gtk::ListBox,
    items: RefCell<Vec<Suggestion>>,
    provider: Box<Provider>,
    quiet: Cell<bool>,
}

impl Completion {
    pub fn attach(
        widget: &(impl IsA<gtk::Editable> + IsA<gtk::Widget>),
        provider: impl Fn(&str, usize) -> Vec<Suggestion> + 'static,
    ) {
        let list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::Single)
            .activate_on_single_click(true)
            .can_focus(false)
            .focusable(false)
            .css_classes(["navigation-sidebar"])
            .build();

        let scroller = gtk::ScrolledWindow::builder()
            .child(&list)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .propagate_natural_height(true)
            .max_content_height(HEIGHT)
            .build();

        let popover = gtk::Popover::builder()
            .child(&scroller)
            .autohide(false)
            .has_arrow(false)
            .can_focus(false)
            .position(gtk::PositionType::Bottom)
            .css_classes(["completion"])
            .build();
        popover.set_parent(widget);

        let focus = gtk::EventControllerFocus::new();
        widget.add_controller(focus.clone());

        let completion = Rc::new(Self {
            anchor: widget.upcast_ref::<gtk::Widget>().downgrade(),
            editable: widget.upcast_ref::<gtk::Editable>().downgrade(),
            focus,
            popover,
            scroller,
            list,
            items: RefCell::default(),
            provider: Box::new(provider),
            quiet: Cell::new(false),
        });

        completion.connect(widget);

        let held = completion.clone();
        widget.connect_destroy(move |_| held.popover.unparent());
    }

    fn connect(self: &Rc<Self>, widget: &(impl IsA<gtk::Editable> + IsA<gtk::Widget>)) {
        let weak = Rc::downgrade(self);
        widget.connect_changed(move |_| {
            if let Some(completion) = weak.upgrade().filter(|completion| !completion.quiet.get()) {
                completion.later(false);
            }
        });

        let weak = Rc::downgrade(self);
        self.focus.connect_leave(move |_| {
            if let Some(completion) = weak.upgrade() {
                completion.hide();
            }
        });

        let weak = Rc::downgrade(self);
        self.list.connect_row_activated(move |_, row| {
            if let Some(completion) = weak.upgrade() {
                completion.apply(row.index());
            }
        });

        let click = gtk::GestureClick::new();
        click.set_propagation_phase(gtk::PropagationPhase::Capture);

        let weak = Rc::downgrade(self);
        click.connect_pressed(move |_, _, _, _| {
            if let Some(completion) = weak.upgrade() {
                completion.invite();
            }
        });

        widget.add_controller(click);

        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);

        let weak = Rc::downgrade(self);
        keys.connect_key_pressed(move |_, key, _, modifiers| match weak.upgrade() {
            Some(completion) => completion.key(key, modifiers),
            None => glib::Propagation::Proceed,
        });

        widget.add_controller(keys);
    }

    fn key(self: &Rc<Self>, key: gdk::Key, modifiers: gdk::ModifierType) -> glib::Propagation {
        let open = self.popover.is_visible();
        let selected = self.list.selected_row().map(|row| row.index());

        match key {
            gdk::Key::space if modifiers.contains(gdk::ModifierType::CONTROL_MASK) => {
                self.refresh(true)
            }
            gdk::Key::Down | gdk::Key::KP_Down if !open => self.refresh(true),
            gdk::Key::Down | gdk::Key::KP_Down => self.step(1),
            gdk::Key::Up | gdk::Key::KP_Up if open => self.step(-1),
            gdk::Key::Page_Down if open => self.step(8),
            gdk::Key::Page_Up if open => self.step(-8),
            gdk::Key::Tab | gdk::Key::Return | gdk::Key::KP_Enter | gdk::Key::ISO_Enter if open => {
                self.apply(selected.unwrap_or_default())
            }
            gdk::Key::Escape if open => self.hide(),
            _ => return glib::Propagation::Proceed,
        }

        glib::Propagation::Stop
    }

    fn invite(self: &Rc<Self>) {
        match self.editable.upgrade() {
            Some(editable) if editable.text().is_empty() => self.later(true),
            _ => self.hide(),
        }
    }

    fn later(self: &Rc<Self>, force: bool) {
        let weak: Weak<Self> = Rc::downgrade(self);

        glib::idle_add_local_once(move || {
            if let Some(completion) = weak.upgrade() {
                completion.refresh(force);
            }
        });
    }

    fn refresh(&self, force: bool) {
        let (Some(anchor), Some(editable)) = (self.anchor.upgrade(), self.editable.upgrade())
        else {
            return;
        };

        let text = editable.text();

        if !self.focus.contains_focus() || (text.is_empty() && !force) {
            return self.hide();
        }

        let cursor = usize::try_from(editable.position()).unwrap_or_default();
        let items = (self.provider)(&text, cursor);

        if items.is_empty() {
            return self.hide();
        }

        self.list.remove_all();

        for item in &items {
            self.list.append(&row(item));
        }

        self.items.replace(items);
        self.list.select_row(self.list.row_at_index(0).as_ref());
        self.scroller.vadjustment().set_value(0.0);
        self.scroller
            .set_size_request(anchor.width().max(WIDTH), -1);

        if self.popover.is_visible() {
            self.popover.present();
        } else {
            self.popover.popup();
        }
    }

    fn step(&self, offset: i32) {
        let count = self.items.borrow().len() as i32;
        let current = self.list.selected_row().map_or(-1, |row| row.index());
        let next = (current + offset).clamp(0, count - 1);

        let Some(row) = self.list.row_at_index(next) else {
            return;
        };

        self.list.select_row(Some(&row));

        if let Some(bounds) = row.compute_bounds(&self.list) {
            let top = f64::from(bounds.y());
            self.scroller
                .vadjustment()
                .clamp_page(top, top + f64::from(bounds.height()));
        }
    }

    fn apply(self: &Rc<Self>, index: i32) {
        let Some(editable) = self.editable.upgrade() else {
            return;
        };

        let Some((text, position, more)) = self
            .items
            .borrow()
            .get(usize::try_from(index).unwrap_or_default())
            .map(|item| splice(&editable.text(), item))
        else {
            return;
        };

        self.quiet.set(true);
        editable.set_text(&text);
        editable.set_position(position);
        self.quiet.set(false);

        if more {
            self.later(true);
        } else {
            self.hide();
        }
    }

    fn hide(&self) {
        if self.popover.is_visible() {
            self.popover.popdown();
        }
    }
}

fn splice(text: &str, item: &Suggestion) -> (String, i32, bool) {
    let characters: Vec<char> = text.chars().collect();
    let start = item.start.min(characters.len());
    let end = item.end.clamp(start, characters.len());

    let mut spliced: String = characters[..start].iter().collect();
    spliced.push_str(&item.insert);
    let position = spliced.chars().count() as i32;
    spliced.extend(&characters[end..]);

    (spliced, position, item.more)
}

fn row(item: &Suggestion) -> gtk::ListBoxRow {
    let label = gtk::Label::builder()
        .label(format::printable(&item.label))
        .xalign(0.0)
        .hexpand(true)
        .ellipsize(pango::EllipsizeMode::Middle)
        .css_classes(["monospace"])
        .build();

    let detail = gtk::Label::builder()
        .label(format::printable(&item.detail))
        .xalign(1.0)
        .ellipsize(pango::EllipsizeMode::End)
        .css_classes(["dimmed", "caption"])
        .build();

    let content = gtk::Box::builder().spacing(18).build();
    content.append(&label);
    content.append(&detail);

    gtk::ListBoxRow::builder()
        .child(&content)
        .can_focus(false)
        .focusable(false)
        .build()
}
