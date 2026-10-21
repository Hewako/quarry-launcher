//! Вход Microsoft: OAuth (device code, тенант `consumers`) → Xbox Live → XSTS →
//! Minecraft Services → проверка владения (entitlements) → профиль.
//! Источники: learn.microsoft.com (device code, refresh) и minecraft.wiki/w/Microsoft_authentication
//! (сверено 21.10.2026, см. docs/RESEARCH.md). Коды XSTS — из неофициальной документации.
//!
//! Пока Mojang не одобрил client ID, `login_with_xbox` отвечает 403 — это ожидаемо (план, 6.2).

use super::{AccountRef, AccountStore, Provider, Tokens};
use crate::download::Cancel;
use crate::error::{Error, Result};
use crate::instance::now;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::time::Duration;

/// Client ID задаётся при сборке: `QUARRY_MS_CLIENT_ID=<id> cargo build`.
/// Чужой ID (например, другого лаунчера) использовать нельзя — см. docs/MICROSOFT_SETUP.md.
pub const CLIENT_ID: Option<&str> = option_env!("QUARRY_MS_CLIENT_ID");

const DEVICE_CODE_URL: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/devicecode";
const TOKEN_URL: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/token";
const XBL_URL: &str = "https://user.auth.xboxlive.com/user/authenticate";
const XSTS_URL: &str = "https://xsts.auth.xboxlive.com/xsts/authorize";
const MC_LOGIN_URL: &str = "https://api.minecraftservices.com/authentication/login_with_xbox";
const MC_ENTITLEMENTS_URL: &str = "https://api.minecraftservices.com/entitlements/mcstore";
const MC_PROFILE_URL: &str = "https://api.minecraftservices.com/minecraft/profile";
const SCOPE: &str = "XboxLive.signin offline_access";

