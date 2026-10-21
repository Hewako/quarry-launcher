//! Правила Mod Doctor как данные: пары «вместе не работают», которых нет в метаданных модов.
//! Встроенный файл — `core/rules/doctor-rules.json`; свежую версию лаунчер берёт из
//! репозитория не чаще раза в сутки и строго проверяет (только данные, без кода).

use crate::error::{Error, Result};
use crate::paths::{DataDir, write_atomic};
use serde::Deserialize;
use std::sync::{Arc, RwLock};

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Conflict {
    pub a: String,
    pub b: String,
    pub why: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Rules {
    pub version: u32,
    #[serde(default)]
    pub updated: String,
    pub conflicts: Vec<Conflict>,
}

const BUILTIN: &str = include_str!("../../rules/doctor-rules.json");
const URL: &str =
    "https://raw.githubusercontent.com/Hewako/quarry-launcher/main/core/rules/doctor-rules.json";
const CACHE: &str = "doctor-rules.json";

/// Разобрать и проверить файл правил: известная версия формата, ID модов — только
/// латиница/цифры/`-`/`_`, пояснение не длиннее 300 символов.
pub fn parse(text: &str) -> Result<Rules> {
    let r: Rules = serde_json::from_str(text).map_err(|e| Error::parse("правила Mod Doctor", e))?;
    let id_ok = |s: &str| {
        !s.is_empty()
            && s.len() <= 64
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    };
    if r.version != 1 || r.conflicts.len() > 1000 {
        return Err(Error::invalid("Неизвестный формат правил Mod Doctor"));
    }
    if let Some(c) = r
        .conflicts
        .iter()
        .find(|c| !id_ok(&c.a) || !id_ok(&c.b) || c.why.chars().count() > 300)
    {
        return Err(Error::invalid(format!(
            "Неверное правило Mod Doctor: {} / {}",
            c.a, c.b
        )));
    }
    Ok(r)
}

fn slot() -> &'static RwLock<Arc<Rules>> {
    static S: std::sync::OnceLock<RwLock<Arc<Rules>>> = std::sync::OnceLock::new();
    S.get_or_init(|| {
        RwLock::new(Arc::new(
            parse(BUILTIN).expect("встроенные правила корректны"),
        ))
    })
}

/// Текущие правила: из кеша, если он загружен, иначе встроенные.
pub fn current() -> Arc<Rules> {
    slot().read().unwrap_or_else(|p| p.into_inner()).clone()
}

/// Подхватить правила из кеша (если файл есть и корректен).
pub fn load_cached(data: &DataDir) {
    if let Ok(text) = std::fs::read_to_string(data.meta().join(CACHE))
        && let Ok(r) = parse(&text)
    {
        *slot().write().unwrap_or_else(|p| p.into_inner()) = Arc::new(r);
    }
}

/// Скачать свежие правила (не чаще раза в сутки). `Ok(true)` — правила обновились.
pub async fn refresh(data: &DataDir) -> Result<bool> {
    let path = data.meta().join(CACHE);
    let fresh = std::fs::metadata(&path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .is_some_and(|age| age.as_secs() < 86_400);
    if fresh {
        return Ok(false);
    }
    let text = crate::net::get_text(URL).await?;
    let rules = parse(&text)?;
    std::fs::create_dir_all(data.meta()).map_err(|e| Error::io(data.meta(), e))?;
    write_atomic(&path, text.as_bytes())?;
    let changed = *current() != rules;
    *slot().write().unwrap_or_else(|p| p.into_inner()) = Arc::new(rules);
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_rules_are_valid() {
        let r = parse(include_str!("../../rules/doctor-rules.json")).unwrap();
        assert!(
            r.conflicts
                .iter()
                .any(|c| c.a == "optifine" && c.b == "sodium")
        );
    }

    #[test]
    fn bad_rules_are_rejected() {
        assert!(parse("не json").is_err());
        assert!(
            parse(r#"{"version":2,"conflicts":[]}"#).is_err(),
            "неизвестная версия формата"
        );
        assert!(parse(r#"{"version":1,"conflicts":[{"a":"../x","b":"y","why":"w"}]}"#).is_err());
        let long = "w".repeat(400);
        assert!(
            parse(&format!(
                r#"{{"version":1,"conflicts":[{{"a":"x","b":"y","why":"{long}"}}]}}"#
            ))
            .is_err()
        );
    }
}
