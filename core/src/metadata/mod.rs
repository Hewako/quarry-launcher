//! Источник метаданных Mojang: манифест версий и JSON отдельных версий.
//! Ответы кешируются в `cache/meta`, чтобы установленные версии запускались без сети.

pub mod rules;
pub mod version;

pub use rules::{Arch, Environment, Os};
pub use version::VersionJson;

use crate::error::{Error, IoContext, Result};
use crate::paths::{DataDir, write_atomic};
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};

pub const MANIFEST_URL: &str = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionManifest {
    pub latest: LatestVersions,
    pub versions: Vec<VersionEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LatestVersions {
    pub release: String,
    pub snapshot: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionEntry {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub url: String,
    #[serde(default)]
    pub sha1: Option<String>,
    pub time: String,
    pub release_time: String,
}

impl VersionManifest {
    pub fn find(&self, id: &str) -> Option<&VersionEntry> {
        self.versions.iter().find(|v| v.id == id)
    }

    /// Версии в порядке манифеста (новые сверху). Порядок берётся из данных, номер версии
    /// не разбирается: схема 26.x и 1.x сравниваются корректно.
    pub fn filtered(&self, include_snapshots: bool, include_old: bool) -> Vec<&VersionEntry> {
        self.versions
            .iter()
            .filter(|v| match v.kind.as_str() {
                "release" => true,
                "snapshot" => include_snapshots,
                _ => include_old,
            })
            .collect()
    }
}

/// Откуда получены данные: свежие из сети или из кеша (сеть недоступна).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Freshness {
    Online,
    Cached,
}

/// Получить манифест; при ошибке сети — последний сохранённый.
pub async fn manifest(data: &DataDir) -> Result<(VersionManifest, Freshness)> {
    let cache = data.meta().join("version_manifest_v2.json");
    match crate::net::get_bytes(MANIFEST_URL).await {
        Ok(bytes) => {
            let m: VersionManifest = serde_json::from_slice(&bytes)
                .map_err(|e| Error::parse("манифест версий Mojang", e))?;
            write_atomic(&cache, &bytes)?;
            Ok((m, Freshness::Online))
        }
        Err(net_err) => match std::fs::read(&cache) {
            Ok(bytes) => {
                let m = serde_json::from_slice(&bytes)
                    .map_err(|e| Error::parse("сохранённый манифест версий", e))?;
                Ok((m, Freshness::Cached))
            }
            Err(_) => Err(net_err),
        },
    }
}

/// JSON ванильной версии. Кеш проверяется по SHA-1 из манифеста, поэтому повреждённый
/// или устаревший файл будет скачан заново.
pub async fn vanilla_version(data: &DataDir, id: &str) -> Result<VersionJson> {
    let cache = data
        .meta()
        .join("versions")
        .join(format!("{}.json", file_safe(id)?));
    // Без сети: берём кеш как есть, если он вообще есть.
    let manifest = match manifest(data).await {
        Ok((m, _)) => m,
        Err(e) => {
            return match std::fs::read_to_string(&cache) {
                Ok(text) => VersionJson::parse(&text),
                Err(_) => Err(e),
            };
        }
    };
    let entry = manifest
        .find(id)
        .ok_or_else(|| Error::NotFound(format!("Версия {id} не найдена в манифесте Mojang")))?;

    if let Ok(bytes) = std::fs::read(&cache) {
        let fresh = entry
            .sha1
            .as_deref()
            .is_none_or(|expected| sha1_hex(&bytes).eq_ignore_ascii_case(expected));
        if fresh && let Ok(text) = String::from_utf8(bytes) {
            return VersionJson::parse(&text);
        }
    }

    let bytes = crate::net::get_bytes(&entry.url).await?;
    if let Some(expected) = &entry.sha1 {
        let actual = sha1_hex(&bytes);
        if !actual.eq_ignore_ascii_case(expected) {
            return Err(Error::HashMismatch {
                path: cache,
                expected: expected.clone(),
                actual,
            });
        }
    }
    let text = String::from_utf8(bytes).map_err(|e| Error::parse("JSON версии", e))?;
    let parsed = VersionJson::parse(&text)?;
    write_atomic(&cache, text.as_bytes())?;
    Ok(parsed)
}

/// Сохранить JSON загрузчика (или любой другой) в кеш метаданных и прочитать обратно.
pub fn cache_json(data: &DataDir, subdir: &str, name: &str, text: &str) -> Result<()> {
    let path = data
        .meta()
        .join(subdir)
        .join(format!("{}.json", file_safe(name)?));
    write_atomic(&path, text.as_bytes())
}

pub fn read_cached_json(data: &DataDir, subdir: &str, name: &str) -> Option<String> {
    let path = data
        .meta()
        .join(subdir)
        .join(format!("{}.json", file_safe(name).ok()?));
    std::fs::read_to_string(path).ok()
}

/// Имя версии как имя файла. ID приходят из сети, поэтому проверяем.
pub fn file_safe(id: &str) -> Result<&str> {
    let ok = !id.is_empty()
        && id.len() <= 128
        && id != "."
        && id != ".."
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '+' | ' '));
    if ok {
        Ok(id)
    } else {
        Err(Error::invalid(format!(
            "Недопустимый идентификатор версии: {id:?}"
        )))
    }
}