pub fn client_id() -> Result<&'static str> {
    CLIENT_ID.filter(|s| !s.trim().is_empty()).ok_or_else(|| {
        Error::Auth(
            "Вход Microsoft не настроен в этой сборке: нет client ID приложения Azure. Инструкция — docs/MICROSOFT_SETUP.md."
                .into(),
        )
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceCode {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: u64,
    pub interval: u64,
    #[serde(default)]
    pub message: String,
}

#[derive(Deserialize)]
struct MsToken {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
}

#[derive(Deserialize)]
struct MsError {
    error: String,
    #[serde(default)]
    error_description: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct XboxResponse {
    token: String,
    display_claims: DisplayClaims,
}
#[derive(Deserialize)]
struct DisplayClaims {
    xui: Vec<Xui>,
}
#[derive(Deserialize)]
struct Xui {
    uhs: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct XstsError {
    #[serde(rename = "XErr")]
    xerr: u64,
}

#[derive(Deserialize)]
struct McLogin {
    access_token: String,
    #[serde(default)]
    expires_in: Option<u64>,
}

#[derive(Deserialize)]
struct Entitlements {
    #[serde(default)]
    items: Vec<EntitlementItem>,
}
#[derive(Deserialize)]
struct EntitlementItem {
    name: String,
}

#[derive(Deserialize)]
struct McProfile {
    id: String,
    name: String,
    #[serde(default)]
    skins: Vec<McSkin>,
}

#[derive(Deserialize)]
struct McSkin {
    #[serde(default)]
    state: String,
    url: String,
    #[serde(default)]
    variant: Option<String>,
}

impl McProfile {
    /// Активный скин. Адреса textures.minecraft.net приходят по http — переводим на https
    /// (сервер отдаёт текстуры и по https; не подтверждено первоисточником).
    fn active_skin(&self) -> Option<(String, Option<String>)> {
        let s = self
            .skins
            .iter()
            .find(|s| s.state == "ACTIVE")
            .or(self.skins.first())?;
        let url = s.url.replacen("http://", "https://", 1);
        Some((url, s.variant.as_ref().map(|v| v.to_ascii_lowercase())))
    }
}

/// Объяснение кода XSTS для пользователя (план, 5.8; коды — неофициальные).
pub fn xsts_message(xerr: u64) -> String {
    match xerr {
        2148916227 => "Аккаунт заблокирован в Xbox. Подробности — в поддержке Xbox.".into(),
        2148916233 => "У этой учётной записи Microsoft нет профиля Xbox. Создайте его на xbox.com (или войдите один раз на minecraft.net) и повторите вход.".into(),
        2148916235 => "Xbox Live недоступен в стране, указанной в учётной записи Microsoft.".into(),
        2148916236 | 2148916237 => "Нужно подтвердить возраст на странице Xbox (требование для Южной Кореи), затем повторить вход.".into(),
        2148916238 => "Это детская учётная запись: взрослый должен добавить её в семейную группу Microsoft (account.microsoft.com/family).".into(),
        other => format!("Xbox отклонил вход (код XSTS {other}). Попробуйте позже."),
    }
}

/// Владеет ли аккаунт Java Edition по списку entitlements (wiki: product_minecraft / game_minecraft;
/// Game Pass — product_game_pass_pc / product_game_pass_ultimate).
pub fn owns_java(items: &[String]) -> bool {
    items.iter().any(|n| {
        matches!(
            n.as_str(),
            "product_minecraft"
                | "game_minecraft"
                | "product_game_pass_pc"
                | "product_game_pass_ultimate"
        )
    })
}

async fn post_json<T: for<'de> Deserialize<'de>>(
    url: &str,
    body: serde_json::Value,
) -> Result<std::result::Result<T, (u16, String)>> {
    let resp = crate::net::client()
        .post(url)
        .header("Accept", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| crate::net::network_error(url, e))?;
    let status = resp.status().as_u16();
    let text = resp.text().await.unwrap_or_default();
    if (200..300).contains(&status) {
        Ok(Ok(
            serde_json::from_str(&text).map_err(|e| Error::parse(crate::net::host_of(url), e))?
        ))
    } else {
        Ok(Err((status, text)))
    }
}

async fn get_bearer(url: &str, token: &str) -> Result<std::result::Result<String, u16>> {
    let resp = crate::net::client()
        .get(url)
        .bearer_auth(token)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| crate::net::network_error(url, e))?;
    let status = resp.status().as_u16();
    let text = resp.text().await.unwrap_or_default();
    Ok(if (200..300).contains(&status) {
        Ok(text)
    } else {
        Err(status)
    })
}

/// Шаг 1: получить код для ввода на microsoft.com/link.
pub async fn start_device_code() -> Result<DeviceCode> {
    let id = client_id()?;
    let resp = crate::net::client()
        .post(DEVICE_CODE_URL)
        .form(&[("client_id", id), ("scope", SCOPE)])
        .send()
        .await
        .map_err(|e| crate::net::network_error(DEVICE_CODE_URL, e))?;
    let ok = resp.status().is_success();
    let text = resp.text().await.unwrap_or_default();
    if !ok {
        return Err(device_code_error(&text));
    }
    serde_json::from_str(&text).map_err(|e| Error::parse("ответ Microsoft (device code)", e))
}

fn device_code_error(body: &str) -> Error {
    let err: Option<MsError> = serde_json::from_str(body).ok();
    match err {
        // Проверено 21.10.2026: на неизвестный ID приходит unauthorized_client (AADSTS700038/700016).
        Some(e) if e.error == "unauthorized_client" || e.error == "invalid_client" => Error::Auth(
            "Microsoft не принял client ID этой сборки: приложение не найдено или для него не разрешены public client flows. См. docs/MICROSOFT_SETUP.md, шаги 1.5–1.6."
                .into(),
        ),
        Some(e) => Error::Auth(format!("Microsoft: {}", e.error_description.unwrap_or(e.error))),
        None => Error::Auth("Microsoft вернул непонятный ответ на запрос кода входа".into()),
    }
}

/// Шаг 2: ждать, пока пользователь подтвердит вход в браузере.
async fn poll_device_code(code: &DeviceCode, cancel: &Cancel) -> Result<MsToken> {
    let id = client_id()?;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(code.expires_in.max(60));
    let mut interval = code.interval.max(1);
    loop {
        cancel.check()?;
        tokio::time::sleep(Duration::from_secs(interval)).await;
        if tokio::time::Instant::now() >= deadline {
            return Err(Error::Auth(
                "Код входа Microsoft истёк. Начните вход заново.".into(),
            ));
        }
        let resp = crate::net::client()
            .post(TOKEN_URL)
            .form(&[
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                ("client_id", id),
                ("device_code", code.device_code.as_str()),
            ])
            .send()
            .await
            .map_err(|e| crate::net::network_error(TOKEN_URL, e))?;
        let ok = resp.status().is_success();
        let text = resp.text().await.unwrap_or_default();
        if ok {
            return serde_json::from_str(&text).map_err(|e| Error::parse("токен Microsoft", e));
        }
        let err: MsError = serde_json::from_str(&text)
            .map_err(|_| Error::Auth("Microsoft вернул непонятный ответ".into()))?;
        match err.error.as_str() {
            "authorization_pending" => {}
            // RFC 8628; на странице Microsoft не описано — не подтверждено первоисточником.
            "slow_down" => interval += 5,
            "authorization_declined" => {
                return Err(Error::Auth("Вход в Microsoft отклонён.".into()));
            }
            "expired_token" => {
                return Err(Error::Auth(
                    "Код входа Microsoft истёк. Начните вход заново.".into(),
                ));
            }
            _ => {
                return Err(Error::Auth(format!(
                    "Microsoft: {}",
                    err.error_description.unwrap_or(err.error)
                )));
            }
        }
    }
}

struct Chain {
    account: AccountRef,
    tokens: Tokens,
}

/// Шаги 3–6: Xbox Live → XSTS → Minecraft → владение → профиль.
async fn minecraft_chain(ms: MsToken) -> Result<Chain> {
    let xbl: XboxResponse = post_json(
        XBL_URL,
        json!({
            "Properties": {"AuthMethod": "RPS", "SiteName": "user.auth.xboxlive.com", "RpsTicket": format!("d={}", ms.access_token)},
            "RelyingParty": "http://auth.xboxlive.com",
            "TokenType": "JWT"
        }),
    )
    .await?
    .map_err(|(s, _)| Error::Auth(format!("Xbox Live отклонил вход (HTTP {s}). Попробуйте позже.")))?;
    let uhs = xbl
        .display_claims
        .xui
        .first()
        .map(|x| x.uhs.clone())
        .ok_or_else(|| Error::Auth("Xbox Live не вернул идентификатор пользователя".into()))?;

    let xsts: XboxResponse = match post_json::<XboxResponse>(
        XSTS_URL,
        json!({
            "Properties": {"SandboxId": "RETAIL", "UserTokens": [xbl.token]},
            "RelyingParty": "rp://api.minecraftservices.com/",
            "TokenType": "JWT"
        }),
    )
    .await?
    {
        Ok(x) => x,
        Err((401, body)) => {
            let xerr = serde_json::from_str::<XstsError>(&body)
                .map(|e| e.xerr)
                .unwrap_or(0);
            return Err(Error::Auth(xsts_message(xerr)));
        }
        Err((s, _)) => return Err(Error::Auth(format!("Xbox (XSTS) ответил HTTP {s}"))),
    };

    let login: McLogin = match post_json::<McLogin>(
        MC_LOGIN_URL,
        json!({"identityToken": format!("XBL3.0 x={uhs};{}", xsts.token)}),
    )
    .await?
    {
        Ok(l) => l,
        Err((403, _)) => {
            return Err(Error::Auth(
                "Minecraft Services ответил 403: приложение Quarry ещё не одобрено Mojang (Invalid app registration). Вход заработает после одобрения client ID — см. docs/MICROSOFT_SETUP.md."
                    .into(),
            ));
        }
        Err((s, _)) => {
            return Err(Error::Auth(format!(
                "Minecraft Services ответил HTTP {s}. Попробуйте позже."
            )));
        }
    };

    let ent_text = get_bearer(MC_ENTITLEMENTS_URL, &login.access_token)
        .await?
        .map_err(|s| Error::Auth(format!("Не удалось проверить покупку игры (HTTP {s})")))?;
    let ent: Entitlements =
        serde_json::from_str(&ent_text).map_err(|e| Error::parse("entitlements", e))?;
    let names: Vec<String> = ent.items.into_iter().map(|i| i.name).collect();
    let owns = owns_java(&names);

    let profile: McProfile = match get_bearer(MC_PROFILE_URL, &login.access_token).await? {
        Ok(text) => {
            serde_json::from_str(&text).map_err(|e| Error::parse("профиль Minecraft", e))?
        }
        Err(404) if owns => {
            return Err(Error::Auth(
                "У аккаунта ещё нет профиля Minecraft (так бывает с Xbox Game Pass). Войдите один раз в официальный лаунчер Minecraft, выберите ник и повторите вход здесь."
                    .into(),
            ));
        }
        Err(404) => {
            return Err(Error::Auth(
                "На этой учётной записи Microsoft не куплена Minecraft: Java Edition.".into(),
            ));
        }
        Err(s) => {
            return Err(Error::Auth(format!(
                "Не удалось получить профиль Minecraft (HTTP {s})"
            )));
        }
    };

    let skin = profile.active_skin();
    Ok(Chain {
        account: AccountRef {
            provider: Provider::Microsoft,
            uuid: profile.id.replace('-', ""),
            name: profile.name,
            owns_java_edition: owns,
            checked_at: now(),
            skin_url: skin.as_ref().map(|s| s.0.clone()),
            skin_variant: skin.and_then(|s| s.1),
        },
        tokens: Tokens {
            access_token: login.access_token,
            expires_at: now() + login.expires_in.unwrap_or(86_400),
            refresh_token: ms.refresh_token,
            // XUID из ответа XSTS для rp://api.minecraftservices.com не приходит; игра принимает
            // «0» (не подтверждено первоисточником).
            xuid: Some("0".into()),
        },
    })
}

/// Полный вход: пользователь вводит `code.user_code` на `code.verification_uri`.
pub async fn complete_login(
    store: &AccountStore<'_>,
    code: &DeviceCode,
    cancel: &Cancel,
) -> Result<AccountRef> {
    let ms = poll_device_code(code, cancel).await?;
    let chain = minecraft_chain(ms).await?;
    store.upsert(chain.account.clone(), &chain.tokens)?;
    Ok(chain.account)
}

/// Обновить токен игры заранее. Заодно перепроверяется владение.
pub async fn refresh_if_needed(
    store: &AccountStore<'_>,
    account: &AccountRef,
) -> Result<(AccountRef, Tokens)> {
    let key = account.key();
    let tokens = store
        .tokens(&key)?
        .ok_or_else(|| Error::Auth("Токен Microsoft не найден. Войдите ещё раз.".into()))?;
    if !tokens.needs_refresh(now()) {
        return Ok((account.clone(), tokens));
    }
    let refresh = tokens
        .refresh_token
        .clone()
        .ok_or_else(|| Error::Auth("Сессия Microsoft истекла. Войдите ещё раз.".into()))?;
    let id = client_id()?;
    let resp = crate::net::client()
        .post(TOKEN_URL)
        .form(&[
            ("grant_type", "refresh_token"),
            ("client_id", id),
            ("refresh_token", refresh.as_str()),
            ("scope", SCOPE),
        ])
        .send()
        .await
        .map_err(|e| crate::net::network_error(TOKEN_URL, e))?;
    let ok = resp.status().is_success();
    let text = resp.text().await.unwrap_or_default();
    if !ok {
        let err: Option<MsError> = serde_json::from_str(&text).ok();
        return Err(Error::Auth(match err.map(|e| e.error) {
            Some(e) if e == "invalid_grant" || e == "interaction_required" => {
                "Microsoft отозвал вход (пароль сменён или доступ отозван). Войдите ещё раз.".into()
            }
            _ => "Не удалось обновить вход Microsoft. Проверьте интернет и попробуйте ещё раз."
                .into(),
        }));
    }
    let ms: MsToken =
        serde_json::from_str(&text).map_err(|e| Error::parse("токен Microsoft", e))?;
    let mut chain = minecraft_chain(ms).await?;
    // Старый refresh token заменяется новым (документация Microsoft).
    if chain.tokens.refresh_token.is_none() {
        chain.tokens.refresh_token = Some(refresh);
    }
    store.upsert(chain.account.clone(), &chain.tokens)?;
    Ok((chain.account, chain.tokens))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xsts_codes_are_explained() {
        assert!(xsts_message(2148916233).contains("профиля Xbox"));
        assert!(xsts_message(2148916238).contains("детская"));
        assert!(xsts_message(2148916236).contains("возраст"));
        assert!(xsts_message(1).contains("код XSTS 1"));
    }

    #[test]
    fn unknown_client_id_is_explained() {
        let e = device_code_error(
            r#"{"error":"unauthorized_client","error_description":"AADSTS700038: 0 is not a valid application identifier."}"#,
        );
        assert!(e.to_string().contains("MICROSOFT_SETUP"));
    }

    #[test]
    fn ownership_from_entitlements() {
        assert!(owns_java(&[
            "product_minecraft".into(),
            "game_minecraft".into()
        ]));
        assert!(owns_java(&["product_game_pass_pc".into()]));
        assert!(!owns_java(&[
            "product_minecraft_bedrock".into(),
            "game_dungeons".into()
        ]));
        assert!(!owns_java(&[]));
    }

    #[test]
    fn xsts_error_body_parses() {
        let e: XstsError = serde_json::from_str(
            r#"{"Identity":"0","XErr":2148916238,"Message":"","Redirect":"https://start.ui.xboxlive.com/AddChildToFamily"}"#,
        )
        .unwrap();
        assert_eq!(e.xerr, 2148916238);
    }

    #[test]
    fn profile_skin_is_read() {
        let p: McProfile = serde_json::from_str(
            r#"{"id":"abc","name":"Steve","skins":[{"id":"1","state":"ACTIVE","url":"http://textures.minecraft.net/texture/x","variant":"SLIM","alias":"ALEX"}],"capes":[]}"#,
        )
        .unwrap();
        assert_eq!(
            p.active_skin(),
            Some((
                "https://textures.minecraft.net/texture/x".into(),
                Some("slim".into())
            ))
        );
        let none: McProfile = serde_json::from_str(r#"{"id":"a","name":"b"}"#).unwrap();
        assert!(none.active_skin().is_none());
    }

    #[test]
    fn xbox_response_parses() {
        let x: XboxResponse = serde_json::from_str(
            r#"{"IssueInstant":"2020-12-07T19:52:08Z","NotAfter":"2020-12-21T19:52:08Z","Token":"t","DisplayClaims":{"xui":[{"uhs":"123"}]}}"#,
        )
        .unwrap();
        assert_eq!(x.display_claims.xui[0].uhs, "123");
    }

    #[test]
    fn missing_client_id_is_explained() {
        if CLIENT_ID.is_none() {
            assert!(
                client_id()
                    .unwrap_err()
                    .to_string()
                    .contains("MICROSOFT_SETUP")
            );
        }
    }
}
