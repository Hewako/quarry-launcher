pub mod ely;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AccountKind {
    Microsoft,
    ElyBy,
    Offline,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountRef {
    pub kind: AccountKind,
    pub uuid: String,
    pub username: String,
}

pub fn save_active_account(acc: &AccountRef) -> Result<()> {
    let home = dirs::home_dir().context("Не удалось найти домашнюю папку")?;
    let dir = home.join(".quarry");
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join("active_account.toml"), toml::to_string(acc)?)?;
    Ok(())
}

pub fn load_active_account() -> Result<Option<AccountRef>> {
    let home = match dirs::home_dir() {
        Some(h) => h,
        None => return Ok(None),
    };
    let path = home.join(".quarry").join("active_account.toml");
    if !path.exists() {
        return Ok(None);
    }
    Ok(Some(toml::from_str(&std::fs::read_to_string(path)?)?))
}

pub fn save_token_to_keyring(account_key: &str, token: &str) -> Result<()> {
    let entry = keyring::Entry::new("quarry-launcher", account_key)?;
    entry.set_password(token)?;
    Ok(())
}

pub fn get_token_from_keyring(account_key: &str) -> Result<String> {
    let entry = keyring::Entry::new("quarry-launcher", account_key)?;
    Ok(entry.get_password()?)
}
