#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Certificate(#[from] rcgen::Error),

    #[error(transparent)]
    Tls(#[from] rustls::Error),
}