pub fn sha1_hex(bytes: &[u8]) -> String {
    let mut h = Sha1::new();
    h.update(bytes);
    hex::encode(h.finalize())
}

/// Прочитать файл, если он есть (для проверок без сети).
pub fn read_if_exists(path: &std::path::Path) -> Result<Option<Vec<u8>>> {
    match std::fs::read(path) {
        Ok(b) => Ok(Some(b)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).at(path),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest_fixture() -> VersionManifest {
        let text = std::fs::read_to_string(format!(
            "{}/tests/fixtures/manifest-excerpt.json",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap();
        serde_json::from_str(&text).unwrap()
    }

    #[test]
    fn releases_only_by_default() {
        let m = manifest_fixture();
        let ids: Vec<_> = m
            .filtered(false, false)
            .iter()
            .map(|v| v.id.clone())
            .collect();
        assert_eq!(
            ids,
            ["26.3", "1.21.11", "1.20.1", "1.16.5", "1.12.2", "1.6.4"]
        );
    }

    #[test]
    fn manifest_order_is_kept_for_new_scheme() {
        // 26.3 новее 1.21.11, хотя строка «1.21.11» > «26.3» не сравнивается — порядок из данных.
        let m = manifest_fixture();
        let all = m.filtered(true, true);
        assert_eq!(all[0].id, "26.4-snapshot-5");
        assert_eq!(all[1].id, "26.3");
        assert_eq!(all.last().unwrap().id, "b1.7.3");
        assert!(all.iter().all(|v| v.sha1.is_some()));
    }

    #[test]
    fn version_ids_are_checked_before_use_as_file_names() {
        assert!(file_safe("26.3").is_ok());
        assert!(file_safe("1.21.11-pre1").is_ok());
        assert!(file_safe("fabric-loader-0.20.1-26.3").is_ok());
        assert!(file_safe("../x").is_err());
        assert!(file_safe("a/b").is_err());
        assert!(file_safe("").is_err());
    }

    #[tokio::test]
    #[ignore = "сеть: манифест Mojang (~280 КБ)"]
    async fn fetch_real_manifest() {
        let root = std::env::temp_dir().join("quarry-net-manifest");
        let data = DataDir::open(&root).unwrap();
        let (m, fresh) = manifest(&data).await.unwrap();
        assert_eq!(fresh, Freshness::Online);
        assert!(m.find(&m.latest.release).is_some());
        let v = vanilla_version(&data, &m.latest.release).await.unwrap();
        assert!(v.java_version.is_some());
    }
}
