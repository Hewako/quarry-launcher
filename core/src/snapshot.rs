//! Снимки профиля и откат. В снимок входят `mods/`, `config/`, `options.txt` и `pack.lock`
//! (миры защищают отдельные бэкапы — снимок большого пака не должен весить гигабайты).
//! JAR модов не меняются после скачивания, поэтому вместо копий используются жёсткие ссылки
//! (если файловая система не умеет — обычные копии).

use crate::error::{Error, IoContext, Result};
use crate::instance::now;
use crate::paths::{safe_join, write_atomic};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Сколько автоматических снимков хранить (закреплённые не считаются).
pub const KEEP_AUTO: usize = 10;
/// Сколько бэкапов каждого мира хранить.
pub const KEEP_WORLD_BACKUPS: usize = 5;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotInfo {
    pub id: String,
    /// Причина простыми словами: «Перед установкой Sodium».
    pub reason: String,
    pub created_at: u64,
    #[serde(default)]
    pub pinned: bool,
    /// Создан лаунчером сам (перед рискованным действием) или вручную.
    #[serde(default)]
    pub auto: bool,
    #[serde(default)]
    pub mods: usize,
    #[serde(default)]
    pub configs: usize,
}

fn snapshots_dir(instance_dir: &Path) -> PathBuf {
    instance_dir.join("snapshots")
}

fn slug(reason: &str) -> String {
    let s: String = reason
        .chars()
        .flat_map(|c| c.to_lowercase())
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect();
    let s = s
        .split('-')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    s.chars().take(32).collect()
}

/// Скопировать дерево: JAR — жёсткой ссылкой, остальное — копией.
fn copy_tree(from: &Path, to: &Path, count: &mut usize) -> Result<()> {
    let Ok(rd) = std::fs::read_dir(from) else {
        return Ok(());
    };
    std::fs::create_dir_all(to).at(to)?;
    for e in rd.flatten() {
        let src = e.path();
        let dst = to.join(e.file_name());
        let ft = e.file_type().at(&src)?;
        if ft.is_dir() {
            copy_tree(&src, &dst, count)?;
        } else if ft.is_file() {
            let name = e.file_name().to_string_lossy().into_owned();
            let is_jar = name.ends_with(".jar") || name.ends_with(".jar.disabled");
            if !(is_jar && std::fs::hard_link(&src, &dst).is_ok()) {
                std::fs::copy(&src, &dst).at(&dst)?;
            }
            *count += 1;
        }
        // Символические ссылки в снимок не попадают (могут вести наружу).
    }
    Ok(())
}

/// Создать снимок.
pub fn create(
    instance_dir: &Path,
    game_dir: &Path,
    reason: &str,
    auto: bool,
) -> Result<SnapshotInfo> {
    let created_at = now();
    let mut id = format!("{created_at}-{}", slug(reason));
    let base = snapshots_dir(instance_dir);
    let mut n = 2;
    while base.join(&id).exists() {
        id = format!("{created_at}-{}-{n}", slug(reason));
        n += 1;
    }
    let staging = crate::paths::unique_sibling(&base.join(&id), "staging");
    let files = staging.join("files");
    std::fs::create_dir_all(&files).at(&files)?;
    let mut mods = 0;
    let mut configs = 0;
    copy_tree(&game_dir.join("mods"), &files.join("mods"), &mut mods)?;
    copy_tree(
        &game_dir.join("config"),
        &files.join("config"),
        &mut configs,
    )?;
    for (src, dst) in [
        (game_dir.join("options.txt"), files.join("options.txt")),
        (instance_dir.join("pack.lock"), staging.join("pack.lock")),
    ] {
        if src.is_file() {
            std::fs::copy(&src, &dst).at(&dst)?;
        }
    }
    let info = SnapshotInfo {
        id: id.clone(),
        reason: reason.to_string(),
        created_at,
        pinned: false,
        auto,
        mods,
        configs,
    };
    write_atomic(
        &staging.join("snapshot.json"),
        &serde_json::to_vec_pretty(&info).unwrap_or_default(),
    )?;
    std::fs::rename(&staging, base.join(&id)).at(&staging)?;
    rotate(instance_dir)?;
    Ok(info)
}

