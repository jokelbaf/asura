use sha1::{Digest, Sha1};
use x509_parser::extensions::GeneralName;
use x509_parser::prelude::{FromDer, X509Certificate, X509Name};

use crate::ui::format;

pub struct Details {
    pub name: String,
    pub issuer_name: String,
    pub subject: String,
    pub issuer: String,
    pub valid_from: String,
    pub valid_until: String,
    pub names: Vec<String>,
    pub serial: String,
    pub thumbprint: String,
}

pub fn inspect(der: &[u8]) -> Option<Details> {
    let (_, certificate) = X509Certificate::from_der(der).ok()?;
    let validity = certificate.validity();

    let names = certificate
        .subject_alternative_name()
        .ok()
        .flatten()
        .map(|extension| {
            extension
                .value
                .general_names
                .iter()
                .filter_map(|name| match name {
                    GeneralName::DNSName(name) => Some((*name).to_owned()),
                    GeneralName::IPAddress(bytes) => address(bytes),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default();

    Some(Details {
        name: common_name(certificate.subject()),
        issuer_name: common_name(certificate.issuer()),
        subject: certificate.subject().to_string(),
        issuer: certificate.issuer().to_string(),
        valid_from: format::calendar(validity.not_before.to_datetime()),
        valid_until: format::calendar(validity.not_after.to_datetime()),
        names,
        serial: certificate.raw_serial_as_string(),
        thumbprint: hex::encode_upper(Sha1::digest(der)),
    })
}

fn common_name(name: &X509Name<'_>) -> String {
    name.iter_common_name()
        .find_map(|name| name.as_str().ok())
        .map_or_else(|| name.to_string(), str::to_owned)
}

fn address(bytes: &[u8]) -> Option<String> {
    let address = match bytes.len() {
        4 => std::net::IpAddr::from(<[u8; 4]>::try_from(bytes).ok()?),
        16 => std::net::IpAddr::from(<[u8; 16]>::try_from(bytes).ok()?),
        _ => return None,
    };

    Some(address.to_string())
}
