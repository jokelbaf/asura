#![no_std]

mod endpoint;
mod origin;
mod settings;

pub use endpoint::Endpoint;
pub use origin::Origin;
pub use settings::Settings;

#[cfg(feature = "user")]
mod pod {
    use super::{Endpoint, Origin, Settings};

    unsafe impl aya::Pod for Endpoint {}
    unsafe impl aya::Pod for Origin {}
    unsafe impl aya::Pod for Settings {}
}
