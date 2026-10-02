use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::client::{ClientSessionMemoryCache, ClientSessionStore, Resumption};
use rustls::crypto::CryptoProvider;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{CertificateError, ClientConfig, DigitallySignedStruct, SignatureScheme};
use tokio::net::TcpStream;
use tokio::time::timeout;
use tokio_rustls::TlsConnector;
use tokio_rustls::client::TlsStream;

const HANDSHAKE: Duration = Duration::from_secs(15);
const SESSIONS: usize = 1024;

pub struct Upstream {
    provider: Arc<CryptoProvider>,
    verifier: Arc<Verifier>,
    sessions: Arc<dyn ClientSessionStore>,
}

impl Upstream {
    pub fn new(provider: Arc<CryptoProvider>) -> Result<Self, rustls::Error> {
        Ok(Self {
            verifier: Arc::new(Verifier {
                inner: rustls_platform_verifier::Verifier::new(provider.clone())?,
            }),
            sessions: Arc::new(ClientSessionMemoryCache::new(SESSIONS)),
            provider,
        })
    }

    pub async fn connect(
        &self,
        server_name: Option<&str>,
        protocols: Vec<Vec<u8>>,
        destination: SocketAddr,
        stream: TcpStream,
    ) -> io::Result<TlsStream<TcpStream>> {
        let mut config = ClientConfig::builder_with_provider(self.provider.clone())
            .with_safe_default_protocol_versions()
            .map_err(io::Error::other)?
            .dangerous()
            .with_custom_certificate_verifier(self.verifier.clone())
            .with_no_client_auth();

        config.alpn_protocols = protocols;
        config.enable_sni = server_name.is_some();
        config.resumption = Resumption::store(self.sessions.clone());

        let name = match server_name {
            Some(name) => ServerName::try_from(name.to_owned()).map_err(io::Error::other)?,
            None => ServerName::IpAddress(destination.ip().into()),
        };

        timeout(
            HANDSHAKE,
            TlsConnector::from(Arc::new(config)).connect(name, stream),
        )
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "handshake timed out"))?
    }
}

#[derive(Debug)]
struct Verifier {
    inner: rustls_platform_verifier::Verifier,
}

impl ServerCertVerifier for Verifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        ocsp_response: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        let verdict = self.inner.verify_server_cert(
            end_entity,
            intermediates,
            server_name,
            ocsp_response,
            now,
        );

        match (verdict, server_name) {
            (
                Err(rustls::Error::InvalidCertificate(
                    CertificateError::NotValidForName
                    | CertificateError::NotValidForNameContext { .. },
                )),
                ServerName::IpAddress(_),
            ) => Ok(ServerCertVerified::assertion()),
            (verdict, _) => verdict,
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        certificate: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.inner
            .verify_tls12_signature(message, certificate, signature)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        certificate: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.inner
            .verify_tls13_signature(message, certificate, signature)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.inner.supported_verify_schemes()
    }
}