pub fn list(instance_dir: &Path) -> Result<Vec<SnapshotInfo>> {
    let base = snapshots_dir(instance_dir);
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(&base) else {
        return Ok(out);
    };
    for e in rd.flatten() {
        let p = e.path().join("snapshot.json");
        if let Ok(bytes) = std::fs::read(&p)
            && let Ok(info) = serde_json::from_slice::<SnapshotInfo>(&bytes)
        {
            out.push(info);
        }
    }
    out.sort_by(|a, b| b.created_at.cmp(&a.created_at).then(b.id.cmp(&a.id)));
    Ok(out)
}

fn snapshot_dir(instance_dir: &Path, id: &str) -> Result<PathBuf> {
    let dir = safe_join(&snapshots_dir(instance_dir), id)
        .filter(|_| !id.contains('/') && !id.contains('\\'))
        .ok_or_else(|| Error::invalid("Недопустимый снимок"))?;
    if !dir.join("snapshot.json").is_file() {
        return Err(Error::NotFound("Снимок не найден".into()));
    }
    Ok(dir)
}

pub fn set_pinned(instance_dir: &Path, id: &str, pinned: bool) -> Result<()> {
    let dir = snapshot_dir(instance_dir, id)?;
    let path = dir.join("snapshot.json");
    let mut info: SnapshotInfo = serde_json::from_slice(&std::fs::read(&path).at(&path)?)
        .map_err(|e| Error::parse("snapshot.json", e))?;
    info.pinned = pinned;
    write_atomic(&path, &serde_json::to_vec_pretty(&info).unwrap_or_default())
}

/// Удалить самые старые автоматические снимки сверх KEEP_AUTO. Закреплённые и ручные не трогаются.
pub fn rotate(instance_dir: &Path) -> Result<()> {
    let autos: Vec<SnapshotInfo> = list(instance_dir)?
        .into_iter()
        .filter(|s| s.auto && !s.pinned)
        .collect();
    for old in autos.iter().skip(KEEP_AUTO) {
        let dir = snapshots_dir(instance_dir).join(&old.id);
        std::fs::remove_dir_all(&dir).at(&dir)?;
    }
    Ok(())
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Diff {
    /// Файлы, которые есть сейчас, но не было в снимке.
    pub added: Vec<String>,
    /// Были в снимке, сейчас нет.
    pub removed: Vec<String>,
    pub changed: Vec<String>,
    /// Изменения options.txt: ключ, было, стало.
    pub options: Vec<(String, String, String)>,
}

fn file_map(root: &Path, prefix: &str, out: &mut BTreeMap<String, (u64, String)>) {
    let Ok(rd) = std::fs::read_dir(root) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        let rel = format!("{prefix}/{}", e.file_name().to_string_lossy());
        if p.is_dir() {
            file_map(&p, &rel, out);
        } else if let Ok(m) = p.metadata() {
            let hash = if m.len() < 1_048_576 * 8 {
                crate::download::hash_file::<sha1::Sha1>(&p).unwrap_or_default()
            } else {
                String::new()
            };
            out.insert(rel, (m.len(), hash));
        }
    }
}

fn options_map(path: &Path) -> BTreeMap<String, String> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| {
            l.split_once(':')
                .map(|(k, v)| (k.to_string(), v.to_string()))
        })
        .collect()
}

/// Что изменилось с момента снимка.
pub fn diff(instance_dir: &Path, game_dir: &Path, id: &str) -> Result<Diff> {
    let snap = snapshot_dir(instance_dir, id)?.join("files");
    let mut then = BTreeMap::new();
    let mut now_ = BTreeMap::new();
    for sub in ["mods", "config"] {
        file_map(&snap.join(sub), sub, &mut then);
        file_map(&game_dir.join(sub), sub, &mut now_);
    }
    let mut d = Diff::default();
    for (k, v) in &now_ {
        match then.get(k) {
            None => d.added.push(k.clone()),
            Some(old) if old != v => d.changed.push(k.clone()),
            _ => {}
        }
    }
    d.removed = then
        .keys()
        .filter(|k| !now_.contains_key(*k))
        .cloned()
        .collect();
    let (o_then, o_now) = (
        options_map(&snap.join("options.txt")),
        options_map(&game_dir.join("options.txt")),
    );
    for (k, v) in &o_now {
        let old = o_then.get(k).cloned().unwrap_or_default();
        if &old != v {
            d.options.push((k.clone(), old, v.clone()));
        }
    }
    Ok(d)
}

