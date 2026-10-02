use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error("{0} has not been initialised yet; run the application once")]
    Uninitialised(PathBuf),

    #[error("{0} is in use; close every program running in it first")]
    InUse(PathBuf),

    #[error("{command} failed: {output}")]
    Refresh {
        command: &'static str,
        output: String,
    },
}
