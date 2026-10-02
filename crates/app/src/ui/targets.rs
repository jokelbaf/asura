use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use adw::prelude::*;
use gtk::glib;

use crate::service::{Origin, Service};
use crate::ui::{notify, row};

const STEAM: &str = "asura-run %command%";

pub struct Targets {
    pub button: gtk::MenuButton,
    content: adw::ButtonContent,
    list: gtk::ListBox,
    empty: gtk::Label,
    everything: adw::SwitchRow,
    service: Arc<Service>,
    syncing: Cell<bool>,
}

impl Targets {
    pub fn new(service: Arc<Service>) -> Rc<Self> {
        let empty = gtk::Label::builder()
            .label("No targets yet. Launch a program or attach to a running one.")
            .wrap(true)
            .max_width_chars(40)
            .xalign(0.0)
            .css_classes(["dimmed"])
            .build();

        let list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .css_classes(["boxed-list"])
            .visible(false)
            .build();

        let launch = action("Launch Program…", "system-run-symbolic", "win.launch");
        let attach = action("Attach to Process…", "find-location-symbolic", "win.attach");

        let actions = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .css_classes(["boxed-list"])
            .build();
        actions.append(&launch);
        actions.append(&attach);

        let everything = adw::SwitchRow::builder()
            .title("Capture All Processes")
            .subtitle("Every process of every user except Asura itself")
            .build();

        let steam = row("Steam launch options", STEAM);
        steam.add_css_class("pinned");

        let options = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .css_classes(["boxed-list"])
            .build();
        options.append(&everything);
        options.append(&steam);

        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(12)
            .margin_top(6)
            .margin_bottom(6)
            .margin_start(6)
            .margin_end(6)
            .width_request(360)
            .build();

        content.append(
            &gtk::Label::builder()
                .label("Capture Targets")
                .css_classes(["heading"])
                .xalign(0.0)
                .build(),
        );
        content.append(&empty);
        content.append(&list);
        content.append(&actions);
        content.append(&options);

        let popover = gtk::Popover::builder().child(&content).build();

        for row in [&launch, &attach] {
            row.connect_activated(glib::clone!(
                #[weak]
                popover,
                move |_| popover.popdown()
            ));
        }

        let label = adw::ButtonContent::builder()
            .icon_name("applications-games-symbolic")
            .label("Targets")
            .build();
        let button = gtk::MenuButton::builder()
            .child(&label)
            .popover(&popover)
            .tooltip_text("Capture Targets")
            .build();

        let targets = Rc::new(Self {
            button,
            content: label,
            list,
            empty,
            everything,
            service,
            syncing: Cell::new(false),
        });

        let weak = Rc::downgrade(&targets);
        targets.everything.connect_active_notify(move |row| {
            if let Some(targets) = weak.upgrade().filter(|targets| !targets.syncing.get()) {
                targets.capture_all(row.is_active());
            }
        });

        targets.update();
        targets
    }

    pub fn update(self: &Rc<Self>) {
        let targets = self.service.targets();
        self.list.remove_all();

        for target in &targets {
            let origin = match target.origin {
                Origin::Launched => "launched",
                Origin::Attached => "attached",
            };

            let row = adw::ActionRow::builder()
                .use_markup(false)
                .tooltip_text(&target.command)
                .build();
            row.set_title(&target.name);
            row.set_subtitle(&format!("PID {} · {origin}", target.process));

            let remove = gtk::Button::builder()
                .icon_name("window-close-symbolic")
                .tooltip_text("Stop Capturing This Target")
                .valign(gtk::Align::Center)
                .css_classes(["flat", "circular"])
                .build();

            let service = self.service.clone();
            let cgroup = target.cgroup.id;
            remove.connect_clicked(move |_| service.release(cgroup));

            row.add_suffix(&remove);
            self.list.append(&row);
        }

        self.list.set_visible(!targets.is_empty());
        self.empty.set_visible(targets.is_empty());

        self.content.set_label(&match targets.len() {
            0 => String::from("Targets"),
            1 => String::from("1 Target"),
            count => format!("{count} Targets"),
        });

        self.syncing.set(true);
        self.everything.set_active(self.service.is_capturing_all());
        self.syncing.set(false);
    }

    fn capture_all(self: &Rc<Self>, enabled: bool) {
        let service = self.service.clone();
        let targets = self.clone();

        glib::spawn_future_local(async move {
            let background = service.clone();
            let result = service
                .background(move || background.capture_all(enabled))
                .await;

            if let Err(error) = result {
                notify(&targets.button, &format!("{error:#}"));
            }

            targets.update();
        });
    }
}

fn action(title: &str, icon: &str, name: &str) -> adw::ActionRow {
    let row = adw::ActionRow::builder()
        .title(title)
        .activatable(true)
        .action_name(name)
        .build();

    row.add_prefix(&gtk::Image::from_icon_name(icon));
    row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
    row
}
