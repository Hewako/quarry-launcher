//! Установка, обновление и удаление модов. Каждое изменение: план (предпросмотр) →
//! автоматический снимок → изменение → запись в pack.lock и журнал. Молча ничего не удаляется.

use super::lock::{Layer, LockEntry, PackLock, Source};
use super::{ModFile, inventory};
use crate::catalog::{self, Version};
use crate::download::{self, Cancel, Expected, Task, Verify};
use crate::error::{Error, IoContext, Result};
use crate::instance::{Instance, InstanceStore, now};
use crate::snapshot::{self, SnapshotInfo};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    Add,
    Update,
    Remove,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Change {
    pub kind: ChangeKind,
    pub title: String,
    /// Файл, который появится (для Add/Update).
    pub file_name: Option<String>,
    /// Файл, который уйдёт (для Update/Remove).
    pub old_file: Option<String>,
    pub old_version: Option<String>,
    pub new_version: Option<String>,
    /// Почему: «выбран вами», «нужен для Iris».
    pub reason: String,
    pub size: u64,
    #[serde(skip)]
    version: Option<Version>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    pub changes: Vec<Change>,
    /// Пояснения: «необязательные зависимости не ставятся» и т. п.
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalEntry {
    pub at: u64,
    pub action: String,
    pub what: String,
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<String>,
}

pub fn journal_append(instance_dir: &Path, e: &JournalEntry) -> Result<()> {
    use std::io::Write;
    let path = instance_dir.join("journal.jsonl");
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .at(&path)?;
    let line = serde_json::to_string(e).map_err(|err| Error::parse("журнал", err))?;
    writeln!(f, "{line}").at(&path)
}

pub fn journal(instance_dir: &Path) -> Vec<JournalEntry> {
    std::fs::read_to_string(instance_dir.join("journal.jsonl"))
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

struct Ctx {
    instance: Instance,
    game_dir: std::path::PathBuf,
    instance_dir: std::path::PathBuf,
    loaders: Vec<&'static str>,
}

fn ctx(store: &InstanceStore, id: &str) -> Result<Ctx> {
    let instance = store.get(id)?;
    let loaders = catalog::loaders_for(instance.loader);
    if loaders.is_empty() {
        return Err(Error::invalid(
            "В профиле без загрузчика моды не работают — выберите профиль с Fabric, Quilt или NeoForge",
        ));
    }
    Ok(Ctx {
        game_dir: store.game_dir(id)?,
        instance_dir: store.dir(id)?,
        instance,
        loaders,
    })
}

/// Проекты Modrinth, которые уже есть в профиле: по pack.lock и по хэшам файлов.
async fn present_projects(files: &[ModFile]) -> BTreeMap<String, String> {
    let mut out: BTreeMap<String, String> = BTreeMap::new();
    let mut unknown = Vec::new();
    for f in files {
        match f.source.as_ref().and_then(|s| s.project_id.clone()) {
            Some(p) => {
                out.insert(p, f.file_name.clone());
            }
            None => unknown.push(f.sha1.clone()),
        }
    }
    if let Ok(found) = catalog::versions_by_hash(&unknown).await {
        for (sha1, v) in found {
            if let Some(f) = files.iter().find(|f| f.sha1 == sha1) {
                out.insert(v.project_id, f.file_name.clone());
            }
        }
    }
    out
}

/// План установки проекта Modrinth вместе с обязательными зависимостями.
pub async fn plan_install(store: &InstanceStore, id: &str, project: &str) -> Result<Plan> {
    let c = ctx(store, id)?;
    let files = inventory(&c.game_dir, &c.instance_dir)?;
    let present = present_projects(&files).await;
    let mut plan = Plan {
        changes: Vec::new(),
        notes: Vec::new(),
    };
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut queue: Vec<(String, String)> = vec![(project.to_string(), "выбран вами".into())];
    while let Some((proj, reason)) = queue.pop() {
        if !seen.insert(proj.clone()) || seen.len() > 40 {
            continue;
        }
        let info = catalog::project(&proj).await?;
        if let Some(file) = present.get(&info.id) {
            if proj == project {
                return Err(Error::invalid(format!(
                    "{} уже установлен ({file}). Обновить его можно кнопкой «Обновления»",
                    info.title
                )));
            }
            continue;
        }
        let list = catalog::versions(&info.id, &c.loaders, &c.instance.game_version).await?;
        let Some(v) = catalog::pick_best(&list).cloned() else {
            if proj == project {
                return Err(Error::invalid(format!(
                    "У {} нет версии для Minecraft {} и {}",
                    info.title,
                    c.instance.game_version,
                    c.instance.loader.display()
                )));
            }
            plan.notes.push(format!(
                "Нужен «{}», но подходящей версии нет — проверьте вручную",
                info.title
            ));
            continue;
        };
        for d in &v.dependencies {
            match (d.dependency_type.as_str(), &d.project_id) {
                ("required", Some(p)) => queue.push((p.clone(), format!("нужен для {}", info.title))),
                ("optional", Some(_)) => {}
                ("incompatible", Some(p)) if present.contains_key(p) => plan.notes.push(format!(
                    "{} несовместим с уже установленным модом ({}) — Mod Doctor покажет подробности",
                    info.title,
                    present[p]
                )),
                _ => {}
            }
        }
        let file = v
            .primary_file()
            .ok_or_else(|| Error::invalid(format!("У версии {} нет файла", v.name)))?;
        plan.changes.push(Change {
            kind: ChangeKind::Add,
            title: info.title.clone(),
            file_name: Some(file.filename.clone()),
            old_file: None,
            old_version: None,
            new_version: Some(v.version_number.clone()),
            reason,
            size: file.size,
            version: Some(v.clone()),
        });
    }
    plan.changes.reverse(); // зависимости первыми
    Ok(plan)
}

#[derive(Debug, Clone, Serialize)]
pub struct UpdateInfo {
    pub file_name: String,
    pub title: String,
    pub current: String,
    pub latest: String,
}

/// Проверить обновления всех модов профиля (одним запросом по хэшам).
pub async fn check_updates(store: &InstanceStore, id: &str) -> Result<Plan> {
    let c = ctx(store, id)?;
    let files = inventory(&c.game_dir, &c.instance_dir)?;
    let hashes: Vec<String> = files.iter().map(|f| f.sha1.clone()).collect();
    let updates = catalog::updates_by_hash(&hashes, &c.loaders, &c.instance.game_version).await?;
    // Текущие версии — чтобы не предлагать бету тому, у кого стоит стабильная.
    let current = catalog::versions_by_hash(&hashes).await.unwrap_or_default();
    let mut plan = Plan {
        changes: Vec::new(),
        notes: Vec::new(),
    };
    for f in &files {
        let Some(v) = updates.get(&f.sha1) else {
            continue;
        };
        let Some(file) = v.primary_file() else {
            continue;
        };
        if file
            .hashes
            .get("sha1")
            .is_some_and(|h| h.eq_ignore_ascii_case(&f.sha1))
        {
            continue; // уже последняя
        }
        if let Some(cur) = current.get(&f.sha1)
            && (stability(&v.version_type) > stability(&cur.version_type) || cur.id == v.id)
        {
            continue;
        }
        plan.changes.push(Change {
            kind: ChangeKind::Update,
            title: f.display_name(),
            file_name: Some(file.filename.clone()),
            old_file: Some(f.rel_path.trim_start_matches("mods/").to_string()),
            old_version: f
                .primary()
                .map(|m| m.version.clone())
                .or_else(|| f.source.as_ref().and_then(|s| s.version_number.clone())),
            new_version: Some(v.version_number.clone()),
            reason: if v.version_type == "release" {
                "новая версия".into()
            } else {
                format!("новая версия ({})", v.version_type)
            },
            size: file.size,
            version: Some(v.clone()),
        });
    }
    if files.iter().any(|f| f.source.is_none()) {
        plan.notes.push("Моды, которых нет на Modrinth, проверить на обновления нельзя — их нужно обновлять вручную".into());
    }
    Ok(plan)
}

/// 0 — release, 1 — beta, 2 — alpha.
fn stability(t: &str) -> u8 {
    match t {
        "release" => 0,
        "beta" => 1,
        _ => 2,
    }
}

fn safe_file_name(name: &str) -> Result<&str> {
    if name.is_empty()
        || name.contains(['/', '\\'])
        || name.contains("..")
        || !name.ends_with(".jar")
    {
        return Err(Error::invalid(format!("Недопустимое имя файла: {name}")));
    }
    Ok(name)
}

/// Применить план (возможно, частично: только выбранные `titles`). Перед изменением — снимок.
pub async fn apply(
    store: &InstanceStore,
    id: &str,
    plan: Plan,
    reason: &str,
    cancel: &Cancel,
) -> Result<SnapshotInfo> {
    let c = ctx(store, id)?;
    if plan.changes.is_empty() {
        return Err(Error::invalid("Нечего менять"));
    }
    let snap = snapshot::create(&c.instance_dir, &c.game_dir, reason, true)?;
    let mods_dir = c.game_dir.join("mods");
    std::fs::create_dir_all(&mods_dir).at(&mods_dir)?;
    let mut tasks = Vec::new();
    for ch in &plan.changes {
        if let (Some(v), Some(name)) = (&ch.version, &ch.file_name) {
            let f = v
                .primary_file()
                .ok_or_else(|| Error::invalid("У версии нет файла"))?;
            let sha1 = f
                .hashes
                .get("sha1")
                .ok_or_else(|| Error::invalid("Modrinth не прислал хэш файла"))?;
            tasks.push(Task {
                url: f.url.clone(),
                dest: mods_dir.join(safe_file_name(name)?),
                expected: Expected::Sha1(sha1.to_ascii_lowercase()),
                size: Some(f.size),
            });
        }
    }
    download::download_all(tasks, 6, Verify::Full, None, cancel).await?;

    let mut lock = PackLock::load(&c.instance_dir)?;
    for ch in &plan.changes {
        if let Some(old) = &ch.old_file {
            let old = safe_file_name(old.trim_end_matches(".disabled")).map(|_| old.as_str())?;
            if ch.file_name.as_deref() != Some(old) {
                for candidate in [mods_dir.join(old), mods_dir.join(format!("{old}.disabled"))] {
                    if candidate.exists() {
                        std::fs::remove_file(&candidate).at(&candidate)?;
                    }
                }
            }
            lock.remove(&format!("mods/{old}"));
        }
        if let (Some(v), Some(name)) = (&ch.version, &ch.file_name) {
            let f = v.primary_file().expect("проверено выше");
            lock.upsert(LockEntry {
                path: format!("mods/{name}"),
                sha1: f.hashes.get("sha1").cloned().unwrap_or_default(),
                sha512: f.hashes.get("sha512").cloned(),
                source: Source::Modrinth,
                project_id: Some(v.project_id.clone()),
                version_id: Some(v.id.clone()),
                version_number: Some(v.version_number.clone()),
                title: Some(ch.title.clone()),
                layer: Layer::User,
                installed_at: now(),
            });
        }
        journal_append(
            &c.instance_dir,
            &JournalEntry {
                at: now(),
                action: match ch.kind {
                    ChangeKind::Add => "Установлено",
                    ChangeKind::Update => "Обновлено",
                    ChangeKind::Remove => "Удалено",
                }
                .into(),
                what: format!(
                    "{} {}",
                    ch.title,
                    match (&ch.old_version, &ch.new_version) {
                        (Some(o), Some(n)) => format!("{o} → {n}"),
                        (None, Some(n)) => n.clone(),
                        (Some(o), None) => o.clone(),
                        _ => String::new(),
                    }
                ),
                reason: ch.reason.clone(),
                snapshot: Some(snap.id.clone()),
            },
        )?;
    }
    lock.save(&c.instance_dir)?;
    Ok(snap)
}

/// Удалить мод из профиля (после снимка — вернуть можно откатом).
pub fn remove(
    store: &InstanceStore,
    id: &str,
    file_name: &str,
    reason: &str,
) -> Result<SnapshotInfo> {
    let game_dir = store.game_dir(id)?;
    let instance_dir = store.dir(id)?;
    let base = safe_file_name(file_name)?;
    let snap = snapshot::create(
        &instance_dir,
        &game_dir,
        &format!("Перед удалением {base}"),
        true,
    )?;
    let mut removed = false;
    for p in [
        game_dir.join("mods").join(base),
        game_dir.join("mods").join(format!("{base}.disabled")),
    ] {
        if p.exists() {
            std::fs::remove_file(&p).at(&p)?;
            removed = true;
        }
    }
    if !removed {
        return Err(Error::NotFound(format!("Мод {base} не найден")));
    }
    let mut lock = PackLock::load(&instance_dir)?;
    lock.remove(&format!("mods/{base}"));
    lock.save(&instance_dir)?;
    journal_append(
        &instance_dir,
        &JournalEntry {
            at: now(),
            action: "Удалено".into(),
            what: base.to_string(),
            reason: reason.to_string(),
            snapshot: Some(snap.id.clone()),
        },
    )?;
    Ok(snap)
}

/// Отметить в журнале включение/выключение мода (сам файл переименовывает `mods::set_enabled`).
pub fn toggle(
    store: &InstanceStore,
    id: &str,
    file_name: &str,
    enabled: bool,
    reason: &str,
) -> Result<()> {
    let game_dir = store.game_dir(id)?;
    super::set_enabled(&game_dir, file_name, enabled)?;
    journal_append(
        &store.dir(id)?,
        &JournalEntry {
            at: now(),
            action: if enabled {
                "Включено"
            } else {
                "Выключено"
            }
            .into(),
            what: file_name.to_string(),
            reason: reason.to_string(),
            snapshot: None,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loader::LoaderKind;
    use crate::paths::DataDir;

    #[test]
    fn remove_takes_snapshot_and_writes_journal() {
        let root = std::env::temp_dir().join(format!("quarry-manage-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let store = InstanceStore::new(DataDir::open(&root).unwrap());
        let inst = store
            .create("Mods", "1.21.11", LoaderKind::Fabric, Some("0.17".into()))
            .unwrap();
        let game = store.game_dir(&inst.id).unwrap();
        std::fs::create_dir_all(game.join("mods")).unwrap();
        std::fs::write(game.join("mods/indium.jar"), b"jar").unwrap();
        let snap = remove(&store, &inst.id, "indium.jar", "выбрано в Mod Doctor").unwrap();
        assert!(!game.join("mods/indium.jar").exists());
        let j = journal(&store.dir(&inst.id).unwrap());
        assert_eq!(j.last().unwrap().action, "Удалено");
        assert_eq!(
            j.last().unwrap().snapshot.as_deref(),
            Some(snap.id.as_str())
        );
        // Вернуть можно откатом.
        snapshot::restore(&store.dir(&inst.id).unwrap(), &game, &snap.id).unwrap();
        assert!(game.join("mods/indium.jar").exists());
        assert!(remove(&store, &inst.id, "../x.jar", "").is_err());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn vanilla_profile_has_no_mods() {
        let root = std::env::temp_dir().join(format!("quarry-manage-v-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let store = InstanceStore::new(DataDir::open(&root).unwrap());
        let inst = store
            .create("V", "1.21.11", LoaderKind::Vanilla, None)
            .unwrap();
        assert!(ctx(&store, &inst.id).is_err());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[tokio::test]
    #[ignore = "сеть: api.modrinth.com, скачивает Iris и Sodium (~5 МБ)"]
    async fn live_install_with_dependency() {
        let root = std::env::temp_dir().join(format!("quarry-manage-live-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let store = InstanceStore::new(DataDir::open(&root).unwrap());
        let inst = store
            .create("Iris", "1.21.11", LoaderKind::Fabric, Some("0.17".into()))
            .unwrap();
        let plan = plan_install(&store, &inst.id, "iris")
            .await
            .map_err(|e| e.to_string())
            .unwrap();
        for c in &plan.changes {
            println!(
                "{:?} {} {:?} ({})",
                c.kind, c.title, c.new_version, c.reason
            );
        }
        assert!(
            plan.changes
                .iter()
                .any(|c| c.title == "Sodium" && c.reason.contains("Iris"))
        );
        apply(
            &store,
            &inst.id,
            plan,
            "Перед установкой Iris",
            &Cancel::new(),
        )
        .await
        .unwrap();
        let inv = inventory(
            &store.game_dir(&inst.id).unwrap(),
            &store.dir(&inst.id).unwrap(),
        )
        .unwrap();
        assert!(inv.iter().all(|f| f.source.is_some()));
        let updates = check_updates(&store, &inst.id).await.unwrap();
        for c in &updates.changes {
            println!(
                "обновление: {} {:?} -> {:?}",
                c.title, c.old_version, c.new_version
            );
        }
        assert!(
            updates.changes.is_empty(),
            "только что установленное — последняя версия"
        );
        std::fs::remove_dir_all(&root).unwrap();
    }
}
