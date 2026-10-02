#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Os(#[from] rustix::io::Errno),

    #[error(transparent)]
    Map(#[from] aya::maps::MapError),

    #[error(transparent)]
    Bus(#[from] zbus::Error),

    #[error("privileged helper failed: {0}")]
    Helper(String),

    #[error("malformed descriptor handoff")]
    Handoff,

    #[error("process {0} has no cgroup v2 membership")]
    Membership(u32),

    #[error("unit {unit} could not be started: {result}")]
    Scope { unit: String, result: String },
}
