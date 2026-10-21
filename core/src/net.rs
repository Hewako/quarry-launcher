//! Общий HTTP-клиент: один пул соединений на всё приложение, User-Agent, таймауты, повторы.

use crate::error::{Error, Result};
use serde::de::DeserializeOwned;
use std::sync::OnceLock;
use std::time::Duration;

/// User-Agent по требованию Modrinth: автор/проект/версия (контакт).
pub const USER_AGENT: &str = concat!(
    "Hewako/quarry-launcher/",
    env!("CARGO_PKG_VERSION"),
    " (github.com/Hewako/quarry-launcher)"
);

/// Сколько раз пробовать запрос при сетевой ошибке, 429 или 5xx.
pub const ATTEMPTS: u32 = 4;

pub fn client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .connect_timeout(Duration::from_secs(15))
            .read_timeout(Duration::from_secs(60))
            .https_only(true)
            .build()
            .expect("HTTP-клиент с настройками по умолчанию собирается всегда")
    })
}

pub fn host_of(url: &str) -> String {
    url.split("://")
        .nth(1)
        .and_then(|rest| rest.split('/').next())
        .unwrap_or(url)
        .to_string()
}

pub fn network_error(url: &str, e: reqwest::Error) -> Error {
    let reason = if e.is_timeout() {
        "истекло время ожидания".to_string()
    } else if e.is_connect() {
        "не удалось подключиться".to_string()
    } else {
        // Текст reqwest содержит URL; токенов в наших URL нет.
        e.to_string()
    };
    Error::Network {
        host: host_of(url),
        reason,
    }
}

/// Проверить статус ответа; тело ошибки (обрезанное) попадает в сообщение.
pub async fn check_status(url: &str, resp: reqwest::Response) -> Result<reqwest::Response> {
    let status = resp.status();
    if status.is_success() {
        return Ok(resp);
    }
    let body = resp.text().await.unwrap_or_default();
    let detail = if body.trim().is_empty() {
        String::new()
    } else {
        format!(": {}", body.chars().take(300).collect::<String>())
    };
    Err(Error::Http {
        host: host_of(url),
        status: status.as_u16(),
        detail,
    })
}

/// Пауза перед повтором номер `attempt` (1, 2, 3…): 0,5 с, 1 с, 2 с…
pub fn backoff(attempt: u32) -> Duration {
    Duration::from_millis(500 * 2u64.pow(attempt.saturating_sub(1).min(5)))
}

/// Выполнить запрос с повторами при временных ошибках.
pub async fn with_retries<T, F, Fut>(mut op: F) -> Result<T>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T>>,
{
    let mut attempt = 0;
    loop {
        attempt += 1;
        match op().await {
            Err(e) if e.is_retryable() && attempt < ATTEMPTS => {
                tokio::time::sleep(backoff(attempt)).await;
            }
            other => return other,
        }
    }
}

pub async fn get_bytes(url: &str) -> Result<Vec<u8>> {
    with_retries(|| async {
        let resp = client()
            .get(url)
            .send()
            .await
            .map_err(|e| network_error(url, e))?;
        let resp = check_status(url, resp).await?;
        let bytes = resp.bytes().await.map_err(|e| network_error(url, e))?;
        Ok(bytes.to_vec())
    })
    .await
}

pub async fn get_text(url: &str) -> Result<String> {
    let bytes = get_bytes(url).await?;
    String::from_utf8(bytes).map_err(|e| Error::parse(url, e))
}

pub async fn get_json<T: DeserializeOwned>(url: &str) -> Result<T> {
    let bytes = get_bytes(url).await?;
    serde_json::from_slice(&bytes).map_err(|e| Error::parse(format!("ответ {}", host_of(url)), e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_is_extracted() {
        assert_eq!(
            host_of("https://piston-meta.mojang.com/mc/game/x.json"),
            "piston-meta.mojang.com"
        );
        assert_eq!(host_of("nonsense"), "nonsense");
    }

    #[test]
    fn backoff_grows() {
        assert_eq!(backoff(1), Duration::from_millis(500));
        assert_eq!(backoff(2), Duration::from_millis(1000));
        assert_eq!(backoff(3), Duration::from_millis(2000));
    }

    #[test]
    fn user_agent_has_contact() {
        assert!(USER_AGENT.starts_with("Hewako/quarry-launcher/"));
        assert!(USER_AGENT.contains("github.com"));
    }

    #[test]
    fn retryable_statuses() {
        let http = |status| Error::Http {
            host: "x".into(),
            status,
            detail: String::new(),
        };
        assert!(http(429).is_retryable());
        assert!(http(503).is_retryable());
        assert!(!http(404).is_retryable());
        assert!(!Error::Cancelled.is_retryable());
    }
}
