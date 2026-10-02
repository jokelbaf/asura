use adw::prelude::*;
use session::{Connection, Mark, Record, Stage, State};
use time::OffsetDateTime;

use crate::ui::{certificate, clear, copy_button, format, row};

pub struct Overview {
    pub widget: gtk::ScrolledWindow,
    content: gtk::Box,
}

impl Overview {
    pub fn new() -> Self {
        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(18)
            .margin_top(18)
            .margin_bottom(18)
            .margin_start(12)
            .margin_end(12)
            .build();

        let clamp = adw::Clamp::builder()
            .maximum_size(860)
            .child(&content)
            .build();
        let widget = gtk::ScrolledWindow::builder()
            .child(&clamp)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .build();

        Self { widget, content }
    }

    pub fn show(&self, connection: &Connection, record: &Record) {
        clear(&self.content);

        self.exchange(connection, record);
        self.timing(record);
        self.client(connection);
        self.connection(connection, record);
        self.tls(connection);
        self.certificates(connection);
    }

    fn exchange(&self, connection: &Connection, record: &Record) {
        let request = record.request.as_ref();
        let response = record.response.as_ref();

        let result = match (&record.state, record.paused) {
            (_, Some(Stage::Request)) => {
                String::from("Paused at a breakpoint before the request was sent")
            }
            (_, Some(Stage::Response)) => {
                String::from("Paused at a breakpoint before the response was delivered")
            }
            (State::Open, None) => String::from("In progress"),
            (State::Complete, None) => String::from("Complete"),
            (State::Failed(reason), None) => format!("Failed: {reason}"),
        };

        let tampering: Vec<String> = record
            .marks
            .iter()
            .map(|mark| match mark {
                Mark::Composed => String::from("Sent from Asura"),
                Mark::Mocked(rule) => format!("Answered by the auto-responder rule {rule}"),
                Mark::RequestEdited => String::from("Request edited at a breakpoint"),
                Mark::ResponseEdited => String::from("Response edited at a breakpoint"),
            })
            .collect();

        let fields = [
            (
                "URL",
                request.map(|request| request.url()).unwrap_or_default(),
            ),
            (
                "Destination",
                if request.is_some() {
                    String::new()
                } else {
                    destination(connection)
                },
            ),
            (
                "Method",
                request
                    .map(|request| request.method.clone())
                    .unwrap_or_default(),
            ),
            (
                "Status",
                response
                    .map(|response| format!("{} {}", response.status, response.reason))
                    .unwrap_or_default(),
            ),
            (
                "Version",
                request
                    .map(|request| request.version.as_str().to_owned())
                    .unwrap_or_default(),
            ),
            ("Result", result),
            ("Tampering", tampering.join("; ")),
            ("Started", format::moment(record.timing.started)),
            (
                "Request body",
                request
                    .map(|request| format::size(request.body.len()))
                    .unwrap_or_default(),
            ),
            (
                "Response body",
                response
                    .map(|response| format::size(response.body.len()))
                    .unwrap_or_default(),
            ),
            ("WebSocket messages", count(record.frames.len())),
        ];

        self.group("Exchange", &fields);
    }

    fn timing(&self, record: &Record) {
        let timing = &record.timing;
        let span = |from: Option<OffsetDateTime>, to: Option<OffsetDateTime>| match (from, to) {
            (Some(from), Some(to)) => format::duration(to - from),
            _ => String::new(),
        };

        let finished = timing.response_complete.or(timing.request_complete);

        let fields = [
            (
                "Request sent",
                span(Some(timing.started), timing.request_complete),
            ),
            (
                "Waiting for response",
                span(timing.request_complete, timing.response_started),
            ),
            (
                "Receiving response",
                span(timing.response_started, timing.response_complete),
            ),
            ("Total", span(Some(timing.started), finished)),
        ];

        self.group("Timing", &fields);
    }

    fn client(&self, connection: &Connection) {
        let client = &connection.client;

        let fields = [
            ("Process", client.name.clone()),
            ("Process ID", count(client.process as usize)),
            ("Command line", client.command_line.clone()),
            (
                "Capture scope",
                client
                    .target
                    .map(|target| target.to_string())
                    .unwrap_or_default(),
            ),
        ];

        self.group("Client", &fields);
    }

