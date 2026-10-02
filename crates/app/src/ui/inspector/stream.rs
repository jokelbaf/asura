use std::rc::Rc;
use std::sync::Arc;

use session::Entry;

use super::body::BodyView;
use super::content::Part;

pub struct StreamPage {
    pub widget: gtk::Paned,
    pub sent: Rc<BodyView>,
    pub received: Rc<BodyView>,
}

impl StreamPage {
    pub fn new() -> Self {
        let sent = BodyView::new("Sent");
        let received = BodyView::new("Received");

        let widget = gtk::Paned::builder()
            .orientation(gtk::Orientation::Vertical)
            .start_child(&sent.widget)
            .end_child(&received.widget)
            .shrink_start_child(false)
            .shrink_end_child(false)
            .wide_handle(true)
            .position(260)
            .build();

        Self {
            widget,
            sent,
            received,
        }
    }

    pub fn show(&self, entry: &Arc<Entry>) {
        self.sent.show(entry, Part::Sent);
        self.received.show(entry, Part::Received);
    }

    pub fn clear(&self) {
        self.sent.clear();
        self.received.clear();
    }
}
