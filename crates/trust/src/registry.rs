use std::fs;
use std::io;
use std::path::Path;

use time::OffsetDateTime;

use crate::certificate::Certificate;

const LINE_WIDTH: usize = 76;
const FILETIME_EPOCH: i128 = 11_644_473_600;

pub fn contains(contents: &str, certificate: &Certificate<'_>) -> bool {
    find(contents, &certificate.key()).is_some()
}

pub fn insert(contents: &str, certificate: &Certificate<'_>) -> String {
    let mut updated = contents.trim_end_matches('\n').to_owned();
    updated.push_str("\n\n");
    updated.push_str(&block(certificate));
    updated
}

pub fn remove(contents: &str, certificate: &Certificate<'_>) -> String {
    let Some(start) = find(contents, &certificate.key()) else {
        return contents.to_owned();
    };

    let end = contents[start..]
        .find("\n[")
        .map_or(contents.len(), |offset| start + offset + 1);

    let mut updated = contents[..start].trim_end_matches('\n').to_owned();
    updated.push_str("\n\n");
    updated.push_str(&contents[end..]);
    updated
}

pub fn write(path: &Path, contents: &str) -> io::Result<()> {
    let staging = path.with_extension("reg.asura");
    fs::write(&staging, contents)?;
    fs::set_permissions(&staging, fs::metadata(path)?.permissions())?;
    fs::rename(staging, path)
}

fn find(contents: &str, key: &str) -> Option<usize> {
    let header = format!("[{key}]");

    contents
        .match_indices(&header)
        .map(|(index, _)| index)
        .find(|&index| index == 0 || contents.as_bytes()[index - 1] == b'\n')
}

fn block(certificate: &Certificate<'_>) -> String {
    let now = OffsetDateTime::now_utc();
    let filetime = (now.unix_timestamp_nanos() / 100 + FILETIME_EPOCH * 10_000_000) as u64;

    format!(
        "[{}] {}\n#time={filetime:x}\n{}\n",
        certificate.key(),
        now.unix_timestamp(),
        binary("Blob", &certificate.blob())
    )
}

fn binary(name: &str, data: &[u8]) -> String {
    let mut output = format!("\"{name}\"=hex:");
    let mut column = output.len();

    for (index, byte) in data.iter().enumerate() {
        output.push_str(&format!("{byte:02x}"));
        column += 2;

        if index + 1 < data.len() {
            output.push(',');
            column += 1;

            if column > LINE_WIDTH {
                output.push_str("\\\n  ");
                column = 2;
            }
        }
    }

    output
}
