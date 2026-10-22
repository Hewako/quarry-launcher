//! Миры профиля: данные из level.dat, бэкапы и восстановление, проверка совместимости версии.
//! Версия мира сравнивается по числу DataVersion (как это делает сама игра), а не по строке:
//! версия игры берётся из `version.json` внутри клиентского JAR (`world_version`).

use crate::error::{Error, IoContext, Result};
use crate::nbt;
use crate::paths::safe_join;
use crate::snapshot::{self, WorldBackup};
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize)]
pub struct WorldInfo {
    /// Имя папки в saves/.
    pub folder: String,
    /// Название мира из level.dat.
    pub name: String,
    pub version_name: Option<String>,
    pub data_version: Option<i64>,
    pub game_mode: String,
    pub hardcore: bool,
    /// Когда в мире играли последний раз (Unix-время, секунды).
    pub last_played: u64,
    pub size: u64,
    pub backups: Vec<WorldBackup>,
}

/// Сведения о версии игры из клиентского JAR.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct GameJarInfo {
    pub world_version: Option<i64>,
    /// Формат ресурспаков (с 1.21.9 — старшая часть `resource_major`).
    pub resource_pack_format: Option<i64>,
    pub data_pack_format: Option<i64>,
}

pub fn game_jar_info(client_jar: &Path) -> Result<GameJarInfo> {
    let file = std::fs::File::open(client_jar).at(client_jar)?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| Error::parse("клиент игры", e))?;
    let mut e = zip
        .by_name("version.json")
        .map_err(|_| Error::invalid("В клиенте игры нет version.json"))?;
    let mut s = String::new();
    e.read_to_string(&mut s).at(client_jar)?;
    let v: serde_json::Value =
        serde_json::from_str(&s).map_err(|e| Error::parse("version.json клиента", e))?;
    let pv = v.get("pack_version");
    let (res, data) = match pv {
        Some(serde_json::Value::Number(n)) => (n.as_i64(), n.as_i64()),
        Some(o) => (
            o.get("resource_major")
                .or_else(|| o.get("resource"))
                .and_then(|x| x.as_i64()),
            o.get("data_major")
                .or_else(|| o.get("data"))
                .and_then(|x| x.as_i64()),
        ),
        None => (None, None),
    };
    Ok(GameJarInfo {
        world_version: v.get("world_version").and_then(|x| x.as_i64()),
        resource_pack_format: res,
        data_pack_format: data,
    })
}

fn mode_name(v: i64) -> &'static str {
    match v {
        0 => "выживание",
        1 => "творчество",
        2 => "приключение",
        3 => "наблюдатель",
        _ => "неизвестно",
    }
}

