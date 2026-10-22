//! Моды профиля: инвентаризация (метаданные из JAR, хэши, источник из pack.lock),
//! включение/выключение, Mod Doctor и разбор крашей.

pub mod bisect;
pub mod crash;
pub mod doctor;
pub mod groups;
pub mod lock;
pub mod manage;
pub mod meta;
pub mod range;
pub mod rules;

use crate::download::hash_file;
use crate::error::{Error, IoContext, Result};
use lock::{LockEntry, PackLock};
use meta::ModMeta;
use serde::Serialize;
use sha1::Sha1;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize)]
pub struct ModFile {
    /// Имя файла без `.disabled`.
    pub file_name: String,
    /// Путь относительно папки игры (`mods/x.jar` или `mods/x.jar.disabled`).
    pub rel_path: String,
    pub enabled: bool,
    pub size: u64,
    pub sha1: String,
    /// Моды внутри файла (обычно один). Пусто — метаданных нет.
    pub mods: Vec<ModMeta>,
    /// Почему метаданные не прочитаны (повреждённый архив и т. п.).
    pub read_error: Option<String>,
    /// Откуда файл (из pack.lock); `None` — положен вручную и не записан.
    pub source: Option<LockEntry>,
    /// Мод добавляет блоки, измерения или генерацию мира.
    pub world_content: bool,
}

impl ModFile {
    pub fn primary(&self) -> Option<&ModMeta> {
        self.mods.first()
    }
    pub fn display_name(&self) -> String {
        self.primary()
            .map(|m| m.name.clone())
            .or_else(|| self.source.as_ref().and_then(|s| s.title.clone()))
            .unwrap_or_else(|| self.file_name.clone())
    }
}

/// Прочитать все моды из `game/mods`: `*.jar` включены, `*.jar.disabled` выключены.
pub fn inventory(game_dir: &Path, instance_dir: &Path) -> Result<Vec<ModFile>> {
    let mods_dir = game_dir.join("mods");
    let lock = PackLock::load(instance_dir)?;
    let mut out = Vec::new();
    let entries = match std::fs::read_dir(&mods_dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
        Err(e) => return Err(Error::io(&mods_dir, e)),
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let (enabled, file_name) = if let Some(base) = name.strip_suffix(".disabled") {
            (false, base.to_string())
        } else {
            (true, name.clone())
        };
        if !file_name.ends_with(".jar") {
            continue;
        }
        let rel_path = format!("mods/{name}");
        let sha1 = hash_file::<Sha1>(&path)?;
        let (mods, read_error) = match meta::read_jar(&path) {
            Ok(m) => (m, None),
            Err(e) => (Vec::new(), Some(e.to_string())),
        };
        out.push(ModFile {
            source: lock.find(&rel_path, &sha1).cloned(),
            world_content: meta::adds_world_content(&path),
            file_name,
            rel_path,
            enabled,
            size: entry.metadata().map(|m| m.len()).unwrap_or(0),
            sha1,
            mods,
            read_error,
        });
    }
    out.sort_by(|a, b| {
        a.display_name()
            .to_lowercase()
            .cmp(&b.display_name().to_lowercase())
    });
    Ok(out)
}

/// Имя JAR-файла мода (из каталога, архива или интерфейса) — один файл в `mods/`, не путь.
pub(crate) fn jar_file_name(name: &str) -> Result<&str> {
    if crate::paths::is_plain_file_name(name) && name.ends_with(".jar") {
        Ok(name)
    } else {
        Err(Error::invalid(format!(
            "Недопустимое имя файла мода: {name}"
        )))
    }
}

/// Путь к файлу мода в `mods/` по имени из интерфейса.
fn mod_path(game_dir: &Path, file_name: &str) -> Result<PathBuf> {
    Ok(game_dir.join("mods").join(jar_file_name(file_name)?))
}

/// Включить или выключить мод: `x.jar` ↔ `x.jar.disabled`. Файл не удаляется.
pub fn set_enabled(game_dir: &Path, file_name: &str, enabled: bool) -> Result<()> {
    let on = mod_path(game_dir, file_name)?;
    let off = on.with_file_name(format!("{file_name}.disabled"));
    let (from, to) = if enabled { (&off, &on) } else { (&on, &off) };
    if !from.exists() {
        return if to.exists() {
            Ok(())
        } else {
            Err(Error::NotFound(format!("Мод {file_name} не найден")))
        };
    }
    if to.exists() {
        return Err(Error::invalid(format!(
            "Есть и включённая, и выключенная копия {file_name} — оставьте одну"
        )));
    }
    std::fs::rename(from, to).at(from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use meta::tests::make_jar;

    #[test]
    fn inventory_and_toggle() {
        let root = std::env::temp_dir().join(format!("quarry-inv-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let game = root.join("game");
        std::fs::create_dir_all(game.join("mods")).unwrap();
        make_jar(
            &game.join("mods/sodium.jar"),
            &[(
                "fabric.mod.json",
                br#"{"id":"sodium","name":"Sodium","version":"0.8"}"#,
            )],
        );
        make_jar(
            &game.join("mods/old.jar.disabled"),
            &[("fabric.mod.json", br#"{"id":"old","version":"1"}"#)],
        );
        std::fs::write(game.join("mods/readme.txt"), b"x").unwrap();
        std::fs::write(game.join("mods/broken.jar"), b"not zip").unwrap();
        let inv = inventory(&game, &root).unwrap();
        assert_eq!(inv.len(), 3, "txt пропущен");
        let broken = inv.iter().find(|m| m.file_name == "broken.jar").unwrap();
        assert!(broken.read_error.is_some() && broken.mods.is_empty());
        assert!(
            !inv.iter()
                .find(|m| m.file_name == "old.jar")
                .unwrap()
                .enabled
        );

        set_enabled(&game, "sodium.jar", false).unwrap();
        assert!(game.join("mods/sodium.jar.disabled").exists());
        set_enabled(&game, "sodium.jar", true).unwrap();
        assert!(game.join("mods/sodium.jar").exists());
        assert!(set_enabled(&game, "../evil.jar", false).is_err());
        std::fs::remove_dir_all(&root).unwrap();
    }
}
