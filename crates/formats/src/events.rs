#[derive(Clone, Debug, Default)]
pub struct Event {
    pub kind: String,
    pub id: String,
    pub data: String,
    pub retry: String,
    pub comment: bool,
}

impl Event {
    fn is_empty(&self) -> bool {
        self.kind.is_empty() && self.id.is_empty() && self.data.is_empty() && self.retry.is_empty()
    }
}

pub fn events(text: &str) -> Vec<Event> {
    let mut events = Vec::new();
    let mut current = Event::default();

    for line in text
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
    {
        if line.is_empty() {
            dispatch(&mut events, &mut current);
            continue;
        }

        if let Some(comment) = line.strip_prefix(':') {
            events.push(Event {
                data: comment.trim_start().to_owned(),
                comment: true,
                ..Event::default()
            });
            continue;
        }

        let (field, value) = line.split_once(':').unwrap_or((line, ""));
        let value = value.strip_prefix(' ').unwrap_or(value);

        match field {
            "event" => current.kind = value.to_owned(),
            "id" => current.id = value.to_owned(),
            "retry" => current.retry = value.to_owned(),
            "data" => {
                current.data.push_str(value);
                current.data.push('\n');
            }
            _ => {}
        }
    }

    dispatch(&mut events, &mut current);
    events
}

fn dispatch(events: &mut Vec<Event>, current: &mut Event) {
    if current.is_empty() {
        return;
    }

    let mut event = std::mem::take(current);

    if event.data.ends_with('\n') {
        event.data.pop();
    }

    events.push(event);
}
