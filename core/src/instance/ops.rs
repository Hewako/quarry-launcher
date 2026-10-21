//! Операции над профилями: копия профиля, место на диске, перенос файлов между профилями.

use super::{Instance, InstanceStore};
use crate::error::{Error, IoContext, Result};
use crate::paths::{copy_dir, dir_size};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Что взять в копию профиля. Настройки запуска копируются всегда.
#[derive(Debug, Clone, Copy, Default, Deserialize)]
pub struct CloneParts {
    /// Моды и pack.lock.
    #[serde(default)]
    pub mods: bool,
    /// Настройки модов и игры: config/, options.txt, servers.dat.
    #[serde(default)]
    pub configs: bool,
    /// Ресурспаки и шейдеры.
    #[serde(default)]
    pub resources: bool,
    #[serde(default)]
    pub worlds: bool,
}

/// Скопировать профиль под новым именем. Аккаунт, журнал и история запусков не копируются.
pub fn clone_instance(
    store: &InstanceStore,
    id: &str,
    name: &str,
    parts: CloneParts,
) -> Result<Instance> {
    let src = store.get(id)?;
    let src_game = store.game_dir(id)?;
    let mut inst = store.create(
        name,
        &src.game_version,
        src.loader,
        src.loader_version.clone(),
    )?;
    inst.settings = src.settings.clone();
    inst.pack_settings = src.pack_settings.clone();
    inst.group = src.group.clone();
    inst.description = src.description.clone();
    inst.color = src.color.clone();
    store.save(&inst)?;

    let dst_game = store.game_dir(&inst.id)?;
    let mut items: Vec<&str> = Vec::new();
    if parts.mods {
        items.push("mods");
        let lock = store.dir(id)?.join("pack.lock");
        if lock.is_file() {
            let to = store.dir(&inst.id)?.join("pack.lock");
            std::fs::copy(&lock, &to).at(&to)?;
        }
    }
    if parts.configs {
        items.extend(["config", "options.txt", "servers.dat"]);
    }
    if parts.resources {
        items.extend(["resourcepacks", "shaderpacks"]);
    }
    if parts.worlds {
        items.push("saves");
    }
    for item in items {
        copy_item(&src_game.join(item), &dst_game.join(item))?;
    }
    Ok(inst)
}

