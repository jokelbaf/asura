use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use rcgen::{
    BasicConstraints, CertificateParams, DnType, IsCa, KeyPair, KeyUsagePurpose, PKCS_RSA_SHA256,
    RsaKeySize,
};
use rustls::pki_types::pem::PemObject;
use time::{Duration, OffsetDateTime};

use crate::Error;

const CERTIFICATE: &str = "authority.pem";
const KEY: &str = "authority.key";
const NAME: &str = "Asura Interception Authority";
const LIFETIME: Duration = Duration::days(3650);

pub fn certificate(directory: &Path) -> Result<Vec<u8>, Error> {
    let pem = fs::read_to_string(directory.join(CERTIFICATE))?;
    let der = rustls::pki_types::CertificateDer::from_pem_slice(pem.as_bytes())
        .map_err(|_| rcgen::Error::CouldNotParseCertificate)?;

    Ok(der.to_vec())
}

pub struct Material {
    pub certificate: String,
    pub key: KeyPair,
}

pub fn load_or_create(directory: &Path) -> Result<Material, Error> {
    let certificate = directory.join(CERTIFICATE);
    let key = directory.join(KEY);

    if certificate.exists() && key.exists() {
        return Ok(Material {
            certificate: fs::read_to_string(certificate)?,
            key: KeyPair::from_pem(&fs::read_to_string(key)?)?,
        });
    }

    let material = generate()?;

    fs::create_dir_all(directory)?;
    write_private(&key, &material.key.serialize_pem())?;
    fs::write(certificate, &material.certificate)?;

    Ok(material)
}

fn generate() -> Result<Material, Error> {
    let key = KeyPair::generate_rsa_for(&PKCS_RSA_SHA256, RsaKeySize::_2048)?;
    let now = OffsetDateTime::now_utc();

    let mut params = CertificateParams::default();
    params.distinguished_name.push(DnType::CommonName, NAME);
    params
        .distinguished_name
        .push(DnType::OrganizationName, "Asura");
    params.is_ca = IsCa::Ca(BasicConstraints::Constrained(0));
    params.key_usages = vec![
        KeyUsagePurpose::KeyCertSign,
        KeyUsagePurpose::CrlSign,
        KeyUsagePurpose::DigitalSignature,
    ];
    params.not_before = now - Duration::days(1);
    params.not_after = now + LIFETIME;

    Ok(Material {
        certificate: params.self_signed(&key)?.pem(),
        key,
    })
}

fn write_private(path: &Path, contents: &str) -> Result<(), Error> {
    OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?
        .write_all(contents.as_bytes())?;

    Ok(())
}