/// Откатить профиль к снимку. Сначала создаётся снимок «Перед откатом» — откат тоже обратим.
pub fn restore(instance_dir: &Path, game_dir: &Path, id: &str) -> Result<SnapshotInfo> {
    let dir = snapshot_dir(instance_dir, id)?;
    let path = dir.join("snapshot.json");
    let info: SnapshotInfo = serde_json::from_slice(&std::fs::read(&path).at(&path)?)
        .map_err(|e| Error::parse("snapshot.json", e))?;
    let before = create(
        instance_dir,
        game_dir,
        &format!("Перед откатом к «{}»", info.reason),
        true,
    )?;
    // Текущие папки уходят во временную, и только после успешного копирования удаляются.
    let aside = crate::paths::unique_sibling(&instance_dir.join("restore"), "old");
    std::fs::create_dir_all(&aside).at(&aside)?;
    for name in ["mods", "config", "options.txt"] {
        let cur = game_dir.join(name);
        if cur.exists() {
            std::fs::rename(&cur, aside.join(name)).at(&cur)?;
        }
    }
    let files = dir.join("files");
    let result = (|| -> Result<()> {
        let mut n = 0;
        copy_tree(&files.join("mods"), &game_dir.join("mods"), &mut n)?;
        copy_tree(&files.join("config"), &game_dir.join("config"), &mut n)?;
        if files.join("options.txt").is_file() {
            std::fs::copy(files.join("options.txt"), game_dir.join("options.txt")).at(game_dir)?;
        }
        match std::fs::read(dir.join("pack.lock")) {
            Ok(b) => write_atomic(&instance_dir.join("pack.lock"), &b),
            Err(_) => {
                let _ = std::fs::remove_file(instance_dir.join("pack.lock"));
                Ok(())
            }
        }
    })();
    match result {
        Ok(()) => {
            let _ = std::fs::remove_dir_all(&aside);
            Ok(before)
        }
        Err(e) => {
            // Вернуть всё как было.
            for name in ["mods", "config", "options.txt"] {
                let cur = game_dir.join(name);
                let _ = if cur.is_dir() {
                    std::fs::remove_dir_all(&cur)
                } else {
                    std::fs::remove_file(&cur)
                };
                let _ = std::fs::rename(aside.join(name), &cur);
            }
            Err(e)
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct WorldBackup {
    pub world: String,
    pub file: PathBuf,
    pub size: u64,
    pub created_at: u64,
}

/// Бэкап мира в zip: `backups/<профиль>/<мир>-<время>.zip`. Хранится KEEP_WORLD_BACKUPS последних.
pub fn backup_world(game_dir: &Path, backups_dir: &Path, world: &str) -> Result<WorldBackup> {
    use std::io::Write;
    if world.contains(['/', '\\']) || world.contains("..") || world.is_empty() {
        return Err(Error::invalid("Недопустимое имя мира"));
    }
    let src = game_dir.join("saves").join(world);
    if !src.join("level.dat").is_file() {
        return Err(Error::NotFound(format!("Мир «{world}» не найден")));
    }
    std::fs::create_dir_all(backups_dir).at(backups_dir)?;
    let created_at = now();
    let file = backups_dir.join(format!("{world}-{created_at}.zip"));
    let tmp = crate::paths::unique_sibling(&file, "part");
    {
        let f = std::fs::File::create(&tmp).at(&tmp)?;
        let mut zip = zip::ZipWriter::new(f);
        let opts: zip::write::SimpleFileOptions = Default::default();
        let mut stack = vec![src.clone()];
        while let Some(dir) = stack.pop() {
            for e in std::fs::read_dir(&dir).at(&dir)?.flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else if p.is_file() {
                    // session.lock занят запущенной игрой и не нужен для восстановления.
                    if p.file_name().is_some_and(|n| n == "session.lock") {
                        continue;
                    }
                    let rel = p
                        .strip_prefix(&src)
                        .unwrap_or(&p)
                        .to_string_lossy()
                        .replace('\\', "/");
                    zip.start_file(format!("{world}/{rel}"), opts)
                        .map_err(|e| Error::invalid(e.to_string()))?;
                    zip.write_all(&std::fs::read(&p).at(&p)?).at(&tmp)?;
                }
            }
        }
        zip.finish().map_err(|e| Error::invalid(e.to_string()))?;
    }
    std::fs::rename(&tmp, &file).at(&file)?;
    // Ротация бэкапов этого мира.
    let mut mine: Vec<PathBuf> = std::fs::read_dir(backups_dir)
        .at(backups_dir)?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .and_then(|n| n.strip_prefix(&format!("{world}-")))
                .is_some_and(|rest| {
                    rest.trim_end_matches(".zip")
                        .chars()
                        .all(|c| c.is_ascii_digit())
                })
        })
        .collect();
    mine.sort();
    while mine.len() > KEEP_WORLD_BACKUPS {
        let old = mine.remove(0);
        let _ = std::fs::remove_file(old);
    }
    Ok(WorldBackup {
        world: world.to_string(),
        size: std::fs::metadata(&file).map(|m| m.len()).unwrap_or(0),
        file,
        created_at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup(name: &str) -> (PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!("quarry-snap-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let game = root.join("game");
        std::fs::create_dir_all(game.join("mods")).unwrap();
        std::fs::create_dir_all(game.join("config/sodium")).unwrap();
        std::fs::write(game.join("mods/sodium.jar"), b"sodium-0.7").unwrap();
        std::fs::write(game.join("config/sodium/options.json"), b"{}").unwrap();
        std::fs::write(game.join("options.txt"), "renderDistance:12\nfov:0.0\n").unwrap();
        (root.clone(), game)
    }

    #[test]
    fn snapshot_diff_and_restore() {
        let (inst, game) = setup("restore");
        let s = create(&inst, &game, "Перед установкой Distant Horizons", true).unwrap();
        assert_eq!((s.mods, s.configs), (1, 1));

        std::fs::write(game.join("mods/dh.jar"), b"dh").unwrap();
        std::fs::remove_file(game.join("mods/sodium.jar")).unwrap();
        std::fs::write(game.join("mods/sodium-new.jar"), b"sodium-0.8").unwrap();
        std::fs::write(game.join("options.txt"), "renderDistance:16\nfov:0.0\n").unwrap();
        let d = diff(&inst, &game, &s.id).unwrap();
        assert!(d.added.contains(&"mods/dh.jar".to_string()));
        assert_eq!(d.removed, vec!["mods/sodium.jar".to_string()]);
        assert_eq!(
            d.options,
            vec![("renderDistance".into(), "12".into(), "16".into())]
        );

        let before = restore(&inst, &game, &s.id).unwrap();
        assert!(game.join("mods/sodium.jar").exists());
        assert!(!game.join("mods/dh.jar").exists());
        assert_eq!(
            std::fs::read_to_string(game.join("options.txt")).unwrap(),
            "renderDistance:12\nfov:0.0\n"
        );
        // Откат обратим: состояние до отката сохранено.
        assert!(before.reason.starts_with("Перед откатом"));
        restore(&inst, &game, &before.id).unwrap();
        assert!(game.join("mods/dh.jar").exists());
        std::fs::remove_dir_all(&inst).unwrap();
    }

    #[test]
    fn rotation_keeps_pinned_and_manual() {
        let (inst, game) = setup("rotate");
        let pinned = create(&inst, &game, "Чистая сборка", true).unwrap();
        set_pinned(&inst, &pinned.id, true).unwrap();
        let manual = create(&inst, &game, "Вручную", false).unwrap();
        for i in 0..(KEEP_AUTO + 3) {
            create(&inst, &game, &format!("авто {i}"), true).unwrap();
        }
        let all = list(&inst).unwrap();
        assert_eq!(
            all.iter().filter(|s| s.auto && !s.pinned).count(),
            KEEP_AUTO
        );
        assert!(all.iter().any(|s| s.id == pinned.id));
        assert!(all.iter().any(|s| s.id == manual.id));
        std::fs::remove_dir_all(&inst).unwrap();
    }

    #[test]
    fn world_backup_and_rotation() {
        let (inst, game) = setup("world");
        std::fs::create_dir_all(game.join("saves/База у реки/region")).unwrap();
        std::fs::write(game.join("saves/База у реки/level.dat"), b"lvl").unwrap();
        std::fs::write(game.join("saves/База у реки/region/r.0.0.mca"), b"chunk").unwrap();
        let backups = inst.join("backups");
        let b = backup_world(&game, &backups, "База у реки").unwrap();
        assert!(b.file.exists() && b.size > 0);
        assert!(backup_world(&game, &backups, "../x").is_err());
        assert!(backup_world(&game, &backups, "нет такого").is_err());
        std::fs::remove_dir_all(&inst).unwrap();
    }

    #[test]
    fn snapshot_ids_are_checked() {
        let (inst, _game) = setup("ids");
        assert!(diff(&inst, &inst, "../../etc").is_err());
        std::fs::remove_dir_all(&inst).unwrap();
    }
}
