//! Наборы модов по назначению: оптимизация, визуал, удобство, серверные, разработка.
//! Группа включается и выключается целиком. Хранится в `mod-groups.toml` профиля.

use crate::error::{Error, Result};
use crate::instance::InstanceStore;
use crate::paths::write_atomic;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

pub const GROUPS: [&str; 5] = [
    "Оптимизация",
    "Визуал",
    "Удобство",
    "Серверные",
    "Разработка",
];

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ModGroups {
    /// Имя файла мода (без `.disabled`) → группа.
    #[serde(default)]
    pub groups: BTreeMap<String, String>,
}

const FILE: &str = "mod-groups.toml";

impl ModGroups {
    pub fn load(instance_dir: &Path) -> Result<Self> {
        let path = instance_dir.join(FILE);
        match std::fs::read_to_string(&path) {
            Ok(t) => toml::from_str(&t).map_err(|e| Error::parse(FILE, e)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(Error::io(&path, e)),
        }
    }
    pub fn save(&self, instance_dir: &Path) -> Result<()> {
        let text = toml::to_string_pretty(self).map_err(|e| Error::parse(FILE, e))?;
        write_atomic(&instance_dir.join(FILE), text.as_bytes())
    }
}

/// Группа по категориям и сторонам Modrinth. Библиотеки не раскладываются: выключать
/// их группой опасно — от них зависят другие моды.
pub fn suggest(
    categories: &[String],
    client_side: &str,
    server_side: &str,
) -> Option<&'static str> {
    let has = |c: &str| categories.iter().any(|x| x == c);
    if has("library") {
        return None;
    }
    if client_side == "unsupported" && server_side == "required" {
        return Some("Серверные");
    }
    if has("optimization") {
        Some("Оптимизация")
    } else if has("decoration") {
        Some("Визуал")
    } else if has("utility") || has("management") || has("storage") || has("social") {
        Some("Удобство")
    } else {
        None
    }
}

fn check_file(file_name: &str) -> Result<()> {
    if file_name.contains(['/', '\\']) || file_name.contains("..") || !file_name.ends_with(".jar") {
        return Err(Error::invalid(format!(
            "Недопустимое имя файла мода: {file_name}"
        )));
    }
    Ok(())
}

/// Отнести мод к группе (`None` — убрать из группы).
pub fn set_group(
    store: &InstanceStore,
    id: &str,
    file_name: &str,
    group: Option<&str>,
) -> Result<()> {
    check_file(file_name)?;
    if let Some(g) = group
        && !GROUPS.contains(&g)
    {
        return Err(Error::invalid(format!("Нет такой группы: {g}")));
    }
    let dir = store.dir(id)?;
    let mut groups = ModGroups::load(&dir)?;
    match group {
        Some(g) => groups.groups.insert(file_name.to_string(), g.to_string()),
        None => groups.groups.remove(file_name),
    };
    groups.save(&dir)
}

/// Включить или выключить все моды группы. Возвращает файлы, которые изменились.
pub fn toggle_group(
    store: &InstanceStore,
    id: &str,
    group: &str,
    enabled: bool,
) -> Result<Vec<String>> {
    let groups = ModGroups::load(&store.dir(id)?)?;
    let mods = store.game_dir(id)?.join("mods");
    let mut changed = Vec::new();
    for (file, g) in &groups.groups {
        if g != group {
            continue;
        }
        let is_on = mods.join(file).is_file();
        let is_off = mods.join(format!("{file}.disabled")).is_file();
        if (enabled && is_off) || (!enabled && is_on) {
            super::manage::toggle(store, id, file, enabled, &format!("группа «{group}»"))?;
            changed.push(file.clone());
        }
    }
    Ok(changed)
}