/// Бэкапы мира в папке бэкапов профиля (`<мир>-<время>.zip`), новые первыми.
pub fn backups_of(backups_dir: &Path, world: &str) -> Vec<WorldBackup> {
    let prefix = format!("{world}-");
    let mut out: Vec<WorldBackup> = std::fs::read_dir(backups_dir)
        .map(|rd| {
            rd.flatten()
                .filter_map(|e| {
                    let name = e.file_name().to_string_lossy().into_owned();
                    let ts = name.strip_prefix(&prefix)?.strip_suffix(".zip")?;
                    let created_at: u64 = ts.parse().ok()?;
                    Some(WorldBackup {
                        world: world.to_string(),
                        size: e.metadata().map(|m| m.len()).unwrap_or(0),
                        file: e.path(),
                        created_at,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    out.sort_by_key(|b| std::cmp::Reverse(b.created_at));
    out
}

pub fn read_world(game_dir: &Path, backups_dir: &Path, folder: &str) -> Result<WorldInfo> {
    let dir = game_dir.join("saves").join(folder);
    let level = dir.join("level.dat");
    let root = nbt::read(&std::fs::read(&level).at(&level)?)?;
    let data = root
        .get("Data")
        .ok_or_else(|| Error::parse("level.dat", "нет раздела Data"))?;
    Ok(WorldInfo {
        folder: folder.to_string(),
        name: data
            .get("LevelName")
            .and_then(|t| t.as_str())
            .unwrap_or(folder)
            .to_string(),
        version_name: data
            .path("Version.Name")
            .and_then(|t| t.as_str())
            .map(str::to_string),
        data_version: data
            .get("DataVersion")
            .and_then(|t| t.as_i64())
            .or_else(|| data.path("Version.Id").and_then(|t| t.as_i64())),
        game_mode: mode_name(data.get("GameType").and_then(|t| t.as_i64()).unwrap_or(-1))
            .to_string(),
        hardcore: data.get("hardcore").and_then(|t| t.as_i64()).unwrap_or(0) != 0,
        last_played: data
            .get("LastPlayed")
            .and_then(|t| t.as_i64())
            .map(|ms| (ms / 1000).max(0) as u64)
            .unwrap_or(0),
        size: crate::paths::dir_size(&dir),
        backups: backups_of(backups_dir, folder),
    })
}

/// Все миры профиля, недавние первыми. Мир с повреждённым level.dat показывается с пометкой.
pub fn list(game_dir: &Path, backups_dir: &Path) -> Vec<WorldInfo> {
    let mut out: Vec<WorldInfo> = std::fs::read_dir(game_dir.join("saves"))
        .map(|rd| {
            rd.flatten()
                .filter(|e| e.path().join("level.dat").is_file())
                .map(|e| {
                    let folder = e.file_name().to_string_lossy().into_owned();
                    read_world(game_dir, backups_dir, &folder).unwrap_or_else(|_| WorldInfo {
                        name: folder.clone(),
                        folder,
                        version_name: None,
                        data_version: None,
                        game_mode: "level.dat не прочитан".into(),
                        hardcore: false,
                        last_played: 0,
                        size: crate::paths::dir_size(&e.path()),
                        backups: Vec::new(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    out.sort_by_key(|w| std::cmp::Reverse(w.last_played));
    out
}

/// Восстановить мир из бэкапа. Текущий мир не удаляется: сначала он сам бэкапится,
/// затем откладывается в `<папка игры>/.quarry-replaced/` — на тот же диск, что и мир
/// (папку игры могли перенести на другой диск, а rename между дисками не работает).
pub fn restore(game_dir: &Path, backups_dir: &Path, backup_file: &Path) -> Result<String> {
    let backup_file = std::fs::canonicalize(backup_file).at(backup_file)?;
    let base = std::fs::canonicalize(backups_dir).at(backups_dir)?;
    if backup_file.parent() != Some(base.as_path()) {
        return Err(Error::invalid("Бэкап не из папки бэкапов этого профиля"));
    }
    let file = std::fs::File::open(&backup_file).at(&backup_file)?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| Error::parse("бэкап мира", e))?;
    // Все записи начинаются с «<мир>/».
    let world = {
        let first = zip.by_index(0).map_err(|e| Error::parse("бэкап мира", e))?;
        first
            .name()
            .unwrap_or_default()
            .split('/')
            .next()
            .unwrap_or_default()
            .to_string()
    };
    if world.is_empty() || world.starts_with('.') || world.contains(['\\', ':']) {
        return Err(Error::invalid("Бэкап повреждён"));
    }
    let saves = game_dir.join("saves");
    std::fs::create_dir_all(&saves).at(&saves)?;
    let staging = crate::paths::unique_sibling(&saves.join(&world), "restore");
    // Оборвавшаяся распаковка не должна оставить в saves/ полмира, который игра покажет.
    if let Err(e) = unpack_world(&mut zip, &world, &staging, &backup_file) {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(e);
    }
    let current = saves.join(&world);
    if current.exists() {
        snapshot::backup_world(game_dir, backups_dir, &world)?;
        let replaced = game_dir.join(".quarry-replaced");
        std::fs::create_dir_all(&replaced).at(&replaced)?;
        let aside = replaced.join(format!("{world}-{}", crate::instance::now()));
        std::fs::rename(&current, &aside).at(&current)?;
    }
    std::fs::rename(&staging, &current).at(&staging)?;
    Ok(world)
}

fn unpack_world(
    zip: &mut zip::ZipArchive<std::fs::File>,
    world: &str,
    staging: &Path,
    backup_file: &Path,
) -> Result<()> {
    for i in 0..zip.len() {
        let mut e = zip.by_index(i).map_err(|e| Error::parse("бэкап мира", e))?;
        let name = e.name().unwrap_or_default().to_string();
        let Some(rel) = name.strip_prefix(&format!("{world}/")) else {
            return Err(Error::invalid("В бэкапе файлы разных миров"));
        };
        if e.is_dir() || rel.is_empty() {
            continue;
        }
        let target = safe_join(staging, rel)
            .ok_or_else(|| Error::invalid(format!("Недопустимый путь в бэкапе: {name}")))?;
        if let Some(p) = target.parent() {
            std::fs::create_dir_all(p).at(p)?;
        }
        let mut buf = Vec::new();
        e.read_to_end(&mut buf).at(backup_file)?;
        std::fs::write(&target, buf).at(&target)?;
    }
    Ok(())
}

/// Миры, сохранённые в более новой версии игры, чем запускаемая.
pub fn newer_than(worlds: &[WorldInfo], game_world_version: i64) -> Vec<&WorldInfo> {
    worlds
        .iter()
        .filter(|w| w.data_version.is_some_and(|v| v > game_world_version))
        .collect()
}

/// Пора ли делать бэкап по расписанию: раз в `every_days` дней, если в мир играли после
/// последнего бэкапа. `every_days == 0` — расписание выключено.
pub fn backup_due(w: &WorldInfo, every_days: u32, now: u64) -> bool {
    if every_days == 0 {
        return false;
    }
    match w.backups.iter().map(|b| b.created_at).max() {
        None => w.last_played > 0,
        Some(last) => {
            now.saturating_sub(last) >= u64::from(every_days) * 86_400 && w.last_played > last
        }
    }
}

/// Бэкап последнего мира, в который играли (если есть). Для автобэкапов.
pub fn backup_latest(game_dir: &Path, backups_dir: &Path) -> Option<WorldBackup> {
    let worlds = list(game_dir, backups_dir);
    let latest = worlds.first()?;
    snapshot::backup_world(game_dir, backups_dir, &latest.folder).ok()
}

pub fn backups_dir(data: &crate::paths::DataDir, instance_id: &str) -> PathBuf {
    data.backups().join(instance_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nbt::Tag;
    use std::collections::BTreeMap;
    use std::io::Write;

    pub fn make_world(game: &Path, folder: &str, name: &str, data_version: i32, last_played: i64) {
        let dir = game.join("saves").join(folder);
        std::fs::create_dir_all(dir.join("region")).unwrap();
        std::fs::write(dir.join("region/r.0.0.mca"), vec![1u8; 1000]).unwrap();
        let mut data = BTreeMap::new();
        data.insert("LevelName".to_string(), Tag::String(name.into()));
        data.insert("DataVersion".to_string(), Tag::Int(data_version));
        data.insert("GameType".to_string(), Tag::Int(1));
        data.insert("LastPlayed".to_string(), Tag::Long(last_played));
        let mut v = BTreeMap::new();
        v.insert("Name".to_string(), Tag::String("1.21.11".into()));
        data.insert("Version".to_string(), Tag::Compound(v));
        let mut root = BTreeMap::new();
        root.insert("Data".to_string(), Tag::Compound(data));
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        gz.write_all(&nbt::write(&Tag::Compound(root))).unwrap();
        std::fs::write(dir.join("level.dat"), gz.finish().unwrap()).unwrap();
    }

    #[test]
    fn list_restore_and_version_check() {
        let root = std::env::temp_dir().join(format!("quarry-worlds-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (game, backups) = (root.join("game"), root.join("backups"));
        make_world(&game, "W1", "База у реки", 4671, 2_000_000_000_000);
        make_world(&game, "W2", "Старый", 2586, 1_000_000_000_000);
        let ws = list(&game, &backups);
        assert_eq!(ws[0].name, "База у реки", "недавний первым");
        assert_eq!(ws[0].game_mode, "творчество");
        assert!(ws[0].size >= 1000);
        assert_eq!(newer_than(&ws, 2586).len(), 1);

        let b = snapshot::backup_world(&game, &backups, "W1").unwrap();
        std::fs::write(game.join("saves/W1/region/r.0.0.mca"), b"broken").unwrap();
        assert_eq!(restore(&game, &backups, &b.file).unwrap(), "W1");
        assert_eq!(
            std::fs::metadata(game.join("saves/W1/region/r.0.0.mca"))
                .unwrap()
                .len(),
            1000
        );
        // Испорченная версия не пропала: забэкаплена и отложена на том же диске, что и мир.
        assert!(game.join(".quarry-replaced").read_dir().unwrap().count() == 1);
        assert!(restore(&game, &backups, &root.join("elsewhere.zip")).is_err());
        assert!(backup_latest(&game, &backups).is_some());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn broken_backup_leaves_no_half_world() {
        let root = std::env::temp_dir().join(format!("quarry-worlds-bad-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (game, backups) = (root.join("game"), root.join("backups"));
        make_world(&game, "W1", "Мир", 4671, 1);
        std::fs::create_dir_all(&backups).unwrap();
        let bad = backups.join("W1-bad.zip");
        let mut z = zip::ZipWriter::new(std::fs::File::create(&bad).unwrap());
        let opts = zip::write::SimpleFileOptions::default();
        z.start_file("W1/level.dat", opts).unwrap();
        z.write_all(b"x").unwrap();
        z.start_file("W1/../../evil", opts).unwrap();
        z.write_all(b"x").unwrap();
        z.finish().unwrap();
        assert!(restore(&game, &backups, &bad).is_err());
        let leftovers: Vec<_> = std::fs::read_dir(game.join("saves"))
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n != "W1")
            .collect();
        assert!(leftovers.is_empty(), "в saves остались {leftovers:?}");
        assert!(!root.join("evil").exists());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn jar_info_both_formats() {
        let root = std::env::temp_dir().join(format!("quarry-jarinfo-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        for (name, json, res) in [
            (
                "new.jar",
                r#"{"id":"26.3","world_version":5023,"pack_version":{"resource_major":97,"resource_minor":1,"data_major":121,"data_minor":0}}"#,
                97,
            ),
            (
                "old.jar",
                r#"{"id":"1.16.5","world_version":2586,"pack_version":6}"#,
                6,
            ),
        ] {
            let p = root.join(name);
            crate::mods::meta::tests::make_jar(&p, &[("version.json", json.as_bytes())]);
            let i = game_jar_info(&p).unwrap();
            assert_eq!(i.resource_pack_format, Some(res));
            assert!(i.world_version.is_some());
        }
        std::fs::remove_dir_all(&root).unwrap();
    }

    fn world(last_played: u64, backups: &[u64]) -> WorldInfo {
        WorldInfo {
            folder: "W".into(),
            name: "W".into(),
            version_name: None,
            data_version: None,
            game_mode: String::new(),
            hardcore: false,
            last_played,
            size: 0,
            backups: backups
                .iter()
                .map(|&t| WorldBackup {
                    world: "W".into(),
                    file: PathBuf::new(),
                    size: 0,
                    created_at: t,
                })
                .collect(),
        }
    }

    #[test]
    fn scheduled_backup_is_due_only_for_played_worlds() {
        let day = 86_400;
        let now = 100 * day;
        // Расписание выключено.
        assert!(!backup_due(&world(now, &[]), 0, now));
        // Бэкапов нет, в мир играли — пора.
        assert!(backup_due(&world(now - 10, &[]), 3, now));
        // Последний бэкап свежий.
        assert!(!backup_due(&world(now - 10, &[now - day]), 3, now));
        // Бэкап старый, и после него в мир играли.
        assert!(backup_due(&world(now - 10, &[now - 4 * day]), 3, now));
        // Бэкап старый, но в мир с тех пор не заходили — копировать нечего.
        assert!(!backup_due(&world(now - 5 * day, &[now - 4 * day]), 3, now));
    }
}
