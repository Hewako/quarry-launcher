//! Ely.by: OAuth 2.0 в системном браузере и authlib-injector.
//!
//! Ограничение (docs.ely.by/en/oauth.html, сверено 21.10.2026): документация описывает только
//! приложения типа Website и передачу `client_secret` при обмене кода; PKCE и клиенты без секрета
//! не упоминаются. Секрет в публичную программу не вшивается, поэтому обмен идёт с PKCE без
//! секрета. Работает ли это у Ely.by — **не подтверждено первоисточником**; проверить 21.10.2026
//! не удалось: сервисы Ely.by были отключены (HTTP 503). Если сервер ответит `invalid_client`,
//! пользователь увидит объяснение, а не сырой ответ.
//!
//! Основной способ входа — логин и пароль через Yggdrasil-совместимый `authserver.ely.by`
//! (docs.ely.by/en/minecraft-auth.html — описан для лаунчеров, секрет приложения не нужен).
//! Пароль отправляется только на authserver.ely.by и нигде не сохраняется; в хранилище ОС
//! попадают accessToken и clientToken. OAuth оставлен как второй способ.
//!
//! Политика: вход доступен только при Microsoft-аккаунте с подтверждённым владением
//! (`AccountStore::ensure_ely_allowed`). Ely.by-сессия не выдаётся за лицензию Microsoft.

use super::loopback::{Callback, Loopback};
use super::{AccountRef, AccountStore, Provider, Tokens};
use crate::download::{self, Cancel, Expected, Task, Verify};
use crate::error::{Error, Result};
use crate::instance::now;
use crate::paths::DataDir;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::Rng;
use rand::distributions::Alphanumeric;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::time::Duration;

/// Client ID приложения Ely.by владельца проекта. Не секрет. Можно заменить при сборке:
/// `QUARRY_ELY_CLIENT_ID=… cargo build`.
pub const CLIENT_ID: &str = match option_env!("QUARRY_ELY_CLIENT_ID") {
    Some(id) => id,
    None => "axol",
};
const AUTH_URL: &str = "https://account.ely.by/oauth2/v1";
const TOKEN_URL: &str = "https://account.ely.by/api/oauth2/v1/token";
const INFO_URL: &str = "https://account.ely.by/api/account/v1/info";
const SCOPES: &str = "account_info offline_access minecraft_server_session";
const AUTHSERVER: &str = "https://authserver.ely.by";

/// API для authlib-injector (docs.ely.by/en/authlib-injector.html).
pub const AUTHLIB_API: &str = "ely.by";

/// Закреплённая версия authlib-injector: сборка 56 (1.2.8), хэш из официального API
/// authlib-injector.yushi.moe/artifact/latest.json (проверено 21.10.2026).
pub const AUTHLIB_VERSION: &str = "1.2.8";
const AUTHLIB_URL: &str =
    "https://authlib-injector.yushi.moe/artifact/56/authlib-injector-1.2.8.jar";
const AUTHLIB_SHA256: &str = "9c7f4343e6c82034958ffb48c14a2cb0c85928be7283103ce17da00c6d5a7b10";

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<u64>,
}

#[derive(Deserialize)]
struct OAuthError {
    error: String,
    #[serde(default)]
    error_description: Option<String>,
}

#[derive(Deserialize)]
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

pub fn pkce_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

fn explain_token_error(status: u16, body: &str) -> Error {
    let parsed: Option<OAuthError> = serde_json::from_str(body).ok();
    match parsed {
        Some(e) if e.error == "invalid_client" => Error::Auth(
            "Ely.by не принял вход без секрета приложения. Безопасного способа входа для программ на компьютере Ely.by пока не подтвердил; вход через Ely.by временно недоступен."
                .into(),
        ),
        Some(e) if e.error == "invalid_grant" || e.error == "invalid_request" => Error::Auth(format!(
            "Ely.by отклонил код входа ({}). Попробуйте войти ещё раз.",
            e.error_description.unwrap_or(e.error)
        )),
        Some(e) => Error::Auth(format!("Ely.by: {}", e.error_description.unwrap_or(e.error))),
        None => Error::Auth(format!("Ely.by ответил HTTP {status}. Сервис может быть недоступен — попробуйте позже.")),
    }
}