/// Разложить по группам моды без группы, опознанные на Modrinth. Возвращает число разложенных.
pub async fn auto_assign(store: &InstanceStore, id: &str) -> Result<usize> {
    let dir = store.dir(id)?;
    let files = super::inventory(&store.game_dir(id)?, &dir)?;
    let mut groups = ModGroups::load(&dir)?;
    let todo: Vec<&super::ModFile> = files
        .iter()
        .filter(|f| !groups.groups.contains_key(&f.file_name))
        .collect();
    // Проект — из pack.lock или по хэшу файла.
    let mut project_of: BTreeMap<String, String> = BTreeMap::new();
    let mut unknown = Vec::new();
    for f in &todo {
        match f.source.as_ref().and_then(|s| s.project_id.clone()) {
            Some(p) => {
                project_of.insert(f.file_name.clone(), p);
            }
            None => unknown.push(f.sha1.clone()),
        }
    }
    if let Ok(found) = crate::catalog::versions_by_hash(&unknown).await {
        for f in &todo {
            if let Some(v) = found.get(&f.sha1) {
                project_of.insert(f.file_name.clone(), v.project_id.clone());
            }
        }
    }
    let ids: Vec<String> = project_of.values().cloned().collect();
    let projects = crate::catalog::projects(&ids).await?;
    let mut n = 0;
    for (file, pid) in project_of {
        if let Some(p) = projects.iter().find(|p| p.id == pid)
            && let Some(g) = suggest(&p.categories, &p.client_side, &p.server_side)
        {
            groups.groups.insert(file, g.to_string());
            n += 1;
        }
    }
    groups.save(&dir)?;
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loader::LoaderKind;
    use crate::paths::DataDir;

    fn cats(c: &[&str]) -> Vec<String> {
        c.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn suggestion_by_modrinth_categories() {
        assert_eq!(
            suggest(&cats(&["optimization"]), "required", "optional"),
            Some("Оптимизация")
        );
        assert_eq!(
            suggest(&cats(&["decoration"]), "required", "unsupported"),
            Some("Визуал")
        );
        assert_eq!(
            suggest(&cats(&["utility"]), "optional", "optional"),
            Some("Удобство")
        );
        assert_eq!(
            suggest(&cats(&["management"]), "unsupported", "required"),
            Some("Серверные")
        );
        // Библиотеки не раскладываем: выключать их группой опасно.
        assert_eq!(
            suggest(&cats(&["library", "optimization"]), "required", "required"),
            None
        );
        assert_eq!(suggest(&cats(&["magic"]), "required", "required"), None);
    }

    #[test]
    fn group_toggles_all_its_mods_and_survives_reload() {
        let root = std::env::temp_dir().join(format!("quarry-groups-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let store = InstanceStore::new(DataDir::open(&root).unwrap());
        let inst = store.create("G", "26.3", LoaderKind::Fabric, None).unwrap();
        let mods = store.game_dir(&inst.id).unwrap().join("mods");
        std::fs::create_dir_all(&mods).unwrap();
        for f in ["sodium.jar", "lithium.jar", "jei.jar"] {
            std::fs::write(mods.join(f), b"jar").unwrap();
        }
        set_group(&store, &inst.id, "sodium.jar", Some("Оптимизация")).unwrap();
        set_group(&store, &inst.id, "lithium.jar", Some("Оптимизация")).unwrap();
        set_group(&store, &inst.id, "jei.jar", Some("Удобство")).unwrap();
        assert!(set_group(&store, &inst.id, "jei.jar", Some("Нет такой")).is_err());

        let changed = toggle_group(&store, &inst.id, "Оптимизация", false).unwrap();
        assert_eq!(changed.len(), 2);
        assert!(
            mods.join("sodium.jar.disabled").is_file()
                && mods.join("lithium.jar.disabled").is_file()
        );
        assert!(mods.join("jei.jar").is_file());
        // Группа помнит выключенные моды по имени без .disabled.
        toggle_group(&store, &inst.id, "Оптимизация", true).unwrap();
        assert!(mods.join("sodium.jar").is_file());

        set_group(&store, &inst.id, "jei.jar", None).unwrap();
        let g = ModGroups::load(&store.dir(&inst.id).unwrap()).unwrap();
        assert_eq!(g.groups.len(), 2);
        std::fs::remove_dir_all(&root).unwrap();
    }
}