    fn connection(&self, connection: &Connection, record: &Record) {
        let tunnel = record.tunnel.as_ref();

        let fields = [
            ("Server", connection.server.to_string()),
            ("Connection", format!("#{}", connection.id)),
            ("Opened", format::moment(connection.opened)),
            (
                "Bytes sent",
                tunnel
                    .map(|tunnel| format::size(tunnel.sent_bytes))
                    .unwrap_or_default(),
            ),
            (
                "Bytes received",
                tunnel
                    .map(|tunnel| format::size(tunnel.received_bytes))
                    .unwrap_or_default(),
            ),
        ];

        self.group("Connection", &fields);
    }

    fn tls(&self, connection: &Connection) {
        let Some(tls) = &connection.tls else {
            return;
        };

        let decryption = match &tls.reason {
            None => String::from("Decrypted"),
            Some(reason) => format!("Not decrypted: {reason}"),
        };

        let fields = [
            ("Server name", tls.server_name.clone().unwrap_or_default()),
            ("Offered protocols", tls.offered_protocols.join(", ")),
            (
                "Negotiated protocol",
                tls.protocol.clone().unwrap_or_default(),
            ),
            ("Version", tls.version.clone().unwrap_or_default()),
            ("Cipher suite", tls.cipher.clone().unwrap_or_default()),
            ("Decryption", decryption),
        ];

        self.group("TLS", &fields);
    }

    fn certificates(&self, connection: &Connection) {
        let Some(tls) = connection
            .tls
            .as_ref()
            .filter(|tls| !tls.certificates.is_empty())
        else {
            return;
        };

        let chain: String = tls.certificates.iter().map(|der| trust::pem(der)).collect();

        let group = adw::PreferencesGroup::builder()
            .title("Server Certificates")
            .description("The chain presented by the real server")
            .header_suffix(&copy_button("Copy Chain as PEM", move || chain.clone()))
            .build();

        for (der, details) in tls
            .certificates
            .iter()
            .filter_map(|der| Some((der, certificate::inspect(der)?)))
        {
            let expander = adw::ExpanderRow::builder().use_markup(false).build();
            expander.set_title(&format::printable(&details.name));
            expander.set_subtitle(&format::printable(&format!(
                "Issued by {}",
                details.issuer_name
            )));

            let pem = trust::pem(der);
            expander.add_suffix(&copy_button("Copy as PEM", move || pem.clone()));

            let fields = [
                ("Subject", details.subject),
                ("Issuer", details.issuer),
                ("Valid from", details.valid_from),
                ("Valid until", details.valid_until),
                ("Alternative names", details.names.join(", ")),
                ("Serial number", details.serial),
                ("SHA-1 thumbprint", details.thumbprint),
            ];

            for (title, value) in fields.iter().filter(|(_, value)| !value.is_empty()) {
                expander.add_row(&row(title, value));
            }

            group.add(&expander);
        }

        self.content.append(&group);
    }

    fn group(&self, title: &str, fields: &[(&str, String)]) {
        let present: Vec<&(&str, String)> = fields
            .iter()
            .filter(|(_, value)| !value.is_empty())
            .collect();

        if present.is_empty() {
            return;
        }

        let text: String = present
            .iter()
            .map(|(title, value)| format!("{title}: {}\n", format::printable(value)))
            .collect();

        let group = adw::PreferencesGroup::builder()
            .title(title)
            .header_suffix(&copy_button("Copy All", move || text.clone()))
            .build();

        for (title, value) in present {
            group.add(&row(title, value));
        }

        self.content.append(&group);
    }
}

fn count(value: usize) -> String {
    if value == 0 {
        String::new()
    } else {
        value.to_string()
    }
}

fn destination(connection: &Connection) -> String {
    match connection
        .tls
        .as_ref()
        .and_then(|tls| tls.server_name.as_ref())
    {
        Some(name) => format!("{name} ({})", connection.server),
        None => connection.server.to_string(),
    }
}
