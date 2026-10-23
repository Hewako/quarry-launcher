//! Локальный сервер для OAuth-redirect в системном браузере (`http://127.0.0.1:<порт>/callback`).
//! Принимает несколько соединений (браузеры делают предзапросы и просят favicon), проверяет
//! `state`, обрабатывает `error=`, завершается по таймауту или отмене.

use crate::download::Cancel;
use crate::error::{Error, Result};
use std::collections::HashMap;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

pub struct Loopback {
    listener: TcpListener,
    pub redirect_uri: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Callback {
    Code(String),
    Denied { error: String, message: String },
}

impl Loopback {
    pub async fn bind() -> Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").await.map_err(|e| {
            Error::Auth(format!("Не удалось открыть локальный порт для входа: {e}"))
        })?;
        let port = listener
            .local_addr()
            .map_err(|e| Error::Auth(format!("Локальный порт недоступен: {e}")))?
            .port();
        Ok(Self {
            listener,
            redirect_uri: format!("http://127.0.0.1:{port}/callback"),
        })
    }

    /// Ждать ответа провайдера с нужным `state`.
    pub async fn wait(self, state: &str, timeout: Duration, cancel: &Cancel) -> Result<Callback> {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            cancel.check()?;
            let accept =
                tokio::time::timeout(Duration::from_millis(500), self.listener.accept()).await;
            if tokio::time::Instant::now() >= deadline {
                return Err(Error::Auth(
                    "Время ожидания входа истекло. Нажмите «Войти» ещё раз.".into(),
                ));
            }
            let Ok(Ok((mut socket, _))) = accept else {
                continue;
            };
            let mut buf = vec![0u8; 8192];
            let n = match tokio::time::timeout(Duration::from_secs(5), socket.read(&mut buf)).await
            {
                Ok(Ok(n)) if n > 0 => n,
                _ => continue, // пустое или зависшее соединение — ждём следующего
            };
            let request = String::from_utf8_lossy(&buf[..n]);
            let target = request
                .lines()
                .next()
                .and_then(|l| l.split_whitespace().nth(1))
                .unwrap_or("");
            let Some(query) = target.strip_prefix("/callback") else {
                let _ = respond(&mut socket, 404, "Не найдено").await;
                continue;
            };
            let params = parse_query(query.trim_start_matches('?'));
            if params.get("state").map(String::as_str) != Some(state) {
                let _ = respond(&mut socket, 400, page("Ошибка входа", "Ответ не относится к этому входу (state не совпал). Вернитесь в лаунчер и попробуйте ещё раз.")).await;
                continue;
            }
            if let Some(error) = params.get("error") {
                let message = params
                    .get("error_message")
                    .or_else(|| params.get("error_description"))
                    .cloned()
                    .unwrap_or_default();
                let _ = respond(
                    &mut socket,
                    200,
                    page(
                        "Вход отменён",
                        "Можно закрыть вкладку и вернуться в лаунчер.",
                    ),
                )
                .await;
                return Ok(Callback::Denied {
                    error: error.clone(),
                    message,
                });
            }
            if let Some(code) = params.get("code").filter(|c| !c.is_empty()) {
                let _ = respond(
                    &mut socket,
                    200,
                    page(
                        "Готово",
                        "Вход выполнен. Можно закрыть вкладку и вернуться в Lime.",
                    ),
                )
                .await;
                return Ok(Callback::Code(code.clone()));
            }
            let _ = respond(
                &mut socket,
                400,
                page("Ошибка входа", "В ответе нет кода авторизации."),
            )
            .await;
        }
    }
}

fn page(title: &str, text: &str) -> String {
    format!(
        "<!doctype html><html lang=\"ru\"><meta charset=\"utf-8\"><title>{title}</title>\
         <body style=\"font-family:system-ui;background:#0f1216;color:#e6ebf0;display:grid;place-items:center;height:100vh;margin:0\">\
         <div style=\"max-width:420px;text-align:center\"><h1 style=\"font-size:22px\">{title}</h1><p style=\"color:#9aa5b1\">{text}</p></div></body></html>"
    )
}

async fn respond(
    socket: &mut tokio::net::TcpStream,
    status: u16,
    body: impl AsRef<str>,
) -> std::io::Result<()> {
    let body = body.as_ref();
    let reason = match status {
        200 => "OK",
        404 => "Not Found",
        _ => "Bad Request",
    };
    let head = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    );
    socket.write_all(head.as_bytes()).await?;
    socket.write_all(body.as_bytes()).await?;
    socket.shutdown().await
}

pub fn parse_query(query: &str) -> HashMap<String, String> {
    query
        .split('&')
        .filter(|p| !p.is_empty())
        .filter_map(|pair| {
            let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
            let decode = |s: &str| {
                urlencoding::decode(&s.replace('+', " "))
                    .ok()
                    .map(|c| c.into_owned())
            };
            Some((decode(k)?, decode(v)?))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_parsing_decodes() {
        let q = parse_query("code=a%2Fb&state=xyz&error_message=Access+denied");
        assert_eq!(q["code"], "a/b");
        assert_eq!(q["state"], "xyz");
        assert_eq!(q["error_message"], "Access denied");
    }

    async fn send(uri: &str, path: &str) -> String {
        let addr = uri
            .trim_start_matches("http://")
            .trim_end_matches("/callback")
            .to_string();
        let mut s = tokio::net::TcpStream::connect(addr).await.unwrap();
        s.write_all(format!("GET {path} HTTP/1.1\r\nHost: x\r\n\r\n").as_bytes())
            .await
            .unwrap();
        let mut out = String::new();
        let _ = s.read_to_string(&mut out).await;
        out
    }

    #[tokio::test]
    async fn ignores_noise_and_wrong_state_then_accepts_code() {
        let lb = Loopback::bind().await.unwrap();
        let uri = lb.redirect_uri.clone();
        let cancel = Cancel::new();
        let waiter =
            tokio::spawn(async move { lb.wait("s1", Duration::from_secs(10), &cancel).await });
        // пустое соединение (предзапрос браузера)
        let addr = uri
            .trim_start_matches("http://")
            .trim_end_matches("/callback")
            .to_string();
        drop(tokio::net::TcpStream::connect(&addr).await.unwrap());
        assert!(send(&uri, "/favicon.ico").await.contains("404"));
        assert!(
            send(&uri, "/callback?code=c&state=other")
                .await
                .contains("400")
        );
        assert!(
            send(&uri, "/callback?code=good&state=s1")
                .await
                .contains("200")
        );
        assert_eq!(
            waiter.await.unwrap().unwrap(),
            Callback::Code("good".into())
        );
    }

    #[tokio::test]
    async fn provider_error_is_reported() {
        let lb = Loopback::bind().await.unwrap();
        let uri = lb.redirect_uri.clone();
        let cancel = Cancel::new();
        let waiter =
            tokio::spawn(async move { lb.wait("s", Duration::from_secs(10), &cancel).await });
        send(
            &uri,
            "/callback?state=s&error=access_denied&error_message=User+cancelled",
        )
        .await;
        assert_eq!(
            waiter.await.unwrap().unwrap(),
            Callback::Denied {
                error: "access_denied".into(),
                message: "User cancelled".into()
            }
        );
    }

    #[tokio::test]
    async fn cancel_stops_waiting() {
        let lb = Loopback::bind().await.unwrap();
        let cancel = Cancel::new();
        cancel.cancel();
        assert!(matches!(
            lb.wait("s", Duration::from_secs(10), &cancel).await,
            Err(Error::Cancelled)
        ));
    }
}