async fn post_token(form: &[(&str, &str)]) -> Result<TokenResponse> {
    let resp = crate::net::client()
        .post(TOKEN_URL)
        .form(form)
        .send()
        .await
        .map_err(|e| crate::net::network_error(TOKEN_URL, e))?;
    let status = resp.status().as_u16();
    let body = resp.text().await.unwrap_or_default();
    if !(200..300).contains(&status) {
        return Err(explain_token_error(status, &body));
    }
    serde_json::from_str(&body)
        .map_err(|_| Error::Auth("Ely.by вернул непонятный ответ на запрос токена".into()))
}

/// Войти через Ely.by в системном браузере. `open_url` открывает ссылку (в CLI — `open`,
/// в интерфейсе — плагин opener).
pub async fn login(
    store: &AccountStore<'_>,
    open_url: impl Fn(&str) -> Result<()>,
    cancel: &Cancel,
) -> Result<AccountRef> {
    store.ensure_ely_allowed()?;
    let state = random_string(32);
    let verifier = random_string(64);
    let lb = Loopback::bind().await?;
    let redirect = lb.redirect_uri.clone();
    let url = format!(
        "{AUTH_URL}?client_id={}&redirect_uri={}&response_type=code&scope={}&state={state}&code_challenge={}&code_challenge_method=S256&prompt=select_account",
        urlencoding::encode(CLIENT_ID),
        urlencoding::encode(&redirect),
        urlencoding::encode(SCOPES),
        pkce_challenge(&verifier),
    );
    open_url(&url)?;
    let code = match lb.wait(&state, Duration::from_secs(300), cancel).await? {
        Callback::Code(c) => c,
        Callback::Denied { error, message } => {
            return Err(Error::Auth(if error == "access_denied" {
                "Вход в Ely.by отменён".into()
            } else {
                format!("Ely.by: {error} {message}")
            }));
        }
    };
    let token = post_token(&[
        ("grant_type", "authorization_code"),
        ("client_id", CLIENT_ID),
        ("code", &code),
        ("redirect_uri", &redirect),
        ("code_verifier", &verifier),
    ])
    .await?;

    let resp = crate::net::client()
        .get(INFO_URL)
        .bearer_auth(&token.access_token)
        .send()
        .await
        .map_err(|e| crate::net::network_error(INFO_URL, e))?;
    let resp = crate::net::check_status(INFO_URL, resp).await?;
    let info: AccountInfo = resp
        .json()
        .await
        .map_err(|_| Error::Auth("Ely.by вернул непонятные данные аккаунта".into()))?;

    let account = AccountRef {
        provider: Provider::ElyBy,
        uuid: info.uuid.replace('-', ""),
        name: info.username.clone(),
        owns_java_edition: false,
        checked_at: now(),
        skin_url: Some(crate::skin::ely_skin_url(&info.username)),
        skin_variant: None,
    };
    let tokens = Tokens {
        access_token: token.access_token,
        expires_at: now() + token.expires_in.unwrap_or(86_400),
        refresh_token: token.refresh_token,
        xuid: None,
        method: Some("oauth".into()),
    };
    store.upsert(account.clone(), &tokens)?;
    Ok(account)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct YggAuth {
    access_token: String,
    client_token: String,
    #[serde(default)]
    selected_profile: Option<YggProfile>,
}

#[derive(Deserialize)]
struct YggProfile {
    id: String,
    name: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct YggError {
    #[serde(default)]
    error_message: String,
}

/// Итог входа по паролю.
#[derive(Debug)]
pub enum PasswordLogin {
    Done(AccountRef),
    /// На аккаунте включена двухфакторная защита — нужен код из приложения.
    NeedTotp,
}

async fn ygg_post(
    path: &str,
    body: serde_json::Value,
) -> Result<std::result::Result<String, (u16, String)>> {
    let url = format!("{AUTHSERVER}{path}");
    let resp = crate::net::client()
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| crate::net::network_error(&url, e))?;
    let status = resp.status().as_u16();
    let text = resp.text().await.unwrap_or_default();
    Ok(if (200..300).contains(&status) {
        Ok(text)
    } else {
        Err((status, text))
    })
}

fn ygg_message(body: &str) -> String {
    serde_json::from_str::<YggError>(body)
        .map(|e| e.error_message)
        .unwrap_or_default()
}

/// Войти логином (ник или e-mail) и паролем. `totp` — код двухфакторной защиты, если нужен.
pub async fn login_password(
    store: &AccountStore<'_>,
    login: &str,
    password: &str,
    totp: Option<&str>,
) -> Result<PasswordLogin> {
    store.ensure_ely_allowed()?;
    if login.trim().is_empty() || password.is_empty() {
        return Err(Error::Auth("Введите логин и пароль Ely.by".into()));
    }
    let client_token = format!("{:032x}", rand::thread_rng().r#gen::<u128>());
    let secret = match totp.map(str::trim).filter(|t| !t.is_empty()) {
        Some(code) => format!("{password}:{code}"),
        None => password.to_string(),
    };
    let body = serde_json::json!({
        "username": login.trim(),
        "password": secret,
        "clientToken": client_token,
        "requestUser": true,
    });
    let text = match ygg_post("/auth/authenticate", body).await? {
        Ok(t) => t,
        Err((401, body)) if ygg_message(&body).contains("two factor") => {
            return Ok(PasswordLogin::NeedTotp);
        }
        Err((401 | 403, body)) => {
            let msg = ygg_message(&body);
            return Err(Error::Auth(if msg.is_empty() {
                "Ely.by: неверный логин или пароль".into()
            } else {
                format!("Ely.by: {msg}")
            }));
        }
        Err((s, _)) => {
            return Err(Error::Auth(format!(
                "Ely.by ответил HTTP {s}. Сервис может быть недоступен — попробуйте позже."
            )));
        }
    };
    let auth: YggAuth = serde_json::from_str(&text).map_err(|e| Error::parse("ответ Ely.by", e))?;
    let profile = auth
        .selected_profile
        .ok_or_else(|| Error::Auth("У аккаунта Ely.by нет игрового профиля".into()))?;
    let account = AccountRef {
        provider: Provider::ElyBy,
        uuid: profile.id.replace('-', ""),
        name: profile.name.clone(),
        owns_java_edition: false,
        checked_at: now(),
        skin_url: Some(crate::skin::ely_skin_url(&profile.name)),
        skin_variant: None,
    };
    let tokens = Tokens {
        access_token: auth.access_token,
        // Срок жизни токена документация не указывает — перед запуском он проверяется (/auth/validate).
        expires_at: now() + 86_400,
        refresh_token: Some(auth.client_token),
        xuid: None,
        method: Some("yggdrasil".into()),
    };
    store.upsert(account.clone(), &tokens)?;
    Ok(PasswordLogin::Done(account))
}

/// Yggdrasil: проверить токен, при необходимости обновить по clientToken.
async fn refresh_yggdrasil(store: &AccountStore<'_>, key: &str, tokens: Tokens) -> Result<Tokens> {
    let valid = ygg_post(
        "/auth/validate",
        serde_json::json!({ "accessToken": tokens.access_token }),
    )
    .await?;
    if valid.is_ok() {
        return Ok(tokens);
    }
    let client_token = tokens
        .refresh_token
        .clone()
        .ok_or_else(|| Error::Auth("Сессия Ely.by истекла. Войдите ещё раз.".into()))?;
    let body = serde_json::json!({ "accessToken": tokens.access_token, "clientToken": client_token, "requestUser": false });
    let text = ygg_post("/auth/refresh", body).await?.map_err(|(_, _)| {
        Error::Auth("Сессия Ely.by истекла. Войдите ещё раз (нужен пароль).".into())
    })?;
    let auth: YggAuth = serde_json::from_str(&text).map_err(|e| Error::parse("ответ Ely.by", e))?;
    let updated = Tokens {
        access_token: auth.access_token,
        expires_at: now() + 86_400,
        refresh_token: Some(auth.client_token),
        xuid: None,
        method: Some("yggdrasil".into()),
    };
    store.update_tokens(key, &updated)?;
    Ok(updated)
}

/// Обновить токен Ely.by заранее (до истечения меньше часа).
pub async fn refresh_if_needed(store: &AccountStore<'_>, account: &AccountRef) -> Result<Tokens> {
    let key = account.key();
    let tokens = store
        .tokens(&key)?
        .ok_or_else(|| Error::Auth("Токен Ely.by не найден. Войдите ещё раз.".into()))?;
    if tokens.method.as_deref() == Some("yggdrasil") {
        return refresh_yggdrasil(store, &key, tokens).await;
    }
    if !tokens.needs_refresh(now()) {
        return Ok(tokens);
    }
    let refresh = tokens
        .refresh_token
        .clone()
        .ok_or_else(|| Error::Auth("Сессия Ely.by истекла. Войдите ещё раз.".into()))?;
    let fresh = post_token(&[
        ("grant_type", "refresh_token"),
        ("client_id", CLIENT_ID),
        ("refresh_token", &refresh),
        ("scope", SCOPES),
    ])
    .await?;
    let updated = Tokens {
        access_token: fresh.access_token,
        expires_at: now() + fresh.expires_in.unwrap_or(86_400),
        // Ely.by не возвращает новый refresh token — старый «не истекает» (документация).
        refresh_token: fresh.refresh_token.or(Some(refresh)),
        xuid: None,
        method: Some("oauth".into()),
    };
    store.update_tokens(&key, &updated)?;
    Ok(updated)
}

/// authlib-injector закреплённой версии, проверенный по SHA-256.
pub async fn ensure_authlib_injector(data: &DataDir, cancel: &Cancel) -> Result<PathBuf> {
    let dest = data
        .libraries()
        .join("moe/yushi/authlibinjector")
        .join(AUTHLIB_VERSION)
        .join(format!("authlib-injector-{AUTHLIB_VERSION}.jar"));
    download::download_all(
        vec![Task {
            url: AUTHLIB_URL.into(),
            dest: dest.clone(),
            expected: Expected::Sha256(AUTHLIB_SHA256.into()),
            size: None,
        }],
        1,
        Verify::Full,
        None,
        cancel,
    )
    .await?;
    Ok(dest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_matches_rfc7636_example() {
        // RFC 7636, приложение B.
        assert_eq!(
            pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn token_errors_are_explained() {
        let e = explain_token_error(
            401,
            r#"{"error":"invalid_client","error_description":"Client authentication failed"}"#,
        );
        assert!(e.to_string().contains("без секрета"));
        let e = explain_token_error(503, "<html>Out of service</html>");
        assert!(e.to_string().contains("503"));
    }

    #[test]
    fn yggdrasil_error_message_is_read() {
        assert_eq!(
            ygg_message(
                r#"{"error":"ForbiddenOperationException","errorMessage":"Account protected with two factor auth."}"#
            ),
            "Account protected with two factor auth."
        );
        assert_eq!(ygg_message("not json"), "");
    }

    #[tokio::test]
    async fn password_login_is_blocked_without_microsoft() {
        let root = std::env::temp_dir().join(format!("quarry-ely-pw-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let data = DataDir::open(&root).unwrap();
        let secrets = super::super::MemoryStore::default();
        let store = AccountStore::new(&data, &secrets);
        let res = login_password(&store, "steve", "pw", None).await;
        assert!(matches!(res, Err(Error::Auth(_))));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[tokio::test]
    async fn login_is_blocked_without_microsoft() {
        let root = std::env::temp_dir().join(format!("quarry-ely-gate-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let data = DataDir::open(&root).unwrap();
        let secrets = super::super::MemoryStore::default();
        let store = AccountStore::new(&data, &secrets);
        let opened = std::sync::atomic::AtomicBool::new(false);
        let res = login(
            &store,
            |_| {
                opened.store(true, std::sync::atomic::Ordering::SeqCst);
                Ok(())
            },
            &Cancel::new(),
        )
        .await;
        assert!(matches!(res, Err(Error::Auth(_))));
        assert!(
            !opened.load(std::sync::atomic::Ordering::SeqCst),
            "браузер даже не открывается"
        );
        std::fs::remove_dir_all(&root).unwrap();
    }
}
