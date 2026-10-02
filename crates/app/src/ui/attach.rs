use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use adw::prelude::*;
use gtk::glib;

use crate::service::Service;
use crate::service::processes::{self, Process};
use crate::ui::{keep, notify, offer};

struct Picker {
    dialog: adw::Dialog,
    search: gtk::SearchEntry,
    list: gtk::ListBox,
    stack: gtk::Stack,
    processes: RefCell<Vec<Process>>,
    service: Arc<Service>,
}

pub fn present(parent: &impl IsA<gtk::Widget>, service: Arc<Service>) {
    let search = gtk::SearchEntry::builder()
        .placeholder_text("Search by name, command line or PID")
        .hexpand(true)
        .build();

    let refresh = gtk::Button::builder()
        .icon_name("view-refresh-symbolic")
        .tooltip_text("Refresh")
        .build();

    let header = adw::HeaderBar::new();
    header.pack_start(&refresh);

    let bar = gtk::Box::builder()
        .margin_start(12)
        .margin_end(12)
        .margin_bottom(6)
        .build();
    bar.append(&search);

    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .margin_start(12)
        .margin_end(12)
        .margin_top(6)
        .margin_bottom(12)
        .valign(gtk::Align::Start)
        .build();

    let loading = adw::Spinner::builder()
        .width_request(32)
        .height_request(32)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .build();

    let stack = gtk::Stack::new();
    stack.add_named(&loading, Some("loading"));
    stack.add_named(
        &gtk::ScrolledWindow::builder()
            .child(&list)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .build(),
        Some("list"),
    );

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.add_top_bar(&bar);
    toolbar.set_content(Some(&stack));

    let dialog = adw::Dialog::builder()
        .title("Attach to Process")
        .content_width(620)
        .content_height(640)
        .child(&toolbar)
        .build();

    let picker = Rc::new(Picker {
        dialog,
        search,
        list,
        stack,
        processes: RefCell::default(),
        service,
    });

    let weak = Rc::downgrade(&picker);
    picker.list.set_filter_func(move |row| {
        weak.upgrade()
            .is_none_or(|picker| picker.matches(row.index()))
    });

    picker.search.connect_search_changed(glib::clone!(
        #[weak]
        picker,
        move |_| picker.list.invalidate_filter()
    ));

    refresh.connect_clicked(glib::clone!(
        #[weak]
        picker,
        move |_| picker.load()
    ));

    picker.list.connect_row_activated(glib::clone!(
        #[weak]
        picker,
        move |_, row| picker.attach(row.index())
    ));

    picker.search.set_key_capture_widget(Some(&picker.dialog));
    picker.load();
    picker.dialog.present(Some(parent));
    picker.search.grab_focus();
    keep(&picker.dialog, picker.clone());
}

impl Picker {
    fn load(self: &Rc<Self>) {
        self.stack.set_visible_child_name("loading");
        let picker = self.clone();

        glib::spawn_future_local(async move {
            let mut found = picker.service.background(processes::list).await;
            found.sort_by_cached_key(|process| {
                (
                    !process.is_windows(),
                    process.name.to_lowercase(),
                    process.pid,
                )
            });

            picker.list.remove_all();

            for process in &found {
                picker.list.append(&row(process));
            }

            picker.processes.replace(found);
            picker.stack.set_visible_child_name("list");
        });
    }

    fn matches(&self, index: i32) -> bool {
        let query = self.search.text().to_lowercase();
        let processes = self.processes.borrow();

        let Some(process) = usize::try_from(index)
            .ok()
            .and_then(|index| processes.get(index))
        else {
            return true;
        };

        query.is_empty()
            || process.pid.to_string() == query
            || process.name.to_lowercase().contains(&query)
            || process.command_line().to_lowercase().contains(&query)
    }

    fn attach(self: &Rc<Self>, index: i32) {
        let Some(process) = usize::try_from(index)
            .ok()
            .and_then(|index| self.processes.borrow().get(index).cloned())
        else {
            return;
        };

        let picker = self.clone();
        self.dialog.set_sensitive(false);

        glib::spawn_future_local(async move {
            let service = picker.service.clone();
            let result = picker
                .service
                .background(move || service.attach(process.pid))
                .await;

            match result {
                Ok(name) if picker.service.is_capturing() => {
                    notify(&picker.dialog, &format!("Capturing {name}"));
                    picker.dialog.close();
                }
                Ok(name) => {
                    offer(
                        &picker.dialog,
                        &format!("Attached to {name}. Start capturing to record its traffic"),
                        "Start",
                        "win.start",
                    );
                    picker.dialog.close();
                }
                Err(error) => {
                    notify(&picker.dialog, &format!("Could not attach: {error:#}"));
                    picker.dialog.set_sensitive(true);
                }
            }
        });
    }
}

fn row(process: &Process) -> adw::ActionRow {
    let icon = if process.is_windows() {
        "applications-games-symbolic"
    } else {
        "system-run-symbolic"
    };

    let row = adw::ActionRow::builder()
        .use_markup(false)
        .subtitle_lines(1)
        .activatable(true)
        .tooltip_text(process.command_line())
        .build();
    row.set_title(&process.name);
    row.set_subtitle(&format!("{} · {}", process.pid, process.command_line()));

    row.add_prefix(&gtk::Image::from_icon_name(icon));
    row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
    row
}