fn copy_item(from: &Path, to: &Path) -> Result<()> {
    if from.is_dir() {
        copy_dir(from, to)?;
    } else if from.is_file() {
        if let Some(p) = to.parent() {
            std::fs::create_dir_all(p).at(p)?;
        }
        std::fs::copy(from, to).at(to)?;
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
pub struct DiskPart {
    pub label: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiskUsage {
    pub parts: Vec<DiskPart>,
    pub total: u64,
}

/// Сколько места занимает профиль — по частям, от большего к меньшему.
pub fn disk_usage(store: &InstanceStore, id: &str) -> Result<DiskUsage> {
    let game = store.game_dir(id)?;
    let dir = store.dir(id)?;
    let mut parts = Vec::new();
    let mut counted = 0;
    for (label, rel) in [
        ("Миры", &["saves"][..]),
        ("Моды", &["mods"]),
        ("Ресурспаки и шейдеры", &["resourcepacks", "shaderpacks"]),
        ("Скриншоты", &["screenshots"]),
        ("Настройки модов", &["config"]),
        ("Логи и отчёты о крашах", &["logs", "crash-reports"]),
    ] {
        let size: u64 = rel.iter().map(|r| dir_size(&game.join(r))).sum();
        counted += size;
        parts.push(DiskPart {
            label: label.into(),
            size,
        });
    }
    let game_total = dir_size(&game);
    parts.push(DiskPart {
        label: "Прочее в папке игры".into(),
        size: game_total.saturating_sub(counted),
    });
    parts.push(DiskPart {
        label: "Снимки модов".into(),
        size: dir_size(&dir.join("snapshots")),
    });
    parts.push(DiskPart {
        label: "Бэкапы миров".into(),
        size: dir_size(&crate::worlds::backups_dir(store.data(), id)),
    });
    parts.retain(|p| p.size > 0);
    parts.sort_by_key(|p| std::cmp::Reverse(p.size));
    Ok(DiskUsage {
        total: parts.iter().map(|p| p.size).sum(),
        parts,
    })
}

/// Что можно перенести из одного профиля в другой.
pub const SHARABLE: [(&str, &str); 6] = [
    ("options.txt", "Настройки игры (управление, графика, звук)"),
    ("servers.dat", "Список серверов"),
    ("resourcepacks", "Ресурспаки"),
    ("shaderpacks", "Шейдеры"),
    ("config", "Настройки модов"),
    ("saves", "Миры (копией, свои миры не заменяются)"),
];

/// Перенести файлы из профиля `from` в `to`. Перед этим у `to` создаётся снимок,
/// а заменяемые папки ресурсов не удаляются — новые файлы добавляются к ним.
pub fn copy_between(store: &InstanceStore, from: &str, to: &str, items: &[String]) -> Result<()> {
    if from == to {
        return Err(Error::invalid("Нужно выбрать другой профиль"));
    }
    let src = store.game_dir(from)?;
    let dst = store.game_dir(to)?;
    for item in items {
        if !SHARABLE.iter().any(|(k, _)| k == item) {
            return Err(Error::invalid(format!("Нельзя перенести {item}")));
        }
    }
    let name = store.get(from)?.name;
    crate::snapshot::create(
        &store.dir(to)?,
        &dst,
        &format!("Перед переносом из «{name}»"),
        true,
    )?;
    for item in items {
        if item == "saves" {
            copy_worlds(&src.join("saves"), &dst.join("saves"), &name)?;
        } else {
            copy_item(&src.join(item), &dst.join(item))?;
        }
    }
    Ok(())
}

/// Скопировать миры: мир с уже занятым именем получает приписку «(из <профиль>)».
fn copy_worlds(from: &Path, to: &Path, source_name: &str) -> Result<()> {
    let Ok(rd) = std::fs::read_dir(from) else {
        return Ok(());
    };
    for e in rd.flatten() {
        if !e.path().join("level.dat").is_file() {
            continue;
        }
        let name = e.file_name().to_string_lossy().into_owned();
        let mut target = to.join(&name);
        let mut n = 2;
        while target.exists() {
            let suffix = if n == 2 {
                String::new()
            } else {
                format!(" {n}")
            };
            target = to.join(format!("{name} (из {source_name}){suffix}"));
            n += 1;
        }
        copy_dir(&e.path(), &target)?;
    }
    Ok(())
}

/// Перенести папку игры профиля в `target_parent/<id>` (например, на другой диск) или,
/// при `None`, обратно в папку данных Quarry. Профиль запоминает новое место.
pub fn move_game_dir(
    store: &InstanceStore,
    id: &str,
    target_parent: Option<&Path>,
) -> Result<std::path::PathBuf> {
    let mut inst = store.get(id)?;
    let from = store.game_dir(id)?;
    // Подключённую папку официального лаунчера не трогаем: её перенос сломал бы его.
    if inst.external_game_dir.is_some()
        && from
            .file_name()
            .is_some_and(|n| n == ".minecraft" || n == "minecraft")
    {
        return Err(Error::invalid(
            "Это папка официального лаунчера — переносить её нельзя",
        ));
    }
    let internal = store.dir(id)?.join("game");
    let to = match target_parent {
        Some(p) => {
            // Сравниваем настоящие пути (символические ссылки раскрыты).
            let real = std::fs::canonicalize(p).at(p)?;
            if real.starts_with(std::fs::canonicalize(&from).at(&from)?) {
                return Err(Error::invalid("Нельзя перенести папку внутрь неё самой"));
            }
            p.join(&inst.id)
        }
        None => internal.clone(),
    };
    if to == from {
        return Ok(to);
    }
    if to.exists() {
        return Err(Error::invalid(format!(
            "Папка {} уже существует — выберите другое место",
            to.display()
        )));
    }
    if let Some(p) = to.parent() {
        std::fs::create_dir_all(p).at(p)?;
    }
    // На том же диске — мгновенное переименование, на другом — копия и удаление исходной.
    if std::fs::rename(&from, &to).is_err() {
        copy_then_remove(&from, &to)?;
    }
    inst.external_game_dir = (to != internal).then(|| to.clone());
    store.save(&inst)?;
    Ok(to)
}

/// Скопировать папку и удалить исходную — только если копия совпала по размеру.
fn copy_then_remove(from: &Path, to: &Path) -> Result<()> {
    copy_dir(from, to)?;
    if dir_size(from) != dir_size(to) {
        let _ = std::fs::remove_dir_all(to);
        return Err(Error::invalid(
            "Копия получилась неполной — перенос отменён, исходная папка не тронута",
        ));
    }
    std::fs::remove_dir_all(from).at(from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loader::LoaderKind;
    use crate::paths::DataDir;

    #[test]
    fn clone_usage_and_copy() {
        let root = std::env::temp_dir().join(format!("quarry-ops-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let store = InstanceStore::new(DataDir::open(&root).unwrap());
        let mut a = store
            .create("База", "26.3", LoaderKind::Fabric, Some("0.19".into()))
            .unwrap();
        a.color = Some("#4a8".into());
        a.settings.max_memory_mb = Some(6144);
        store.save(&a).unwrap();
        let g = store.game_dir(&a.id).unwrap();
        for (f, n) in [
            ("mods/x.jar", 10),
            ("saves/W/level.dat", 30),
            ("options.txt", 5),
        ] {
            std::fs::create_dir_all(g.join(f).parent().unwrap()).unwrap();
            std::fs::write(g.join(f), vec![0u8; n]).unwrap();
        }

        let b = clone_instance(
            &store,
            &a.id,
            "Копия",
            CloneParts {
                mods: true,
                ..Default::default()
            },
        )
        .unwrap();
        let gb = store.game_dir(&b.id).unwrap();
        assert!(gb.join("mods/x.jar").is_file());
        assert!(!gb.join("saves").exists());
        assert!(!gb.join("options.txt").exists());
        assert_eq!(b.settings.max_memory_mb, Some(6144));
        assert_eq!(b.color.as_deref(), Some("#4a8"));
        assert!(b.last_launch.is_none());

        let u = disk_usage(&store, &a.id).unwrap();
        assert_eq!(u.parts[0].label, "Миры");
        assert_eq!(u.total, 45);

        copy_between(&store, &a.id, &b.id, &["options.txt".into()]).unwrap();
        assert!(gb.join("options.txt").is_file());
        assert!(copy_between(&store, &a.id, &b.id, &["mods".into()]).is_err());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn move_game_folder_out_and_back() {
        let root = std::env::temp_dir().join(format!("quarry-move-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let store = InstanceStore::new(DataDir::open(root.join("data")).unwrap());
        let a = store
            .create("Большой", "26.3", LoaderKind::Vanilla, None)
            .unwrap();
        let g = store.game_dir(&a.id).unwrap();
        std::fs::create_dir_all(g.join("saves/W")).unwrap();
        std::fs::write(g.join("saves/W/level.dat"), b"x").unwrap();

        let disk = root.join("other-disk");
        std::fs::create_dir_all(&disk).unwrap();
        let moved = move_game_dir(&store, &a.id, Some(&disk)).unwrap();
        assert_eq!(moved, disk.join(&a.id));
        assert!(moved.join("saves/W/level.dat").is_file());
        assert!(!g.exists());
        assert_eq!(store.game_dir(&a.id).unwrap(), moved);

        // Обратно в папку данных Quarry.
        let back = move_game_dir(&store, &a.id, None).unwrap();
        assert_eq!(back, g);
        assert!(g.join("saves/W/level.dat").is_file());
        assert!(store.get(&a.id).unwrap().external_game_dir.is_none());

        // Внутрь самой папки игры переносить нельзя.
        assert!(move_game_dir(&store, &a.id, Some(&g.join("saves"))).is_err());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn copy_fallback_moves_everything() {
        let root = std::env::temp_dir().join(format!("quarry-movecopy-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("a/sub")).unwrap();
        std::fs::write(root.join("a/sub/f"), b"1234").unwrap();
        copy_then_remove(&root.join("a"), &root.join("b")).unwrap();
        assert!(!root.join("a").exists());
        assert_eq!(std::fs::read(root.join("b/sub/f")).unwrap(), b"1234");
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn copying_worlds_never_overwrites_existing() {
        let root = std::env::temp_dir().join(format!("quarry-saves-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let store = InstanceStore::new(DataDir::open(&root).unwrap());
        let a = store
            .create("Откуда", "26.3", LoaderKind::Vanilla, None)
            .unwrap();
        let b = store
            .create("Куда", "26.3", LoaderKind::Vanilla, None)
            .unwrap();
        let (ga, gb) = (
            store.game_dir(&a.id).unwrap(),
            store.game_dir(&b.id).unwrap(),
        );
        for (g, body) in [(&ga, "new"), (&gb, "old")] {
            std::fs::create_dir_all(g.join("saves/World")).unwrap();
            std::fs::write(g.join("saves/World/level.dat"), body).unwrap();
        }
        copy_between(&store, &a.id, &b.id, &["saves".into()]).unwrap();
        assert_eq!(
            std::fs::read_to_string(gb.join("saves/World/level.dat")).unwrap(),
            "old"
        );
        assert_eq!(
            std::fs::read_to_string(gb.join("saves/World (из Откуда)/level.dat")).unwrap(),
            "new"
        );
        std::fs::remove_dir_all(&root).unwrap();
    }
}
