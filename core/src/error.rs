//! Единая модель ошибок ядра. Сообщения — для пользователя: что случилось и что делать.

use std::path::{Path, PathBuf};

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Нет соединения с {host}: {reason}. Проверьте интернет и повторите.")]
    Network { host: String, reason: String },

    #[error("Сервер {host} ответил HTTP {status}{detail}")]
    Http {
        host: String,
        status: u16,
        detail: String,
    },

    #[error(
        "Файл {path} повреждён: хэш {actual} не совпал с ожидаемым {expected}. Запустите «Проверить файлы» — он будет скачан заново."
    )]
    HashMismatch {
        path: PathBuf,
        expected: String,
        actual: String,
    },

    #[error("Ошибка диска ({path}): {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("Не удалось разобрать {what}: {reason}")]
    Parse { what: String, reason: String },

    #[error("{0}")]
    NotFound(String),

    #[error("{0}")]
    Invalid(String),

    #[error("{0}")]
    Java(String),

    #[error("{0}")]
    Auth(String),

    #[error("Операция отменена")]
    Cancelled,

    #[error("Профиль «{0}» уже запускается или запущен")]
    AlreadyRunning(String),
}

impl Error {
    pub fn io(path: impl AsRef<Path>, source: std::io::Error) -> Self {
        Error::Io {
            path: path.as_ref().to_path_buf(),
            source,
        }
    }

    pub fn parse(what: impl Into<String>, reason: impl ToString) -> Self {
        Error::Parse {
            what: what.into(),
            reason: reason.to_string(),
        }
    }

    pub fn invalid(msg: impl Into<String>) -> Self {
        Error::Invalid(msg.into())
    }

    /// Стоит ли повторять сетевой запрос, который закончился этой ошибкой.
    pub fn is_retryable(&self) -> bool {
        match self {
            Error::Network { .. } => true,
            Error::Http { status, .. } => *status == 429 || *status >= 500,
            _ => false,
        }
    }
}

/// Расширение для `std::io::Result`: добавить путь к ошибке.
pub trait IoContext<T> {
    fn at(self, path: impl AsRef<Path>) -> Result<T>;
}

impl<T> IoContext<T> for std::io::Result<T> {
    fn at(self, path: impl AsRef<Path>) -> Result<T> {
        self.map_err(|e| Error::io(path, e))
    }
}
