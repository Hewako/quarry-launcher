//! Хуки запуска: свои команды до запуска, после выхода и после краша; обёртка (Linux:
//! gamemoderun, mangohud, prime-run). Команда выполняется, только если игрок явно
//! подтвердил именно эти строки — «отпечаток» согласия хранится рядом с ними.

use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;
use std::time::Duration;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hooks {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pre: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub post_exit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub post_crash: Option<String>,
    /// Обёртка перед Java (только Linux), например `gamemoderun`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wrapper: Option<String>,
    /// Отпечаток команд, которые игрок подтвердил.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approved: Option<String>,
}

impl Hooks {
    pub fn fingerprint(&self) -> String {
        let f = |o: &Option<String>| o.clone().unwrap_or_default();
        let text = format!(
            "{}\n{}\n{}\n{}",
            f(&self.pre),
            f(&self.post_exit),
            f(&self.post_crash),
            f(&self.wrapper)
        );
        hex::encode(Sha256::digest(text.as_bytes()))
    }
    pub fn is_empty(&self) -> bool {
        [&self.pre, &self.post_exit, &self.post_crash, &self.wrapper]
            .iter()
            .all(|o| o.as_deref().is_none_or(|s| s.trim().is_empty()))
    }
    /// Подтверждены ли текущие команды.
    pub fn is_approved(&self) -> bool {
        !self.is_empty() && self.approved.as_deref() == Some(self.fingerprint().as_str())
    }
}

/// Выполнить команду в папке игры (до 60 секунд). Возвращает код выхода и вывод.
pub async fn run(cmd: &str, cwd: &Path, profile: &str) -> Result<(i32, String)> {
    let mut c = if cfg!(windows) {
        let mut c = tokio::process::Command::new("cmd");
        c.args(["/C", cmd]);
        #[cfg(windows)]
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW: без мигающей консоли
        c
    } else {
        let mut c = tokio::process::Command::new("sh");
        c.args(["-c", cmd]);
        c
    };
    c.current_dir(cwd)
        .env("QUARRY_PROFILE", profile)
        .env("QUARRY_GAME_DIR", cwd)
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true);
    let out = tokio::time::timeout(Duration::from_secs(60), c.output())
        .await
        .map_err(|_| Error::invalid(format!("Команда не завершилась за 60 секунд: {cmd}")))?
        .map_err(|e| Error::invalid(format!("Команда не запустилась ({cmd}): {e}")))?;
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    let masker = crate::logmask::Masker::new();
    let text: String = text
        .lines()
        .map(|l| masker.mask(l))
        .collect::<Vec<_>>()
        .join("\n")
        .chars()
        .take(4000)
        .collect();
    Ok((out.status.code().unwrap_or(-1), text))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approval_follows_exact_commands() {
        let mut h = Hooks {
            pre: Some("echo привет".into()),
            ..Default::default()
        };
        assert!(!h.is_approved(), "без согласия не выполняется");
        h.approved = Some(h.fingerprint());
        assert!(h.is_approved());
        h.post_exit = Some("rm -rf ~".into());
        assert!(!h.is_approved(), "любая правка требует нового согласия");
        assert!(Hooks::default().is_empty() && !h.is_empty());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn runs_in_game_dir_with_profile_env() {
        let d = std::env::temp_dir().join(format!("quarry-hook-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let (code, out) = run("pwd; echo \"$QUARRY_PROFILE\"; exit 3", &d, "мой-профиль")
            .await
            .unwrap();
        assert_eq!(code, 3);
        assert!(
            out.contains("мой-профиль") && out.contains(d.file_name().unwrap().to_str().unwrap()),
            "{out}"
        );
        std::fs::remove_dir_all(&d).unwrap();
    }
}
