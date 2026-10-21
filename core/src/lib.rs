pub mod metadata;
pub mod downloader;
pub mod java;
pub mod launch;
pub mod account;
pub mod skin;
pub mod instance;
pub mod profile;
pub mod loader;

pub const USER_AGENT: &str = concat!("Quarry/", env!("CARGO_PKG_VERSION"));
