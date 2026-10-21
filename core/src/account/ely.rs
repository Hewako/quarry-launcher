use anyhow::{Context, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use rand::distributions::Alphanumeric;
use rand::Rng;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::sync::{Arc, Mutex};
use crate::account::{save_active_account, save_token_to_keyring, AccountKind, AccountRef};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::oneshot;

const ELY_CLIENT_ID: &str = "quarry1";
const ELY_AUTH_URL: &str = "https://account.ely.by/oauth2/v1";
const ELY_TOKEN_URL: &str = "https://account.ely.by/api/oauth2/v1/token";

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
}

#[derive(Debug, Deserialize)]
struct AccountInfo {
    uuid: String,
    username: String,
}

fn random_string(len: usize) -> String {
    rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(len)
        .map(char::from)
        .collect()
}

pub async fn login() -> Result<AccountRef> {
    let state = random_string(32);
    let code_verifier = random_string(64);

    let mut hasher = Sha256::new();
    hasher.update(code_verifier.as_bytes());
    let code_challenge = URL_SAFE_NO_PAD.encode(hasher.finalize());

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .context("Не удалось открыть локальный порт для OAuth")?;
    let port = listener.local_addr()?.port();
    let redirect_uri = format!("http://127.0.0.1:{}/callback", port);

    let auth_url = format!(
        "{}?client_id={}&redirect_uri={}&response_type=code&scope=minecraft_server_session%20account_info%20offline_access&state={}&code_challenge={}&code_challenge_method=S256",
        ELY_AUTH_URL,
        ELY_CLIENT_ID,
        urlencoding::encode(&redirect_uri),
        state,
        code_challenge
    );

    println!("Открываем браузер для входа Ely.by...");
    open::that(&auth_url).context("Не удалось открыть браузер")?;

    let (tx, rx) = oneshot::channel();
    let tx = Arc::new(Mutex::new(Some(tx)));
    let state_clone = state.clone();

    tokio::spawn(async move {
        if let Ok((mut socket, _)) = listener.accept().await {
            let mut buf = [0; 4096];
            if let Ok(n) = socket.read(&mut buf).await {
                let req = String::from_utf8_lossy(&buf[..n]);
                let path = req.lines().next().unwrap_or("").split_whitespace().nth(1).unwrap_or("");
                let mut final_code = None;
                let mut final_state = None;

                if path.starts_with("/callback") {
                    for pair in path.split('?').nth(1).unwrap_or("").split('&') {
                        let mut it = pair.splitn(2, '=');
                        if let (Some(k), Some(v)) = (it.next(), it.next()) {
                            if k == "code" {
                                final_code = urlencoding::decode(v).ok().map(|c| c.into_owned());
                            }
                            if k == "state" {
                                final_state = Some(v.to_string());
                            }
                        }
                    }
                }

                let html = "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\n\r\n<h1>Готово</h1><p>Можете закрыть вкладку.</p>";
                let _ = socket.write_all(html.as_bytes()).await;

                if let Some(code) = final_code {
                    if final_state.as_deref() == Some(&state_clone) {
                        if let Some(sender) = tx.lock().unwrap().take() {
                            let _ = sender.send(code);
                        }
                    }
                }
            }
        }
    });

    let code = tokio::time::timeout(std::time::Duration::from_secs(120), rx)
        .await
        .context("Время ожидания авторизации истекло")?
        .context("Ошибка получения кода")?;

    let client = reqwest::Client::new();

    let body = format!(
        "grant_type=authorization_code&code={}&redirect_uri={}&client_id={}&code_verifier={}",
        urlencoding::encode(&code),
        urlencoding::encode(&redirect_uri),
        ELY_CLIENT_ID,
        urlencoding::encode(&code_verifier),
    );

    let token_response = client
        .post(ELY_TOKEN_URL)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(body)
        .send()
        .await
        .context("Ошибка запроса к Ely.by token")?;

    let token_text = token_response.text().await?;
    let token_resp: TokenResponse = serde_json::from_str(&token_text)
        .with_context(|| format!("Не удалось распарсить ответ токена: {}", token_text))?;

    let info_response = client
        .get("https://account.ely.by/api/account/v1/info")
        .bearer_auth(&token_resp.access_token)
        .send()
        .await
        .context("Ошибка запроса info")?;

    let info_text = info_response.text().await?;
    let info: AccountInfo = serde_json::from_str(&info_text)
        .with_context(|| format!("Не удалось распарсить ответ info: {}", info_text))?;

    let clean_uuid = info.uuid.replace('-', "");
    save_token_to_keyring(&format!("ely_{}", clean_uuid), &token_resp.access_token)?;

    let acc = AccountRef {
        kind: AccountKind::ElyBy,
        uuid: clean_uuid,
        username: info.username,
    };
    save_active_account(&acc)?;
    Ok(acc)
}
