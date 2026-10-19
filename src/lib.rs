pub mod metadata;
pub mod downloader;
pub mod java;
pub mod launch;
pub mod account;
pub mod instance;

pub const USER_AGENT: &str = concat!("Quarry/", env!("CARGO_PKG_VERSION"));
