use adw::prelude::*;
use gtk::{gio, pango};

const SESSION: [(&str, &str, &str); 5] = [
    (
        "edit-find-symbolic",
        "win.search",
        "Find in Session (Ctrl+Shift+F)",
    ),
    ("folder-open-symbolic", "win.open", "Open Session (Ctrl+O)"),
    (
        "folder-download-symbolic",
        "win.save",
        "Save Session (Ctrl+S)",
    ),
    ("send-to-symbolic", "win.export", "Export as HAR (Ctrl+E)"),
    (
        "edit-clear-all-symbolic",
        "win.clear",
        "Clear Session (Ctrl+Shift+Delete)",
    ),
];

const SHORTCUTS: [(&str, &[(&str, &str)]); 4] = [
    (
        "Capture",
        &[
            ("Start or stop capturing", "F12"),
            ("Launch program", "<Control>l"),
            ("Attach to process", "<Control><Shift>a"),
        ],
    ),
    (
        "Session",
        &[
            ("Open session", "<Control>o"),
            ("Save session", "<Control>s"),
            ("Export as HAR", "<Control>e"),
            ("Find in session", "<Control><Shift>f"),
            ("Clear session", "<Control><Shift>Delete"),
        ],
    ),
    (
        "Requests",
        &[
            (
                "Filter requests, or find in body when the inspector has focus",
                "<Control>f",
            ),
            ("Remove selected requests", "Delete"),
            ("Copy URL", "<Control>c"),
            ("Copy as cURL", "<Control><Shift>c"),
            ("Replay", "<Control>r"),
            ("Edit and resend", "<Control><Shift>r"),
            ("Compose request", "<Control>n"),
            ("Rules", "<Control>b"),
        ],
    ),
    (
        "General",
        &[
            ("Preferences", "<Control>comma"),
            ("Keyboard shortcuts", "<Control>question"),
            ("Close window", "<Control>w"),
            ("Quit", "<Control>q"),
        ],
    ),
];

pub struct Heading {
    pub widget: gtk::Box,
    dot: gtk::Box,
    state: gtk::Label,
}

impl Heading {
    pub fn new() -> Self {
        let title = gtk::Label::builder()
            .label("Asura")
            .single_line_mode(true)
            .css_classes(["heading"])
            .build();

        let dot = gtk::Box::builder()
            .valign(gtk::Align::Center)
            .css_classes(["capture-dot"])
            .build();

        let state = gtk::Label::builder()
            .single_line_mode(true)
            .ellipsize(pango::EllipsizeMode::End)
            .css_classes(["caption", "dimmed"])
            .build();

        let line = gtk::Box::builder()
            .spacing(6)
            .halign(gtk::Align::Center)
            .build();
        line.append(&dot);
        line.append(&state);

        let widget = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .valign(gtk::Align::Center)
            .build();
        widget.append(&title);
        widget.append(&line);

        Self { widget, dot, state }
    }

    pub fn show(&self, capturing: bool, count: &str) {
        let state = if capturing {
            "Capturing"
        } else {
            "Not capturing"
        };
        self.state.set_text(&format!("{state} · {count}"));

        if self.dot.has_css_class("live") != capturing {
            if capturing {
                self.dot.add_css_class("live");
            } else {
                self.dot.remove_css_class("live");
            }
        }
    }
}

pub struct Toggle {
    pub button: gtk::Button,
    icons: gtk::Stack,
}

impl Toggle {
    pub fn new() -> Self {
        let icons = gtk::Stack::builder()
            .transition_type(gtk::StackTransitionType::Crossfade)
            .transition_duration(150)
            .build();
        icons.add_named(
            &gtk::Image::from_icon_name("media-playback-start-symbolic"),
            Some("start"),
        );
        icons.add_named(
            &gtk::Image::from_icon_name("media-playback-stop-symbolic"),
            Some("stop"),
        );
        icons.add_named(&adw::Spinner::new(), Some("busy"));

        let button = gtk::Button::builder()
            .child(&icons)
            .action_name("win.capture")
            .tooltip_text("Start Capturing (F12)")
            .build();

        Self { button, icons }
    }

    pub fn show(&self, capturing: bool) {
        let (icon, tooltip) = if capturing {
            ("stop", "Stop Capturing (F12)")
        } else {
            ("start", "Start Capturing (F12)")
        };

        self.icons.set_visible_child_name(icon);
        self.button.set_tooltip_text(Some(tooltip));
    }

