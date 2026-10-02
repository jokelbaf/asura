use std::rc::Rc;
use std::sync::Arc;

use session::{Entry, Headers, Record};

use super::body::BodyView;
use super::content::Part;
use super::fields::Fields;

pub struct MessagePage {
    pub widget: gtk::Paned,
    fields: Fields,
    pub body: Rc<BodyView>,
    part: Part,
}

impl MessagePage {
    pub fn new(part: Part) -> Self {
        let fields = Fields::new();
        let body = BodyView::new("");

        let widget = gtk::Paned::builder()
            .orientation(gtk::Orientation::Vertical)
            .start_child(&fields.widget)
            .end_child(&body.widget)
            .resize_start_child(true)
            .shrink_start_child(false)
            .shrink_end_child(false)
            .position(220)
            .wide_handle(true)
            .build();

        Self {
            widget,
            fields,
            body,
            part,
        }
    }

    pub fn show(&self, entry: &Arc<Entry>) {
        self.body.show(entry, self.part);
    }

    pub fn render(&self, record: &Record) {
        self.fields.clear();

        match self.part {
            Part::Request => {
                let Some(request) = &record.request else {
                    return;
                };

                self.fields.line(&format!(
                    "{} {} {}",
                    request.method,
                    request.target,
                    request.version.as_str()
                ));
                self.headers("Headers", &request.headers);
                self.query(&request.target);
                self.cookies(&request.headers);
                self.headers("Trailers", &request.trailers);
            }
            _ => {
                let Some(response) = &record.response else {
                    return;
                };

                self.fields.line(&format!(
                    "{} {} {}",
                    response.version.as_str(),
                    response.status,
                    response.reason
                ));
                self.headers("Headers", &response.headers);
                self.headers("Trailers", &response.trailers);
            }
        }
    }

    pub fn clear(&self) {
        self.fields.clear();
        self.body.clear();
    }

    fn headers(&self, title: &str, headers: &Headers) {
        let fields: Vec<(String, String)> = headers
            .iter()
            .map(|(name, value)| (name.to_owned(), String::from_utf8_lossy(value).into_owned()))
            .collect();

        self.fields.section(title, &fields, ": ");
    }

    fn query(&self, target: &str) {
        let Some((_, query)) = target.split_once('?') else {
            return;
        };

        let pairs: Vec<(String, String)> =
            url::form_urlencoded::parse(query.split('#').next().unwrap_or_default().as_bytes())
                .map(|(name, value)| (name.into_owned(), value.into_owned()))
                .collect();

        self.fields.section("Query", &pairs, "=");
    }

    fn cookies(&self, headers: &Headers) {
        let cookies: Vec<(String, String)> = headers
            .all("cookie")
            .flat_map(|value| {
                String::from_utf8_lossy(value)
                    .split(';')
                    .filter_map(|pair| {
                        let (name, value) = pair.split_once('=')?;
                        Some((name.trim().to_owned(), value.trim().to_owned()))
                    })
                    .collect::<Vec<_>>()
            })
            .collect();

        self.fields.section("Cookies", &cookies, "=");
    }
}
