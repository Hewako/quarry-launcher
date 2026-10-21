//! Аккаунты. Ключ — провайдер + UUID: Microsoft и Ely.by — разные пространства идентичности,
//! их токены не смешиваются. В `accounts.toml` только провайдер, UUID, ник и факт владения;
//! токены — в хранилище секретов ОС (macOS Keychain, Windows Credential Manager, Secret Service).

pub mod ely;
pub mod loopback;
pub mod microsoft;

use crate::error::{Error, Result};
use crate::paths::{DataDir, write_atomic};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Microsoft,
    #[serde(rename = "elyby")]
    ElyBy,
}

impl Provider {
    pub fn key(self) -> &'static str {
        match self {
            Provider::Microsoft => "microsoft",
            Provider::ElyBy => "elyby",
        }
    }
    pub fn display(self) -> &'static str {
        match self {
            Provider::Microsoft => "Microsoft",
            Provider::ElyBy => "Ely.by",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountRef {
    pub provider: Provider,
    /// UUID профиля Minecraft без дефисов.
    pub uuid: String,
    pub name: String,
    /// Для Microsoft: владение Java Edition подтверждено через entitlements.
    /// Для Ely.by всегда `false`: Ely.by покупку игры не подтверждает.
    #[serde(default)]
    pub owns_java_edition: bool,
    /// Когда владение проверялось последний раз (Unix-время).
    #[serde(default)]
    pub checked_at: u64,
    /// Адрес текущего скина (для показа аватара). Для Ely.by — система скинов по нику.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skin_url: Option<String>,
    /// Модель скина: `classic` или `slim`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skin_variant: Option<String>,
}

impl AccountRef {
    pub fn key(&self) -> String {
        account_key(self.provider, &self.uuid)
    }
}

pub fn account_key(provider: Provider, uuid: &str) -> String {
    format!("{}:{}", provider.key(), uuid)
}

/// Токены аккаунта. Хранятся только в хранилище секретов ОС.
#[derive(Clone, Serialize, Deserialize)]
pub struct Tokens {
    /// Токен для игры (`--accessToken`).
    pub access_token: String,
    /// Unix-время истечения токена игры.
    pub expires_at: u64,
    /// Токен обновления у провайдера (Microsoft OAuth или Ely.by).
    #[serde(default)]
    pub refresh_token: Option<String>,
    /// XUID (Microsoft) для `--xuid`.
    #[serde(default)]
    pub xuid: Option<String>,
}

impl std::fmt::Debug for Tokens {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tokens")
            .field("access_token", &crate::logmask::TOKEN)
            .field("expires_at", &self.expires_at)
            .field(
                "refresh_token",
                &self.refresh_token.as_ref().map(|_| crate::logmask::TOKEN),
            )
            .finish()
    }
}

impl Tokens {
    /// Пора обновлять: до истечения меньше часа (обновляем заранее).
    pub fn needs_refresh(&self, now: u64) -> bool {
        self.expires_at <= now + 3600
    }
}

/// Провайдер входа. Вход у каждого свой (код Microsoft, браузер Ely.by), а получение
/// действующей сессии для игры — общий шаг запуска.
#[allow(async_fn_in_trait)]
pub trait AccountProvider {
    fn provider(&self) -> Provider;
    /// Действующие токены (обновлённые заранее, если истекают) и актуальные данные аккаунта.
    async fn session(
        &self,
        store: &AccountStore<'_>,
        account: &AccountRef,
    ) -> Result<(AccountRef, Tokens)>;
}

pub struct MicrosoftProvider;
pub struct ElyByProvider;

impl AccountProvider for MicrosoftProvider {
    fn provider(&self) -> Provider {
        Provider::Microsoft
    }
    async fn session(
        &self,
        store: &AccountStore<'_>,
        account: &AccountRef,
    ) -> Result<(AccountRef, Tokens)> {
        microsoft::refresh_if_needed(store, account).await
    }
}

impl AccountProvider for ElyByProvider {
    fn provider(&self) -> Provider {
        Provider::ElyBy
    }
    async fn session(
        &self,
        store: &AccountStore<'_>,
        account: &AccountRef,
    ) -> Result<(AccountRef, Tokens)> {
        store.ensure_ely_allowed()?;
        let tokens = ely::refresh_if_needed(store, account).await?;
        Ok((account.clone(), tokens))
    }
}

