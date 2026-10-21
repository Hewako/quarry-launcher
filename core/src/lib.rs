//! Ядро лаунчера Quarry: библиотека без зависимости от интерфейса.
//! Интерфейс (Tauri) и CLI — тонкие обёртки над этими модулями.

#[cfg(all(feature = "dev-launch", not(debug_assertions)))]
compile_error!(
    "dev-launch (запуск без входа) разрешён только в отладочной сборке и не должен попадать в релиз"
);

pub mod account;
pub mod archive;
pub mod download;
pub mod error;
pub mod install;
pub mod instance;
pub mod java;
pub mod launch;
pub mod loader;
pub mod logmask;
pub mod metadata;
pub mod net;
pub mod paths;
pub mod settings;
pub mod skin;

pub use error::{Error, Result};
pub use net::USER_AGENT;