    pub fn busy(&self) {
        self.icons.set_visible_child_name("busy");
    }
}

pub fn session_tools() -> gtk::Box {
    let tools = gtk::Box::builder().spacing(2).build();

    for (icon, action, tooltip) in SESSION {
        tools.append(
            &gtk::Button::builder()
                .icon_name(icon)
                .action_name(action)
                .tooltip_text(tooltip)
                .css_classes(["flat"])
                .build(),
        );
    }

    tools
}

pub fn main_menu() -> gio::Menu {
    let menu = gio::Menu::new();

    let session = gio::Menu::new();
    session.append(Some("_Open Session…"), Some("win.open"));
    session.append(Some("_Save Session…"), Some("win.save"));
    session.append(Some("_Export as HAR…"), Some("win.export"));
    session.append(Some("_Find in Session…"), Some("win.search"));
    session.append(Some("_Clear Session…"), Some("win.clear"));
    menu.append_section(None, &session);

    let capture = gio::Menu::new();
    capture.append(Some("_Launch Program…"), Some("win.launch"));
    capture.append(Some("_Attach to Process…"), Some("win.attach"));
    menu.append_section(None, &capture);

    let tampering = gio::Menu::new();
    tampering.append(Some("C_ompose Request…"), Some("win.compose"));
    tampering.append(Some("_Rules…"), Some("win.rules"));
    menu.append_section(None, &tampering);

    let application = gio::Menu::new();
    application.append(Some("_Preferences"), Some("app.preferences"));
    application.append(Some("_Keyboard Shortcuts"), Some("win.shortcuts"));
    application.append(Some("_About Asura"), Some("app.about"));
    menu.append_section(None, &application);

    menu
}

pub fn context_menu() -> gio::Menu {
    let menu = gio::Menu::new();

    let resend = gio::Menu::new();
    resend.append(Some("Re_play"), Some("win.replay"));
    resend.append(Some("_Edit and Resend…"), Some("win.resend"));
    menu.append_section(None, &resend);

    let tamper = gio::Menu::new();
    tamper.append(Some("Add to _Auto Responder…"), Some("win.respond-with"));
    tamper.append(Some("_Break on This URL…"), Some("win.break-here"));
    menu.append_section(None, &tamper);

    let copy = gio::Menu::new();
    copy.append(Some("Copy _URL"), Some("win.copy-url"));
    copy.append(Some("Copy as _cURL"), Some("win.copy-curl"));
    menu.append_section(None, &copy);

    let export = gio::Menu::new();
    export.append(
        Some("_Export Selected as HAR…"),
        Some("win.export-selected"),
    );
    export.append(Some("_Do Not Decrypt This Host"), Some("win.skip-host"));
    menu.append_section(None, &export);

    let remove = gio::Menu::new();
    remove.append(Some("_Remove"), Some("win.remove"));
    menu.append_section(None, &remove);

    menu
}

pub fn welcome() -> adw::StatusPage {
    let launch = gtk::Button::builder()
        .label("Launch Program…")
        .action_name("win.launch")
        .css_classes(["pill", "suggested-action"])
        .build();

    let attach = gtk::Button::builder()
        .label("Attach to Process…")
        .action_name("win.attach")
        .css_classes(["pill"])
        .build();

    let buttons = gtk::Box::builder()
        .spacing(12)
        .halign(gtk::Align::Center)
        .build();
    buttons.append(&launch);
    buttons.append(&attach);

    adw::StatusPage::builder()
        .icon_name("network-transmit-receive-symbolic")
        .title("No Traffic Yet")
        .description("Launch a program through Asura, or attach to a running one and start capturing. For Steam games, set the launch options to <tt>asura-run %command%</tt>.")
        .child(&buttons)
        .build()
}

pub fn unmatched() -> adw::StatusPage {
    adw::StatusPage::builder()
        .icon_name("edit-find-symbolic")
        .title("No Matching Requests")
        .description("Change or clear the filter")
        .build()
}

pub fn shortcuts() -> adw::ShortcutsDialog {
    let dialog = adw::ShortcutsDialog::new();

    for (title, items) in SHORTCUTS {
        let section = adw::ShortcutsSection::new(Some(title));

        for (description, accelerator) in items {
            section.add(adw::ShortcutsItem::new(description, accelerator));
        }

        dialog.add(section);
    }

    dialog
}