/// Сессия для аккаунта любого провайдера.
pub async fn session(
    store: &AccountStore<'_>,
    account: &AccountRef,
) -> Result<(AccountRef, Tokens)> {
    match account.provider {
        Provider::Microsoft => MicrosoftProvider.session(store, account).await,
        Provider::ElyBy => ElyByProvider.session(store, account).await,
    }
}

/// Хранилище секретов. В тестах — в памяти, в программе — системное.
pub trait SecretStore: Send + Sync {
    fn get(&self, key: &str) -> Result<Option<String>>;
    fn set(&self, key: &str, value: &str) -> Result<()>;
    fn delete(&self, key: &str) -> Result<()>;
}

const KEYRING_SERVICE: &str = "quarry-launcher";

pub struct KeyringStore;

impl SecretStore for KeyringStore {
    fn get(&self, key: &str) -> Result<Option<String>> {
        let entry = keyring::Entry::new(KEYRING_SERVICE, key).map_err(keyring_err)?;
        match entry.get_password() {
            Ok(v) => Ok(Some(v)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(keyring_err(e)),
        }
    }
    fn set(&self, key: &str, value: &str) -> Result<()> {
        keyring::Entry::new(KEYRING_SERVICE, key)
            .and_then(|e| e.set_password(value))
            .map_err(keyring_err)
    }
    fn delete(&self, key: &str) -> Result<()> {
        let entry = keyring::Entry::new(KEYRING_SERVICE, key).map_err(keyring_err)?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(keyring_err(e)),
        }
    }
}

fn keyring_err(e: keyring::Error) -> Error {
    Error::Auth(format!(
        "Хранилище секретов ОС недоступно: {e}. Токены не сохраняются в файлы, поэтому вход невозможен без него."
    ))
}

#[derive(Default)]
pub struct MemoryStore(Mutex<HashMap<String, String>>);

impl SecretStore for MemoryStore {
    fn get(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .0
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(key)
            .cloned())
    }
    fn set(&self, key: &str, value: &str) -> Result<()> {
        self.0
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(key.to_string(), value.to_string());
        Ok(())
    }
    fn delete(&self, key: &str) -> Result<()> {
        self.0.lock().unwrap_or_else(|p| p.into_inner()).remove(key);
        Ok(())
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct AccountsFile {
    #[serde(default)]
    accounts: Vec<AccountRef>,
}

pub struct AccountStore<'a> {
    data: &'a DataDir,
    secrets: &'a dyn SecretStore,
}

impl<'a> AccountStore<'a> {
    pub fn new(data: &'a DataDir, secrets: &'a dyn SecretStore) -> Self {
        Self { data, secrets }
    }

    pub fn list(&self) -> Result<Vec<AccountRef>> {
        let path = self.data.accounts_toml();
        match std::fs::read_to_string(&path) {
            Ok(text) => Ok(toml::from_str::<AccountsFile>(&text)
                .map_err(|e| Error::parse("accounts.toml", e))?
                .accounts),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(e) => Err(Error::io(&path, e)),
        }
    }

    fn write(&self, accounts: Vec<AccountRef>) -> Result<()> {
        let text = toml::to_string_pretty(&AccountsFile { accounts })
            .map_err(|e| Error::parse("accounts.toml", e))?;
        write_atomic(&self.data.accounts_toml(), text.as_bytes())
    }

    pub fn get(&self, key: &str) -> Result<Option<AccountRef>> {
        Ok(self.list()?.into_iter().find(|a| a.key() == key))
    }

    /// Добавить или обновить аккаунт вместе с токенами. Токены пишутся первыми: если
    /// хранилище секретов недоступно, аккаунт не появится в списке без токенов.
    pub fn upsert(&self, account: AccountRef, tokens: &Tokens) -> Result<()> {
        let json = serde_json::to_string(tokens).map_err(|e| Error::parse("токены", e))?;
        self.secrets.set(&account.key(), &json)?;
        let mut list = self.list()?;
        list.retain(|a| a.key() != account.key());
        list.push(account);
        self.write(list)
    }

    pub fn update_tokens(&self, key: &str, tokens: &Tokens) -> Result<()> {
        let json = serde_json::to_string(tokens).map_err(|e| Error::parse("токены", e))?;
        self.secrets.set(key, &json)
    }

