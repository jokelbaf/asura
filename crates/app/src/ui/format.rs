use std::borrow::Cow;

use gtk::glib;
use time::{Duration, OffsetDateTime};

const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];

pub fn printable(text: &str) -> Cow<'_, str> {
    if text.contains('\0') {
        Cow::Owned(text.replace('\0', "\u{2400}"))
    } else {
        Cow::Borrowed(text)
    }
}

pub fn size(bytes: u64) -> String {
    let mut value = bytes as f64;
    let mut unit = 0;

    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }

    match unit {
        0 => format!("{bytes} B"),
        _ if value < 10.0 => format!("{value:.1} {}", UNITS[unit]),
        _ => format!("{value:.0} {}", UNITS[unit]),
    }
}

pub fn duration(duration: Duration) -> String {
    let milliseconds = duration.whole_milliseconds();

    match milliseconds {
        ..1 => format!("{} µs", duration.whole_microseconds().max(0)),
        1..1000 => format!("{milliseconds} ms"),
        1000..60_000 => format!("{:.2} s", duration.as_seconds_f64()),
        _ => format!(
            "{} min {} s",
            milliseconds / 60_000,
            milliseconds % 60_000 / 1000
        ),
    }
}

pub fn clock(time: OffsetDateTime) -> String {
    local(time, "%H:%M:%S")
        .map(|clock| format!("{clock}.{:03}", time.millisecond()))
        .unwrap_or_default()
}

pub fn moment(time: OffsetDateTime) -> String {
    local(time, "%Y-%m-%d %H:%M:%S")
        .map(|moment| format!("{moment}.{:03}", time.millisecond()))
        .unwrap_or_default()
}

pub fn calendar(time: OffsetDateTime) -> String {
    local(time, "%Y-%m-%d %H:%M:%S").unwrap_or_default()
}

fn local(time: OffsetDateTime, pattern: &str) -> Option<String> {
    glib::DateTime::from_unix_local(time.unix_timestamp())
        .ok()?
        .format(pattern)
        .ok()
        .map(Into::into)
}
