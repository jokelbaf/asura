use std::num::NonZeroUsize;
use std::path::Path;
use std::sync::Arc;

use lru::LruCache;
use parking_lot::Mutex;
use rcgen::{
    CertificateParams, DnType, ExtendedKeyUsagePurpose, Issuer, KeyPair, KeyUsagePurpose,
    PKCS_RSA_SHA256, RsaKeySize, SanType, SerialNumber,
};
use rustls::crypto::{CryptoProvider, aws_lc_rs};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::sign::CertifiedKey;
use time::{Duration, OffsetDateTime};

use crate::identity::{Identity, Name};
use crate::{Error, storage};

const CACHE: NonZeroUsize = NonZeroUsize::new(4096).unwrap();
const LEAF_LIFETIME: Duration = Duration::days(365);

pub struct Authority {
    issuer: Issuer<'static, KeyPair>,
    certificate: CertificateDer<'static>,
    pem: String,
    leaf: KeyPair,
    provider: Arc<CryptoProvider>,
    cache: Mutex<LruCache<Identity, Arc<CertifiedKey>>>,
}

impl Authority {
    pub fn open(directory: &Path) -> Result<Self, Error> {
        let material = storage::load_or_create(directory)?;
        let issuer = Issuer::from_ca_cert_pem(&material.certificate, material.key)?;
        let certificate = CertificateDer::from_pem_slice(material.certificate.as_bytes())
            .map_err(|_| rcgen::Error::CouldNotParseCertificate)?;

        Ok(Self {
            issuer,
            certificate,
            pem: material.certificate,
            leaf: KeyPair::generate_rsa_for(&PKCS_RSA_SHA256, RsaKeySize::_2048)?,
            provider: Arc::new(aws_lc_rs::default_provider()),
            cache: Mutex::new(LruCache::new(CACHE)),
        })
    }

    pub fn certificate(&self) -> &CertificateDer<'static> {
        &self.certificate
    }

    pub fn pem(&self) -> &str {
        &self.pem
    }

    pub fn provider(&self) -> &Arc<CryptoProvider> {
        &self.provider
    }

    pub fn issue(&self, identity: &Identity) -> Result<Arc<CertifiedKey>, Error> {
        if let Some(key) = self.cache.lock().get(identity) {
            return Ok(key.clone());
        }

        let key = Arc::new(self.sign(identity)?);
        self.cache.lock().put(identity.clone(), key.clone());
        Ok(key)
    }

    fn sign(&self, identity: &Identity) -> Result<CertifiedKey, Error> {
        let now = OffsetDateTime::now_utc();

        let mut params = CertificateParams::default();
        params
            .distinguished_name
            .push(DnType::CommonName, identity.common_name());
        params.subject_alt_names = identity.names().iter().filter_map(san).collect();
        params.serial_number = Some(self.serial()?);
        params.key_usages = vec![
            KeyUsagePurpose::DigitalSignature,
            KeyUsagePurpose::KeyEncipherment,
        ];
        params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        params.use_authority_key_identifier_extension = true;
        params.not_before = now - Duration::days(2);
        params.not_after = now + LEAF_LIFETIME;

        let leaf = params.signed_by(&self.leaf, &self.issuer)?;
        let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(self.leaf.serialize_der()));
        let signer = self.provider.key_provider.load_private_key(key)?;

        Ok(CertifiedKey::new(
            vec![leaf.der().clone(), self.certificate.clone()],
            signer,
        ))
    }

    fn serial(&self) -> Result<SerialNumber, Error> {
        let mut bytes = [0u8; 16];
        self.provider
            .secure_random
            .fill(&mut bytes)
            .map_err(|_| rustls::Error::FailedToGetRandomBytes)?;

        bytes[0] &= 0x7f;
        Ok(SerialNumber::from_slice(&bytes))
    }
}

fn san(name: &Name) -> Option<SanType> {
    match name {
        Name::Dns(name) => Some(SanType::DnsName(name.clone().try_into().ok()?)),
        Name::Ip(address) => Some(SanType::IpAddress(*address)),
    }
}
