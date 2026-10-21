//! `pack.lock` профиля: что установлено, откуда, с каким хэшем и в каком слое.
//! Нужен, чтобы знать источник каждого файла и проверять обновления без догадок.

use crate::error::{Error, Result};
use crate::paths::write_atomic;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Modrinth,
    CurseForge,
    /// Перенесён при импорте из другого лаунчера или сборки.
    Import,
    /// Положен в папку вручную.
    Manual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Layer {
    /// Часть сборки (модпака).
    Base,
    /// Добавлен игроком.
    User,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LockEntry {
    /// Путь относительно папки игры профиля, через `/`.
    pub path: String,
    pub sha1: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha512: Option<String>,
    pub source: Source,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version_number: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub layer: Layer,
    #[serde(default)]
    pub installed_at: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PackLock {
    #[serde(default)]
    pub files: Vec<LockEntry>,
}

impl PackLock {
    pub fn path(instance_dir: &Path) -> PathBuf {
        instance_dir.join("pack.lock")
    }

    pub fn load(instance_dir: &Path) -> Result<Self> {
        let path = Self::path(instance_dir);
        match std::fs::read_to_string(&path) {
            Ok(t) => toml::from_str(&t).map_err(|e| Error::parse("pack.lock", e)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(Error::io(&path, e)),
        }
    }

    pub fn save(&self, instance_dir: &Path) -> Result<()> {
        let text = toml::to_string_pretty(self).map_err(|e| Error::parse("pack.lock", e))?;
        write_atomic(&Self::path(instance_dir), text.as_bytes())
    }

    /// Запись для файла: по пути, а если файл переименовали (например, выключили) — по хэшу.
    pub fn find(&self, rel_path: &str, sha1: &str) -> Option<&LockEntry> {
        let base = rel_path.trim_end_matches(".disabled");
        self.files
            .iter()
            .find(|e| e.path == base || e.path == rel_path)
            .or_else(|| {
                self.files
                    .iter()
                    .find(|e| e.sha1.eq_ignore_ascii_case(sha1))
            })
    }

    pub fn upsert(&mut self, entry: LockEntry) {
        self.files.retain(|e| e.path != entry.path);
        self.files.push(entry);
        self.files.sort_by(|a, b| a.path.cmp(&b.path));
    }

    pub fn remove(&mut self, rel_path: &str) {
        let base = rel_path.trim_end_matches(".disabled");
        self.files.retain(|e| e.path != base && e.path != rel_path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_lookup() {
        let d = std::env::temp_dir().join(format!("quarry-lock-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let mut l = PackLock::default();
        l.upsert(LockEntry {
            path: "mods/sodium.jar".into(),
            sha1: "ABC".into(),
            sha512: None,
            source: Source::Modrinth,
            project_id: Some("AANobbMI".into()),
            version_id: None,
            version_number: Some("0.8.15".into()),
            title: Some("Sodium".into()),
            layer: Layer::User,
            installed_at: 1,
        });
        l.save(&d).unwrap();
        let l = PackLock::load(&d).unwrap();
        assert!(
            l.find("mods/sodium.jar.disabled", "x").is_some(),
            "выключенный мод узнаётся"
        );
        assert!(
            l.find("mods/renamed.jar", "abc").is_some(),
            "переименованный — по хэшу"
        );
        assert!(PackLock::load(&d.join("missing")).unwrap().files.is_empty());
        std::fs::remove_dir_all(&d).unwrap();
    }
}