    pub fn tokens(&self, key: &str) -> Result<Option<Tokens>> {
        match self.secrets.get(key)? {
            Some(json) => Ok(serde_json::from_str(&json).ok()),
            None => Ok(None),
        }
    }

    pub fn remove(&self, key: &str) -> Result<()> {
        self.secrets.delete(key)?;
        let mut list = self.list()?;
        list.retain(|a| a.key() != key);
        self.write(list)
    }

    /// Политика проекта: Ely.by доступен только при Microsoft-аккаунте с подтверждённым
    /// владением Java Edition (план, 5.9).
    pub fn has_verified_microsoft(&self) -> Result<bool> {
        Ok(self
            .list()?
            .iter()
            .any(|a| a.provider == Provider::Microsoft && a.owns_java_edition))
    }

    pub fn ensure_ely_allowed(&self) -> Result<()> {
        if self.has_verified_microsoft()? {
            Ok(())
        } else {
            Err(Error::Auth(
                "Ely.by доступен только вместе с Microsoft-аккаунтом, на котором куплена Minecraft: Java Edition. Сначала войдите через Microsoft."
                    .into(),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data(name: &str) -> (DataDir, std::path::PathBuf) {
        let root = std::env::temp_dir().join(format!("quarry-acc-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        (DataDir::open(&root).unwrap(), root)
    }

    fn tokens(t: &str) -> Tokens {
        Tokens {
            access_token: t.into(),
            expires_at: 100,
            refresh_token: Some("r".into()),
            xuid: None,
        }
    }

    #[test]
    fn providers_do_not_mix_and_tokens_are_not_in_files() {
        let (d, root) = data("mix");
        let secrets = MemoryStore::default();
        let store = AccountStore::new(&d, &secrets);
        let ms = AccountRef {
            provider: Provider::Microsoft,
            uuid: "abc".into(),
            name: "Steve".into(),
            owns_java_edition: true,
            checked_at: 1,
            skin_url: None,
            skin_variant: None,
        };
        let ely = AccountRef {
            provider: Provider::ElyBy,
            owns_java_edition: false,
            ..ms.clone()
        };
        store
            .upsert(ms.clone(), &tokens("ms-token-123456"))
            .unwrap();
        store
            .upsert(ely.clone(), &tokens("ely-token-123456"))
            .unwrap();
        assert_eq!(
            store.list().unwrap().len(),
            2,
            "одинаковый UUID у разных провайдеров — разные аккаунты"
        );
        assert_eq!(
            store.tokens(&ms.key()).unwrap().unwrap().access_token,
            "ms-token-123456"
        );
        assert_eq!(
            store.tokens(&ely.key()).unwrap().unwrap().access_token,
            "ely-token-123456"
        );
        let file = std::fs::read_to_string(d.accounts_toml()).unwrap();
        assert!(!file.contains("token"), "токенов в accounts.toml нет");
        store.remove(&ely.key()).unwrap();
        assert!(store.tokens(&ely.key()).unwrap().is_none());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn ely_requires_verified_microsoft() {
        let (d, root) = data("gate");
        let secrets = MemoryStore::default();
        let store = AccountStore::new(&d, &secrets);
        assert!(store.ensure_ely_allowed().is_err());
        let unverified = AccountRef {
            provider: Provider::Microsoft,
            uuid: "u".into(),
            name: "N".into(),
            owns_java_edition: false,
            checked_at: 0,
            skin_url: None,
            skin_variant: None,
        };
        store.upsert(unverified.clone(), &tokens("t")).unwrap();
        assert!(
            store.ensure_ely_allowed().is_err(),
            "Microsoft без покупки не открывает Ely.by"
        );
        store
            .upsert(
                AccountRef {
                    owns_java_edition: true,
                    ..unverified
                },
                &tokens("t"),
            )
            .unwrap();
        assert!(store.ensure_ely_allowed().is_ok());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn tokens_debug_is_masked() {
        let t = tokens("very-secret-access-token");
        assert!(!format!("{t:?}").contains("very-secret"));
        assert!(t.needs_refresh(0));
        assert!(
            !Tokens {
                expires_at: 100_000,
                ..t
            }
            .needs_refresh(0)
        );
    }
}
