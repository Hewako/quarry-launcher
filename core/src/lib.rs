//! Ядро лаунчера Quarry: библиотека без зависимости от интерфейса.
//! Интерфейс (Tauri) и CLI — тонкие обёртки над этими модулями.

#[cfg(all(feature = "dev-launch", not(debug_assertions)))]
compile_error!(
    "dev-launch (запуск без входа) разрешён только в отладочной сборке и не должен попадать в релиз"
);

pub mod account;
pub mod archive;
pub mod catalog;
pub mod download;
pub mod error;
pub mod import;
pub mod install;
pub mod instance;
pub mod java;
pub mod launch;
pub mod loader;
pub mod logmask;
pub mod metadata;
pub mod mods;
pub mod nbt;
pub mod net;
pub mod paths;
pub mod servers;
pub mod settings;
pub mod skin;
pub mod snapshot;
pub mod worlds;

pub use error::{Error, Result};
pub use net::USER_AGENT;
