//! Импорт: папка инстанса Prism/MultiMC, `.mrpack`, zip CurseForge и существующая `.minecraft`
//! без копирования. Источник только читается: ошибка импорта не меняет исходные файлы, а
//! недоделанный профиль убирается.
//!
//! Форматы: Prism/MultiMC — `instance.cfg` (ключ=значение) и `mmc-pack.json` (`components[].uid`
//! и `version`); `.mrpack` — `modrinth.index.json` (support.modrinth.com, формат 1);
//! CurseForge — `manifest.json` (`minecraft.version`, `modLoaders[].id` вида `fabric-0.16.10`,
//! `files[].projectID/fileID`, `overrides`). Без API-ключа CurseForge файлы модов скачать
//! нельзя — лаунчер составляет список для ручной загрузки.

use crate::download::{self, Cancel, Expected, Task, Verify};
use crate::error::{Error, IoContext, Result};
use crate::instance::{Instance, InstanceStore, now};
use crate::loader::LoaderKind;
use crate::mods::lock::{Layer, LockEntry, PackLock, Source};
use crate::paths::{DataDir, safe_join, write_atomic};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize)]
pub struct ImportResult {
    pub instance: Instance,
    /// Что не удалось сделать автоматически (например, моды CurseForge для ручной загрузки).
    pub manual: Vec<ManualFile>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManualFile {
    pub project_id: u64,
    pub file_id: u64,
    /// Страница проекта по ID (переадресация CurseForge; формат ссылки не подтверждён
    /// первоисточником — если не откроется, ищите мод по ID на сайте).
    pub url: String,
}

/// Удалить недоделанный профиль, если импорт не удался.
fn cleanup_on_error<T>(store: &InstanceStore, id: &str, r: Result<T>) -> Result<T> {
    if r.is_err()
        && let Ok(dir) = store.dir(id)
    {
        let _ = std::fs::remove_dir_all(dir);
    }
    r
}

// ---------- Prism / MultiMC ----------

#[derive(Debug, Deserialize)]
struct MmcPack {
    #[serde(default)]
    components: Vec<MmcComponent>,
}

#[derive(Debug, Deserialize)]
struct MmcComponent {
    uid: String,
    #[serde(default)]
    version: Option<String>,
    #[serde(default, rename = "cachedVersion")]
    cached_version: Option<String>,
}

/// Разобрать `instance.cfg` (формат INI без секций или с `[General]`).
pub fn parse_instance_cfg(text: &str) -> BTreeMap<String, String> {
    text.lines()
        .filter(|l| !l.starts_with('[') && !l.starts_with('#'))
        .filter_map(|l| {
            l.split_once('=')
                .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DetectedPack {
    pub name: String,
    pub game_version: String,
    pub loader: LoaderKind,
    pub loader_version: Option<String>,
}

pub fn detect_mmc(dir: &Path) -> Result<DetectedPack> {
    let cfg = parse_instance_cfg(
        &std::fs::read_to_string(dir.join("instance.cfg")).at(dir.join("instance.cfg"))?,
    );
    let pack: MmcPack = serde_json::from_str(
        &std::fs::read_to_string(dir.join("mmc-pack.json")).at(dir.join("mmc-pack.json"))?,
    )
    .map_err(|e| Error::parse("mmc-pack.json", e))?;
    let ver = |c: &MmcComponent| c.version.clone().or_else(|| c.cached_version.clone());
    let mut game_version = None;
    let mut loader = LoaderKind::Vanilla;
    let mut loader_version = None;
    for c in &pack.components {
        match c.uid.as_str() {
            "net.minecraft" => game_version = ver(c),
            "net.fabricmc.fabric-loader" => (loader, loader_version) = (LoaderKind::Fabric, ver(c)),
            "org.quiltmc.quilt-loader" => (loader, loader_version) = (LoaderKind::Quilt, ver(c)),
            "net.neoforged" => (loader, loader_version) = (LoaderKind::NeoForge, ver(c)),
            "net.minecraftforge" => (loader, loader_version) = (LoaderKind::Forge, ver(c)),
            _ => {}
        }
    }
    Ok(DetectedPack {
        name: cfg.get("name").cloned().unwrap_or_else(|| {
            dir.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default()
        }),
        game_version: game_version
            .ok_or_else(|| Error::invalid("В mmc-pack.json нет версии Minecraft"))?,
        loader,
        loader_version,
    })
}

/// Папки с инстансами Prism и MultiMC на этом компьютере (для списка «Импорт из Prism»).
pub fn find_prism_instances() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(d) = dirs::data_dir() {
        roots.push(d.join("PrismLauncher/instances"));
        roots.push(d.join("MultiMC/instances"));
    }
    if let Some(h) = dirs::home_dir() {
        roots.push(h.join(".local/share/PrismLauncher/instances"));
    }
    let mut out = Vec::new();
    for r in roots {
        if let Ok(rd) = std::fs::read_dir(&r) {
            out.extend(
                rd.flatten()
                    .map(|e| e.path())
                    .filter(|p| p.join("mmc-pack.json").is_file()),
            );
        }
    }
    out.sort();
    out
}

/// Импорт инстанса Prism/MultiMC: копия папки игры, настройки памяти и JVM.
pub fn import_mmc(store: &InstanceStore, dir: &Path) -> Result<ImportResult> {
    let detected = detect_mmc(dir)?;
    if !detected.loader.is_supported() {
        return Err(Error::invalid(format!(
            "{} пока не поддерживается — такой инстанс импортировать нельзя",
            detected.loader.display()
        )));
    }
    let cfg = parse_instance_cfg(&std::fs::read_to_string(dir.join("instance.cfg")).at(dir)?);
    let mut inst = store.create(
        &detected.name,
        &detected.game_version,
        detected.loader,
        detected.loader_version.clone(),
    )?;
    let id = inst.id.clone();
    let r = (|| -> Result<ImportResult> {
        let src_game = [".minecraft", "minecraft"]
            .iter()
            .map(|n| dir.join(n))
            .find(|p| p.is_dir());
        let mut notes = Vec::new();
        if let Some(src) = src_game {
            let n = crate::paths::copy_dir(&src, &store.game_dir(&id)?)?;
            notes.push(format!("Скопировано файлов: {n} (оригинал не изменён)"));
        }
        if cfg.get("OverrideMemory").is_some_and(|v| v == "true") {
            inst.settings.max_memory_mb = cfg.get("MaxMemAlloc").and_then(|v| v.parse().ok());
        }
        if cfg.get("OverrideJavaArgs").is_some_and(|v| v == "true") {
            let args: Vec<String> = cfg
                .get("JvmArgs")
                .map(|a| {
                    a.split_whitespace()
                        // Память — ползунком; опасные аргументы чужого инстанса не переносим.
                        .filter(|x| {
                            !x.starts_with("-Xm") && crate::settings::jvm_arg_forbidden(x).is_none()
                        })
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default();
            if !args.is_empty() {
                inst.settings.jvm_args = Some(args);
            }
        }
        store.save(&inst)?;
        mark_imported(&store.dir(&id)?, &store.game_dir(&id)?)?;
        Ok(ImportResult {
            instance: inst.clone(),
            manual: Vec::new(),
            notes,
        })
    })();
    cleanup_on_error(store, &id, r)
}

/// Все JAR в mods/ — в pack.lock как «импортировано» (слой «база»).
fn mark_imported(instance_dir: &Path, game_dir: &Path) -> Result<()> {
    let mut lock = PackLock::load(instance_dir)?;
    if let Ok(rd) = std::fs::read_dir(game_dir.join("mods")) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if !name.ends_with(".jar") {
                continue;
            }
            let path = format!("mods/{name}");
            if lock.files.iter().any(|f| f.path == path) {
                continue;
            }
            lock.upsert(LockEntry {
                sha1: download::hash_file::<sha1::Sha1>(&e.path())?,
                path,
                sha512: None,
                source: Source::Import,
                project_id: None,
                version_id: None,
                version_number: None,
                title: None,
                layer: Layer::Base,
                installed_at: now(),
                pinned: false,
            });
        }
    }
    lock.save(instance_dir)
}

// ---------- .mrpack ----------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MrIndex {
    format_version: u32,
    #[serde(default)]
    game: String,
    name: String,
    #[serde(default)]
    version_id: String,
    files: Vec<MrFile>,
    dependencies: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MrFile {
    path: String,
    hashes: BTreeMap<String, String>,
    #[serde(default)]
    env: Option<BTreeMap<String, String>>,
    downloads: Vec<String>,
    #[serde(default)]
    file_size: u64,
}

/// Домены, с которых формат .mrpack разрешает скачивать файлы.
const MRPACK_HOSTS: &[&str] = &[
    "cdn.modrinth.com",
    "github.com",
    "raw.githubusercontent.com",
    "gitlab.com",
];

fn allowed_download(url: &str) -> bool {
    url.starts_with("https://") && MRPACK_HOSTS.contains(&crate::net::host_of(url).as_str())
}

fn loader_from_deps(
    deps: &BTreeMap<String, String>,
) -> Result<(String, LoaderKind, Option<String>)> {
    let game = deps
        .get("minecraft")
        .cloned()
        .ok_or_else(|| Error::invalid("В сборке не указана версия Minecraft"))?;
    let (loader, v) = if let Some(v) = deps.get("fabric-loader") {
        (LoaderKind::Fabric, Some(v.clone()))
    } else if let Some(v) = deps.get("quilt-loader") {
        (LoaderKind::Quilt, Some(v.clone()))
    } else if let Some(v) = deps.get("neoforge") {
        (LoaderKind::NeoForge, Some(v.clone()))
    } else if let Some(v) = deps.get("forge") {
        (LoaderKind::Forge, Some(v.clone()))
    } else {
        (LoaderKind::Vanilla, None)
    };
    Ok((game, loader, v))
}

/// Распаковать из архива папку `prefix/` в `dest` (безопасно).
/// Распаковать `prefix/…` в `dest`; пути из `skip` (относительно `dest`) не перезаписываются.
fn extract_prefix(
    zip_path: &Path,
    prefix: &str,
    dest: &Path,
    keep: impl Fn(&str) -> bool,
) -> Result<u64> {
    let file = std::fs::File::open(zip_path).at(zip_path)?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| Error::parse("архив сборки", e))?;
    let mut n = 0;
    for i in 0..zip.len() {
        let mut e = zip
            .by_index(i)
            .map_err(|e| Error::parse("архив сборки", e))?;
        let name = e.name().unwrap_or_default().to_string();
        let Some(rel) = name.strip_prefix(prefix) else {
            continue;
        };
        if e.is_dir() || rel.is_empty() || e.is_symlink() || !keep(rel) {
            continue;
        }
        let target = safe_join(dest, rel)
            .ok_or_else(|| Error::invalid(format!("Сборка содержит недопустимый путь: {name}")))?;
        if let Some(p) = target.parent() {
            std::fs::create_dir_all(p).at(p)?;
        }
        let mut buf = Vec::new();
        e.read_to_end(&mut buf).at(zip_path)?;
        std::fs::write(&target, buf).at(&target)?;
        n += 1;
    }
    Ok(n)
}

/// Папка с конфигами сборки как их выпустил автор — основа для слияния при следующем обновлении.
const PACK_BASE: &str = "pack-base";

/// Сохранить текстовые конфиги из overrides сборки как основу (старая основа заменяется).
fn save_pack_base(zip_path: &Path, instance_dir: &Path) -> Result<()> {
    let base = instance_dir.join(PACK_BASE);
    if base.exists() {
        std::fs::remove_dir_all(&base).at(&base)?;
    }
    for prefix in ["overrides/", "client-overrides/"] {
        extract_prefix(zip_path, prefix, &base, crate::configs::is_text_config)?;
    }
    Ok(())
}

/// Слить конфиг игрока с новой версией автора: основа — `pack-base`. `None` — нет основы,
/// файл не текстовый или правки пересекаются (тогда файл игрока остаётся как есть).
fn merged_config(
    zip_path: &Path,
    instance_dir: &Path,
    game_dir: &Path,
    rel: &str,
) -> Option<String> {
    if !crate::configs::is_text_config(rel) {
        return None;
    }
    let base = std::fs::read_to_string(safe_join(&instance_dir.join(PACK_BASE), rel)?).ok()?;
    let ours = std::fs::read_to_string(safe_join(game_dir, rel)?).ok()?;
    let theirs = read_zip_entry(zip_path, &format!("overrides/{rel}"))
        .or_else(|_| read_zip_entry(zip_path, &format!("client-overrides/{rel}")))
        .ok()?;
    crate::configs::merge3(&base, &ours, &theirs)
}

fn read_zip_entry(zip_path: &Path, name: &str) -> Result<String> {
    let file = std::fs::File::open(zip_path).at(zip_path)?;
    let mut zip = zip::ZipArchive::new(file)
        .map_err(|_| Error::invalid("Файл не является архивом сборки"))?;
    let mut e = zip
        .by_name(name)
        .map_err(|_| Error::invalid(format!("В архиве нет {name}")))?;
    let mut s = String::new();
    e.read_to_string(&mut s).at(zip_path)?;
    Ok(s)
}

fn read_index(path: &Path) -> Result<MrIndex> {
    let index: MrIndex = serde_json::from_str(&read_zip_entry(path, "modrinth.index.json")?)
        .map_err(|e| Error::parse("modrinth.index.json", e))?;
    if index.format_version != 1 || (index.game != "minecraft" && !index.game.is_empty()) {
        return Err(Error::invalid(format!(
            "Неподдерживаемый формат сборки (formatVersion {})",
            index.format_version
        )));
    }
    Ok(index)
}

fn client_env(f: &MrFile) -> Option<&str> {
    f.env
        .as_ref()
        .and_then(|e| e.get("client"))
        .map(String::as_str)
}

/// Файлы сборки для клиента: без серверных и без снятых опциональных.
fn client_files<'a>(index: &'a MrIndex, skip_optional: &[String]) -> Vec<&'a MrFile> {
    index
        .files
        .iter()
        .filter(|f| match client_env(f) {
            Some("unsupported") => false,
            Some("optional") => !skip_optional.contains(&f.path),
            _ => true,
        })
        .collect()
}

/// Задача загрузки и запись pack.lock для файла сборки (с проверкой пути, сайта и хэша).
fn file_entry(f: &MrFile, game_dir: &Path) -> Result<(Task, LockEntry)> {
    let dest = safe_join(game_dir, &f.path)
        .ok_or_else(|| Error::invalid(format!("Сборка содержит недопустимый путь: {}", f.path)))?;
    let url = f
        .downloads
        .iter()
        .find(|u| allowed_download(u))
        .ok_or_else(|| {
            Error::invalid(format!(
                "Файл {} предлагается скачать с неразрешённого сайта",
                f.path
            ))
        })?;
    let sha1 = f
        .hashes
        .get("sha1")
        .ok_or_else(|| Error::invalid(format!("У файла {} нет SHA-1", f.path)))?;
    // project_id и version_id видны в адресе cdn.modrinth.com/data/<проект>/versions/<версия>/.
    let parts: Vec<&str> = url.split('/').collect();
    let (project_id, version_id) = match parts.iter().position(|p| *p == "data") {
        Some(i) if parts.get(i + 2) == Some(&"versions") => (
            parts.get(i + 1).map(|s| s.to_string()),
            parts.get(i + 3).map(|s| s.to_string()),
        ),
        _ => (None, None),
    };
    Ok((
        Task {
            url: url.clone(),
            dest,
            expected: Expected::Sha1(sha1.to_ascii_lowercase()),
            size: Some(f.file_size).filter(|s| *s > 0),
        },
        LockEntry {
            path: f.path.replace('\\', "/"),
            sha1: sha1.clone(),
            sha512: f.hashes.get("sha512").cloned(),
            source: Source::Modrinth,
            project_id,
            version_id,
            version_number: None,
            title: None,
            layer: Layer::Base,
            installed_at: now(),
            pinned: false,
        },
    ))
}

#[derive(Debug, Clone, Serialize)]
pub struct MrpackPreview {
    pub name: String,
    pub version: String,
    pub game: String,
    pub loader: String,
    /// Опциональные компоненты (пути) — игрок выбирает, ставить ли их.
    pub optional: Vec<String>,
}

pub fn preview_mrpack(path: &Path) -> Result<MrpackPreview> {
    let index = read_index(path)?;
    let (game, loader, loader_version) = loader_from_deps(&index.dependencies)?;
    Ok(MrpackPreview {
        name: index.name.clone(),
        version: index.version_id.clone(),
        game,
        loader: format!(
            "{} {}",
            loader.display(),
            loader_version.unwrap_or_default()
        ),
        optional: index
            .files
            .iter()
            .filter(|f| client_env(f) == Some("optional"))
            .map(|f| f.path.clone())
            .collect(),
    })
}

pub async fn import_mrpack(
    store: &InstanceStore,
    path: &Path,
    skip_optional: &[String],
    cancel: &Cancel,
) -> Result<ImportResult> {
    let index = read_index(path)?;
    let (game, loader, loader_version) = loader_from_deps(&index.dependencies)?;
    if !loader.is_supported() {
        return Err(Error::invalid(format!(
            "Сборка на {} — этот загрузчик пока не поддерживается",
            loader.display()
        )));
    }
    let name = if index.version_id.is_empty() {
        index.name.clone()
    } else {
        format!("{} {}", index.name, index.version_id)
    };
    let mut inst = store.create(&name, &game, loader, loader_version)?;
    inst.pack = Some(crate::instance::PackRef {
        name: index.name.clone(),
        version: index.version_id.clone(),
        ..Default::default()
    });
    store.save(&inst)?;
    let id = inst.id.clone();
    let r = async {
        let game_dir = store.game_dir(&id)?;
        let mut tasks = Vec::new();
        let mut lock = PackLock::default();
        let files = client_files(&index, skip_optional);
        let skipped = index.files.len() - files.len();
        for f in files {
            let (task, entry) = file_entry(f, &game_dir)?;
            tasks.push(task);
            lock.upsert(entry);
        }
        download::download_all(tasks, 6, Verify::Full, None, cancel).await?;
        let mut overrides = extract_prefix(path, "overrides/", &game_dir, |_| true)?;
        overrides += extract_prefix(path, "client-overrides/", &game_dir, |_| true)?;
        save_pack_base(path, &store.dir(&id)?)?;
        lock.save(&store.dir(&id)?)?;
        let mut notes = vec![format!(
            "Файлов сборки: {}, из архива: {overrides}",
            index.files.len() - skipped
        )];
        if skipped > 0 {
            notes.push(format!(
                "Пропущено файлов (только для сервера или не выбраны): {skipped}"
            ));
        }
        Ok(ImportResult {
            instance: inst.clone(),
            manual: Vec::new(),
            notes,
        })
    }
    .await;
    cleanup_on_error(store, &id, r)
}

// ---------- Обновление сборки по слоям ----------

/// Что изменится при обновлении сборки. Слой «мои моды» и конфиги, которые правил игрок,
/// не трогаются.
#[derive(Debug, Clone, Serialize)]
pub struct PackUpdatePlan {
    pub from: String,
    pub to: String,
    pub add: Vec<String>,
    pub update: Vec<String>,
    pub remove: Vec<String>,
    pub kept_mods: Vec<String>,
    pub kept_configs: Vec<String>,
    /// Конфиги, которые правил игрок, но правки автора в них вливаются без конфликтов.
    pub merged: Vec<String>,
    pub optional: Vec<String>,
    /// Новая версия игры или загрузчика, если сборка их сменила.
    pub game_change: Option<String>,
}

fn zip_names(path: &Path) -> Result<Vec<String>> {
    let file = std::fs::File::open(path).at(path)?;
    let zip = zip::ZipArchive::new(file).map_err(|e| Error::parse("архив сборки", e))?;
    Ok(zip.file_names().flatten().map(|n| n.to_string()).collect())
}

fn plan_with(
    store: &InstanceStore,
    id: &str,
    path: &Path,
    skip_optional: &[String],
) -> Result<(PackUpdatePlan, MrIndex)> {
    let inst = store.get(id)?;
    let index = read_index(path)?;
    let (game, loader, loader_version) = loader_from_deps(&index.dependencies)?;
    if loader != inst.loader {
        return Err(Error::invalid(format!(
            "Новая версия сборки на {}, а профиль на {} — создайте для неё новый профиль",
            loader.display(),
            inst.loader.display()
        )));
    }
    let dir = store.dir(id)?;
    let lock = PackLock::load(&dir)?;
    let base: BTreeMap<&str, &str> = lock
        .files
        .iter()
        .filter(|f| f.layer == Layer::Base)
        .map(|f| (f.path.as_str(), f.sha1.as_str()))
        .collect();
    let new = client_files(&index, skip_optional);
    let mut plan = PackUpdatePlan {
        from: inst
            .pack
            .as_ref()
            .map(|p| p.version.clone())
            .unwrap_or_default(),
        to: index.version_id.clone(),
        add: Vec::new(),
        update: Vec::new(),
        remove: Vec::new(),
        kept_mods: lock
            .files
            .iter()
            .filter(|f| f.layer == Layer::User)
            .map(|f| f.path.clone())
            .collect(),
        kept_configs: Vec::new(),
        merged: Vec::new(),
        optional: index
            .files
            .iter()
            .filter(|f| client_env(f) == Some("optional"))
            .map(|f| f.path.clone())
            .collect(),
        game_change: (game != inst.game_version || loader_version != inst.loader_version).then(
            || {
                format!(
                    "Minecraft {game}, {} {}",
                    loader.display(),
                    loader_version.clone().unwrap_or_default()
                )
            },
        ),
    };
    for f in &new {
        match base.get(f.path.as_str()) {
            None => plan.add.push(f.path.clone()),
            Some(sha)
                if !sha.eq_ignore_ascii_case(
                    f.hashes.get("sha1").map(String::as_str).unwrap_or(""),
                ) =>
            {
                plan.update.push(f.path.clone())
            }
            _ => {}
        }
    }
    plan.remove = base
        .keys()
        .filter(|p| !new.iter().any(|f| f.path == **p))
        .map(|p| p.to_string())
        .collect();
    for name in zip_names(path)? {
        if let Some(rel) = name
            .strip_prefix("overrides/")
            .or_else(|| name.strip_prefix("client-overrides/"))
            && crate::configs::is_user_edited(&dir, rel)
            && !plan
                .kept_configs
                .iter()
                .chain(&plan.merged)
                .any(|k| k == rel)
        {
            let game_dir = store.game_dir(id)?;
            if merged_config(path, &dir, &game_dir, rel).is_some() {
                plan.merged.push(rel.to_string());
            } else {
                plan.kept_configs.push(rel.to_string());
            }
        }
    }
    for v in [
        &mut plan.add,
        &mut plan.update,
        &mut plan.remove,
        &mut plan.kept_mods,
        &mut plan.kept_configs,
        &mut plan.merged,
    ] {
        v.sort();
    }
    Ok((plan, index))
}

pub fn plan_pack_update(store: &InstanceStore, id: &str, path: &Path) -> Result<PackUpdatePlan> {
    Ok(plan_with(store, id, path, &[])?.0)
}

/// Обновить сборку: снимок, удаление ушедших файлов сборки, загрузка новых и изменённых,
/// распаковка overrides без перезаписи конфигов игрока.
pub async fn apply_pack_update(
    store: &InstanceStore,
    id: &str,
    path: &Path,
    skip_optional: &[String],
    version_id: Option<&str>,
    cancel: &Cancel,
) -> Result<crate::snapshot::SnapshotInfo> {
    let (plan, index) = plan_with(store, id, path, skip_optional)?;
    let dir = store.dir(id)?;
    let game_dir = store.game_dir(id)?;
    let snap = crate::snapshot::create(
        &dir,
        &game_dir,
        &format!("Перед обновлением сборки до {}", plan.to),
        true,
    )?;
    let mut lock = PackLock::load(&dir)?;
    let mut tasks = Vec::new();
    let mut entries = Vec::new();
    for f in client_files(&index, skip_optional) {
        if plan.add.contains(&f.path) || plan.update.contains(&f.path) {
            let (task, entry) = file_entry(f, &game_dir)?;
            tasks.push(task);
            entries.push(entry);
        }
    }
    download::download_all(tasks, 6, Verify::Full, None, cancel).await?;
    for rel in &plan.remove {
        if let Some(p) = safe_join(&game_dir, rel) {
            for candidate in [
                p.clone(),
                PathBuf::from(format!("{}.disabled", p.display())),
            ] {
                if candidate.is_file() {
                    std::fs::remove_file(&candidate).at(&candidate)?;
                }
            }
        }
        lock.remove(rel);
    }
    for e in entries {
        lock.upsert(e);
    }
    let ours = |rel: &str| {
        !plan
            .kept_configs
            .iter()
            .chain(&plan.merged)
            .any(|k| k == rel)
    };
    extract_prefix(path, "overrides/", &game_dir, ours)?;
    extract_prefix(path, "client-overrides/", &game_dir, ours)?;
    for rel in &plan.merged {
        if let (Some(text), Some(target)) = (
            merged_config(path, &dir, &game_dir, rel),
            safe_join(&game_dir, rel),
        ) {
            std::fs::write(&target, text).at(&target)?;
        }
    }
    save_pack_base(path, &dir)?;
    lock.save(&dir)?;
    let mut inst = store.get(id)?;
    let (game, _, loader_version) = loader_from_deps(&index.dependencies)?;
    inst.game_version = game;
    inst.loader_version = loader_version;
    let mut pack = inst.pack.take().unwrap_or_default();
    pack.name = index.name.clone();
    pack.version = plan.to.clone();
    pack.version_id = version_id.map(str::to_string);
    inst.pack = Some(pack);
    store.save(&inst)?;
    crate::mods::manage::journal_append(
        &dir,
        &crate::mods::manage::JournalEntry {
            at: now(),
            action: "Обновлено".into(),
            what: format!("сборка {} {} → {}", index.name, plan.from, plan.to),
            reason: format!(
                "добавлено {}, обновлено {}, убрано {}; свои моды и конфиги сохранены",
                plan.add.len(),
                plan.update.len(),
                plan.remove.len()
            ),
            snapshot: Some(snap.id.clone()),
        },
    )?;
    Ok(snap)
}

// ---------- Сборки из каталога Modrinth ----------

/// Скачать .mrpack версии сборки во временную папку (только CDN Modrinth, с проверкой SHA-1).
/// `version_id = None` — новейшая версия по каналу `channel`.
pub async fn download_pack(
    data: &DataDir,
    project_id: &str,
    version_id: Option<&str>,
    channel: crate::catalog::Channel,
    cancel: &Cancel,
) -> Result<(PathBuf, crate::catalog::Version)> {
    let v = match version_id {
        Some(id) => crate::catalog::version(id).await?,
        None => {
            let list = crate::catalog::project_versions(project_id).await?;
            crate::catalog::pick(&list, channel)
                .cloned()
                .ok_or_else(|| {
                    Error::invalid("У сборки нет подходящей версии в выбранном канале")
                })?
        }
    };
    let f = v
        .primary_file()
        .ok_or_else(|| Error::invalid("У версии сборки нет файла"))?;
    if !f.url.starts_with("https://")
        || crate::net::host_of(&f.url) != "cdn.modrinth.com"
        || !f.filename.ends_with(".mrpack")
    {
        return Err(Error::invalid("Файл сборки не с CDN Modrinth — отменено"));
    }
    let sha1 = f
        .hashes
        .get("sha1")
        .ok_or_else(|| Error::invalid("Modrinth не прислал хэш файла"))?;
    std::fs::create_dir_all(data.tmp()).at(data.tmp())?;
    let dest = data.tmp().join(format!(
        "pack-{}.mrpack",
        v.id.replace(|c: char| !c.is_ascii_alphanumeric(), "")
    ));
    download::download_all(
        vec![Task {
            url: f.url.clone(),
            dest: dest.clone(),
            expected: Expected::Sha1(sha1.to_ascii_lowercase()),
            size: Some(f.size),
        }],
        1,
        Verify::Full,
        None,
        cancel,
    )
    .await?;
    Ok((dest, v))
}

/// Новая версия сборки для профиля по его каналу; `None` — обновлений нет или версия закреплена.
pub async fn pack_update_check(
    store: &InstanceStore,
    id: &str,
) -> Result<Option<crate::catalog::Version>> {
    let Some(pack) = store.get(id)?.pack.filter(|p| !p.pinned) else {
        return Ok(None);
    };
    let Some(project) = pack.project_id.as_deref() else {
        return Ok(None);
    };
    let list = crate::catalog::project_versions(project).await?;
    Ok(crate::catalog::pick(&list, pack.channel)
        .filter(|v| pack.version_id.as_deref() != Some(v.id.as_str()))
        .cloned())
}

// ---------- Экспорт .mrpack ----------

#[derive(Debug, Clone, Serialize)]
pub struct ExportedMod {
    pub title: String,
    pub version: String,
    pub project_id: Option<String>,
    pub path: String,
}

/// Итог экспорта: сколько модов ссылками на Modrinth и какие файлы вложены в архив.
#[derive(Debug, Clone, Serialize)]
pub struct ExportReport {
    pub linked: usize,
    pub bundled: Vec<String>,
    pub mods: Vec<ExportedMod>,
}

/// Сохранить профиль как .mrpack: моды с Modrinth — ссылками, остальное и config/ — в
/// overrides. Миры и личные настройки (options.txt) не попадают.
pub fn export_mrpack(
    store: &InstanceStore,
    id: &str,
    out: &Path,
    name: &str,
    version: &str,
) -> Result<ExportReport> {
    use sha1::Sha1;
    use sha2::Sha512;
    use std::io::Write;
    let inst = store.get(id)?;
    let game_dir = store.game_dir(id)?;
    let lock = PackLock::load(&store.dir(id)?)?;
    let loader_key = match inst.loader {
        LoaderKind::Fabric => "fabric-loader",
        LoaderKind::Quilt => "quilt-loader",
        LoaderKind::NeoForge => "neoforge",
        LoaderKind::Forge => "forge",
        LoaderKind::Vanilla => "",
    };
    let mut deps = serde_json::Map::new();
    deps.insert("minecraft".into(), inst.game_version.clone().into());
    if !loader_key.is_empty() {
        let v = inst.loader_version.clone().ok_or_else(|| {
            Error::invalid("Версия загрузчика ещё неизвестна — запустите профиль один раз")
        })?;
        deps.insert(loader_key.into(), v.into());
    }
    let mut report = ExportReport {
        linked: 0,
        bundled: Vec::new(),
        mods: Vec::new(),
    };
    let mut files = Vec::new();
    let mut names: Vec<String> = std::fs::read_dir(game_dir.join("mods"))
        .map(|rd| {
            rd.flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| n.ends_with(".jar"))
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    for file in names {
        let rel = format!("mods/{file}");
        let full = game_dir.join(&rel);
        let entry = lock.files.iter().find(|e| e.path == rel);
        report.mods.push(ExportedMod {
            title: entry
                .and_then(|e| e.title.clone())
                .unwrap_or_else(|| file.clone()),
            version: entry
                .and_then(|e| e.version_number.clone())
                .unwrap_or_default(),
            project_id: entry.and_then(|e| e.project_id.clone()),
            path: rel.clone(),
        });
        match entry.and_then(|e| Some((e.project_id.as_ref()?, e.version_id.as_ref()?))) {
            Some((p, v)) => {
                files.push(serde_json::json!({
                    "path": rel,
                    "hashes": {
                        "sha1": download::hash_file::<Sha1>(&full)?,
                        "sha512": download::hash_file::<Sha512>(&full)?,
                    },
                    "downloads": [format!("https://cdn.modrinth.com/data/{p}/versions/{v}/{}", urlencoding::encode(&file))],
                    "fileSize": std::fs::metadata(&full).map(|m| m.len()).unwrap_or(0),
                }));
                report.linked += 1;
            }
            None => report.bundled.push(rel),
        }
    }
    let index = serde_json::json!({
        "formatVersion": 1, "game": "minecraft", "versionId": version, "name": name,
        "files": files, "dependencies": deps,
    });
    let f = std::fs::File::create(out).at(out)?;
    let mut z = zip::ZipWriter::new(f);
    let opts = zip::write::SimpleFileOptions::default();
    let zerr = |e: zip::result::ZipError| Error::parse("архив сборки", e);
    z.start_file("modrinth.index.json", opts).map_err(zerr)?;
    z.write_all(
        serde_json::to_string_pretty(&index)
            .unwrap_or_default()
            .as_bytes(),
    )
    .at(out)?;
    let mut extra: Vec<PathBuf> = report.bundled.iter().map(|r| game_dir.join(r)).collect();
    let mut stack = vec![game_dir.join("config")];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            if e.path().is_dir() {
                stack.push(e.path())
            } else {
                extra.push(e.path())
            }
        }
    }
    for p in extra {
        let rel = p
            .strip_prefix(&game_dir)
            .unwrap_or(&p)
            .components()
            .map(|c| c.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/");
        z.start_file(format!("overrides/{rel}"), opts)
            .map_err(zerr)?;
        z.write_all(&std::fs::read(&p).at(&p)?).at(out)?;
    }
    z.finish().map_err(zerr)?;
    Ok(report)
}

/// Отчёт для автора сборки (Markdown): моды, источники, лицензии и что проверить перед
/// распространением. `licenses` — ID проекта Modrinth → лицензия.
pub fn modpack_report(
    name: &str,
    version: &str,
    r: &ExportReport,
    licenses: &BTreeMap<String, String>,
) -> String {
    let mut md = format!(
        "# Сборка «{name}» {version}\n\nМодов: {}. Ссылками на Modrinth: {}. Вложено в архив: {}.\n\n",
        r.mods.len(),
        r.linked,
        r.bundled.len()
    );
    md.push_str("| Мод | Версия | Источник | Лицензия |\n|---|---|---|---|\n");
    for m in &r.mods {
        let (src, lic) = match &m.project_id {
            Some(p) => (
                format!("[Modrinth](https://modrinth.com/project/{p})"),
                licenses.get(p).cloned().unwrap_or_else(|| "?".into()),
            ),
            None => ("неизвестен".to_string(), "проверьте".to_string()),
        };
        md.push_str(&format!(
            "| {} | {} | {src} | {lic} |\n",
            m.title,
            if m.version.is_empty() {
                "—"
            } else {
                &m.version
            }
        ));
    }
    if !r.bundled.is_empty() {
        md.push_str("\n## Файлы внутри архива\n\nЭти файлы распространяются вместе со сборкой. Перед публикацией проверьте лицензию каждого и разрешение автора:\n\n");
        for b in &r.bundled {
            md.push_str(&format!("- `{b}`\n"));
        }
    }
    md
}

// ---------- CurseForge ----------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CfManifest {
    minecraft: CfMinecraft,
    #[serde(default)]
    name: String,
    #[serde(default)]
    version: String,
    #[serde(default)]
    files: Vec<CfFile>,
    #[serde(default = "default_overrides")]
    overrides: String,
}

fn default_overrides() -> String {
    "overrides".into()
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CfMinecraft {
    version: String,
    #[serde(default)]
    mod_loaders: Vec<CfLoader>,
}

#[derive(Debug, Deserialize)]
struct CfLoader {
    id: String,
    #[serde(default)]
    primary: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CfFile {
    #[serde(rename = "projectID")]
    project_id: u64,
    #[serde(rename = "fileID")]
    file_id: u64,
    #[serde(default = "yes")]
    required: bool,
}

fn yes() -> bool {
    true
}

/// `fabric-0.16.10` → (Fabric, 0.16.10).
pub fn parse_cf_loader(id: &str) -> Option<(LoaderKind, String)> {
    let (kind, ver) = id.split_once('-')?;
    let k = match kind {
        "fabric" => LoaderKind::Fabric,
        "quilt" => LoaderKind::Quilt,
        "neoforge" => LoaderKind::NeoForge,
        "forge" => LoaderKind::Forge,
        _ => return None,
    };
    Some((k, ver.to_string()))
}

pub fn import_curseforge(store: &InstanceStore, path: &Path) -> Result<ImportResult> {
    let m: CfManifest = serde_json::from_str(&read_zip_entry(path, "manifest.json")?)
        .map_err(|e| Error::parse("manifest.json CurseForge", e))?;
    let (loader, loader_version) = m
        .minecraft
        .mod_loaders
        .iter()
        .find(|l| l.primary)
        .or(m.minecraft.mod_loaders.first())
        .and_then(|l| parse_cf_loader(&l.id))
        .map(|(k, v)| (k, Some(v)))
        .unwrap_or((LoaderKind::Vanilla, None));
    if !loader.is_supported() {
        return Err(Error::invalid(format!(
            "Сборка на {} — этот загрузчик пока не поддерживается",
            loader.display()
        )));
    }
    let name = if m.version.is_empty() {
        m.name.clone()
    } else {
        format!("{} {}", m.name, m.version)
    };
    let inst = store.create(
        if name.trim().is_empty() {
            "CurseForge"
        } else {
            &name
        },
        &m.minecraft.version,
        loader,
        loader_version,
    )?;
    let id = inst.id.clone();
    let r = (|| -> Result<ImportResult> {
        let game_dir = store.game_dir(&id)?;
        let prefix = format!("{}/", m.overrides.trim_end_matches('/'));
        let n = extract_prefix(path, &prefix, &game_dir, |_| true)?;
        mark_imported(&store.dir(&id)?, &game_dir)?;
        let manual: Vec<ManualFile> = m
            .files
            .iter()
            .filter(|f| f.required)
            .map(|f| ManualFile {
                project_id: f.project_id,
                file_id: f.file_id,
                url: format!("https://www.curseforge.com/projects/{}", f.project_id),
            })
            .collect();
        write_atomic(
            &store.dir(&id)?.join("curseforge-manual.json"),
            &serde_json::to_vec_pretty(&manual).unwrap_or_default(),
        )?;
        Ok(ImportResult {
            instance: inst.clone(),
            notes: vec![
                format!("Из архива: {n} файлов (конфиги и прочее)"),
                format!(
                    "Модов CurseForge для ручной загрузки: {} — без API-ключа CurseForge лаунчер не может скачать их сам",
                    manual.len()
                ),
            ],
            manual,
        })
    })();
    cleanup_on_error(store, &id, r)
}

// ---------- Существующая .minecraft ----------

/// Подключить существующую папку игры без копирования: профиль будет работать прямо в ней.
pub fn link_existing(
    store: &InstanceStore,
    game_dir: &Path,
    name: &str,
    game_version: &str,
    loader: LoaderKind,
    loader_version: Option<String>,
) -> Result<ImportResult> {
    let game_dir = std::fs::canonicalize(game_dir).at(game_dir)?;
    if !game_dir.is_dir() {
        return Err(Error::invalid("Это не папка"));
    }
    let data_root = std::fs::canonicalize(store.data().root())
        .unwrap_or_else(|_| store.data().root().to_path_buf());
    if game_dir.starts_with(&data_root) {
        return Err(Error::invalid("Эта папка уже принадлежит Lime"));
    }
    let mut inst = store.create(name, game_version, loader, loader_version)?;
    let id = inst.id.clone();
    let r = (|| -> Result<ImportResult> {
        // Пустая папка game/ внутри профиля не нужна.
        let own = store.dir(&id)?.join("game");
        let _ = std::fs::remove_dir(&own);
        inst.external_game_dir = Some(game_dir.clone());
        store.save(&inst)?;
        Ok(ImportResult {
            instance: inst.clone(),
            manual: Vec::new(),
            notes: vec![format!(
                "Профиль работает прямо в {} — файлы не копировались",
                game_dir.display()
            )],
        })
    })();
    cleanup_on_error(store, &id, r)
}

/// Папка игры официального лаунчера на этой ОС.
pub fn official_minecraft_dir() -> Option<PathBuf> {
    let p = if cfg!(target_os = "macos") {
        dirs::data_dir()?.join("minecraft")
    } else if cfg!(windows) {
        dirs::config_dir()?.join(".minecraft")
    } else {
        dirs::home_dir()?.join(".minecraft")
    };
    p.is_dir().then_some(p)
}

pub fn store_for(data: &DataDir) -> InstanceStore {
    InstanceStore::new(data.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn store(name: &str) -> (InstanceStore, PathBuf) {
        let root =
            std::env::temp_dir().join(format!("quarry-import-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        (
            InstanceStore::new(DataDir::open(root.join("data")).unwrap()),
            root,
        )
    }

    fn dir_fingerprint(p: &Path) -> Vec<(String, u64)> {
        let mut v = Vec::new();
        let mut stack = vec![p.to_path_buf()];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).unwrap().flatten() {
                let m = e.metadata().unwrap();
                if m.is_dir() {
                    stack.push(e.path());
                } else {
                    v.push((e.path().display().to_string(), m.len()));
                }
            }
        }
        v.sort();
        v
    }

    #[test]
    fn prism_instance_is_copied_and_source_untouched() {
        let (s, root) = store("prism");
        let src = root.join("prism/Tech");
        std::fs::create_dir_all(src.join(".minecraft/mods")).unwrap();
        std::fs::create_dir_all(src.join(".minecraft/saves/World/region")).unwrap();
        std::fs::write(src.join(".minecraft/mods/a.jar"), b"jar").unwrap();
        std::fs::write(src.join(".minecraft/saves/World/level.dat"), b"lvl").unwrap();
        std::fs::write(src.join("instance.cfg"), "[General]\nname=Технология+\nOverrideMemory=true\nMaxMemAlloc=6144\nOverrideJavaArgs=true\nJvmArgs=-Xmx2G -XX:+UseG1GC -XX:OnOutOfMemoryError=calc -agentpath:/tmp/x.dylib\n").unwrap();
        std::fs::write(src.join("mmc-pack.json"), r#"{"components":[{"uid":"net.minecraft","version":"1.21.11"},{"uid":"net.fabricmc.fabric-loader","version":"0.17.2"}],"formatVersion":1}"#).unwrap();
        let before = dir_fingerprint(&src);
        let r = import_mmc(&s, &src).unwrap();
        assert_eq!(r.instance.name, "Технология+");
        assert_eq!(r.instance.loader, LoaderKind::Fabric);
        assert_eq!(r.instance.settings.max_memory_mb, Some(6144));
        assert_eq!(
            r.instance.settings.jvm_args.as_deref(),
            Some(&["-XX:+UseG1GC".to_string()][..]),
            "-Xmx и опасные аргументы (команда при ошибке, нативный агент) отброшены"
        );
        let game = s.game_dir(&r.instance.id).unwrap();
        assert!(game.join("saves/World/level.dat").exists());
        assert_eq!(dir_fingerprint(&src), before, "источник не изменился");
        let lock = PackLock::load(&s.dir(&r.instance.id).unwrap()).unwrap();
        assert_eq!(lock.files[0].source, Source::Import);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn failed_import_leaves_nothing_behind() {
        let (s, root) = store("fail");
        let src = root.join("forge");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("instance.cfg"), "name=Old Forge\n").unwrap();
        std::fs::write(src.join("mmc-pack.json"), r#"{"components":[{"uid":"net.minecraft","version":"1.12.2"},{"uid":"net.minecraftforge","version":"14.23"}]}"#).unwrap();
        assert!(import_mmc(&s, &src).is_err());
        assert!(s.list().unwrap().0.is_empty());
        std::fs::remove_dir_all(&root).unwrap();
    }

    fn zip_with(path: &Path, files: &[(&str, &[u8])]) {
        let mut w = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
        for (n, d) in files {
            w.start_file(*n, zip::write::SimpleFileOptions::default())
                .unwrap();
            w.write_all(d).unwrap();
        }
        w.finish().unwrap();
    }

    #[test]
    fn curseforge_zip_gives_manual_list() {
        let (s, root) = store("cf");
        std::fs::create_dir_all(&root).unwrap();
        let z = root.join("pack.zip");
        zip_with(&z, &[
            ("manifest.json", br#"{"minecraft":{"version":"1.21.1","modLoaders":[{"id":"neoforge-21.1.77","primary":true}]},"manifestType":"minecraftModpack","name":"ATM","version":"1.0","files":[{"projectID":238222,"fileID":5000000,"required":true},{"projectID":1,"fileID":2,"required":false}],"overrides":"overrides"}"#),
            ("overrides/config/jei.toml", b"x=1"),
            ("overrides/../escape.txt", b"bad"),
        ]);
        // Архив с путём наружу отклоняется целиком.
        assert!(import_curseforge(&s, &z).is_err());
        assert!(s.list().unwrap().0.is_empty(), "недоделанный профиль убран");
        zip_with(&z, &[
            ("manifest.json", br#"{"minecraft":{"version":"1.21.1","modLoaders":[{"id":"neoforge-21.1.77","primary":true}]},"name":"ATM","version":"1.0","files":[{"projectID":238222,"fileID":5000000,"required":true}],"overrides":"overrides"}"#),
            ("overrides/config/jei.toml", b"x=1"),
        ]);
        let r = import_curseforge(&s, &z).unwrap();
        assert_eq!(r.instance.loader, LoaderKind::NeoForge);
        assert_eq!(r.instance.loader_version.as_deref(), Some("21.1.77"));
        assert_eq!(r.manual.len(), 1);
        assert!(
            s.game_dir(&r.instance.id)
                .unwrap()
                .join("config/jei.toml")
                .exists()
        );
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn mrpack_validation() {
        assert!(allowed_download(
            "https://cdn.modrinth.com/data/AANobbMI/versions/x/sodium.jar"
        ));
        assert!(!allowed_download("http://cdn.modrinth.com/x.jar"));
        assert!(!allowed_download("https://evil.example.com/x.jar"));
        let deps: BTreeMap<String, String> = [
            ("minecraft".to_string(), "1.21.11".to_string()),
            ("fabric-loader".to_string(), "0.17.2".to_string()),
        ]
        .into();
        assert_eq!(loader_from_deps(&deps).unwrap().1, LoaderKind::Fabric);
        assert_eq!(
            parse_cf_loader("fabric-0.16.10"),
            Some((LoaderKind::Fabric, "0.16.10".into()))
        );
        assert_eq!(parse_cf_loader("rift-1"), None);
    }

    #[test]
    fn existing_folder_is_linked_not_copied() {
        let (s, root) = store("link");
        let mc = root.join("official/minecraft");
        std::fs::create_dir_all(mc.join("saves/W")).unwrap();
        let r = link_existing(
            &s,
            &mc,
            "Официальная папка",
            "26.3",
            LoaderKind::Vanilla,
            None,
        )
        .unwrap();
        let gd = s.game_dir(&r.instance.id).unwrap();
        assert_eq!(gd, std::fs::canonicalize(&mc).unwrap());
        // Корзина убирает только профиль, а не чужую папку.
        s.trash(&r.instance.id).unwrap();
        assert!(mc.join("saves/W").is_dir());
        std::fs::remove_dir_all(&root).unwrap();
    }

    fn mrpack(path: &Path, index: &str, overrides: &[(&str, &str)]) {
        let mut w = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
        let o = zip::write::SimpleFileOptions::default();
        w.start_file("modrinth.index.json", o).unwrap();
        w.write_all(index.as_bytes()).unwrap();
        for (n, body) in overrides {
            w.start_file(format!("overrides/{n}"), o).unwrap();
            w.write_all(body.as_bytes()).unwrap();
        }
        w.finish().unwrap();
    }
    fn mr_file(path: &str, sha1: &str, env: &str) -> String {
        format!(
            r#"{{"path":"{path}","hashes":{{"sha1":"{sha1}","sha512":"x"}},"env":{{"client":"{env}","server":"required"}},"downloads":["https://cdn.modrinth.com/data/P/versions/V/{path}"],"fileSize":1}}"#
        )
    }
    fn index(version: &str, files: &[String]) -> String {
        format!(
            r#"{{"formatVersion":1,"game":"minecraft","name":"Пак","versionId":"{version}","files":[{}],"dependencies":{{"minecraft":"26.3","fabric-loader":"0.19.5"}}}}"#,
            files.join(",")
        )
    }

    #[test]
    fn preview_lists_optional_components() {
        let (_s, root) = store("mrprev");
        std::fs::create_dir_all(&root).unwrap();
        let p = root.join("p.mrpack");
        mrpack(
            &p,
            &index(
                "1.0",
                &[
                    mr_file("mods/a.jar", "a1", "required"),
                    mr_file("mods/extra.jar", "e1", "optional"),
                ],
            ),
            &[],
        );
        let pv = preview_mrpack(&p).unwrap();
        assert_eq!(
            (pv.name.as_str(), pv.version.as_str(), pv.game.as_str()),
            ("Пак", "1.0", "26.3")
        );
        assert_eq!(pv.optional, ["mods/extra.jar"]);
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// Профиль «как после импорта версии 1.0»: моды сборки a, b; свой мод mine; свой конфиг x.
    fn installed_pack(name: &str) -> (InstanceStore, String, PathBuf) {
        let (store, root) = store(name);
        let mut inst = store
            .create("Пак 1.0", "26.3", LoaderKind::Fabric, Some("0.19.5".into()))
            .unwrap();
        inst.pack = Some(crate::instance::PackRef {
            name: "Пак".into(),
            version: "1.0".into(),
            project_id: Some("PACK".into()),
            ..Default::default()
        });
        store.save(&inst).unwrap();
        let game = store.game_dir(&inst.id).unwrap();
        std::fs::create_dir_all(game.join("mods")).unwrap();
        std::fs::create_dir_all(game.join("config")).unwrap();
        for f in ["a.jar", "b.jar", "mine.jar"] {
            std::fs::write(game.join("mods").join(f), b"jar").unwrap();
        }
        std::fs::write(game.join("config/x.toml"), "мой = 1").unwrap();
        let dir = store.dir(&inst.id).unwrap();
        let mut lock = PackLock::default();
        for (path, sha, layer) in [
            ("mods/a.jar", "a1", Layer::Base),
            ("mods/b.jar", "b1", Layer::Base),
            ("mods/mine.jar", "m1", Layer::User),
        ] {
            lock.upsert(LockEntry {
                path: path.into(),
                sha1: sha.into(),
                sha512: None,
                source: Source::Modrinth,
                project_id: None,
                version_id: None,
                version_number: None,
                title: None,
                layer,
                installed_at: 0,
                pinned: false,
            });
        }
        lock.save(&dir).unwrap();
        std::fs::write(
            dir.join("user-configs.toml"),
            "files = [\"config/x.toml\"]\n",
        )
        .unwrap();
        (store, inst.id, root)
    }

    #[test]
    fn pack_update_plan_keeps_user_layer() {
        let (store, id, root) = installed_pack("mrplan");
        let p = root.join("p2.mrpack");
        mrpack(
            &p,
            &index(
                "2.0",
                &[
                    mr_file("mods/a.jar", "a2", "required"),
                    mr_file("mods/c.jar", "c1", "required"),
                ],
            ),
            &[("config/x.toml", "сборка = 2"), ("config/y.toml", "y")],
        );
        let plan = plan_pack_update(&store, &id, &p).unwrap();
        assert_eq!((plan.from.as_str(), plan.to.as_str()), ("1.0", "2.0"));
        assert_eq!(plan.add, ["mods/c.jar"]);
        assert_eq!(plan.update, ["mods/a.jar"]);
        assert_eq!(plan.remove, ["mods/b.jar"]);
        assert_eq!(plan.kept_mods, ["mods/mine.jar"]);
        assert_eq!(plan.kept_configs, ["config/x.toml"]);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[tokio::test]
    async fn pack_update_apply_without_downloads() {
        let (store, id, root) = installed_pack("mrapply");
        let p = root.join("p2.mrpack");
        // Те же хэши у a — скачивать нечего; b убран; новые конфиги.
        mrpack(
            &p,
            &index("2.0", &[mr_file("mods/a.jar", "a1", "required")]),
            &[("config/x.toml", "сборка = 2"), ("config/y.toml", "y")],
        );
        apply_pack_update(&store, &id, &p, &[], Some("NEWVER"), &Cancel::new())
            .await
            .unwrap();
        let game = store.game_dir(&id).unwrap();
        assert!(!game.join("mods/b.jar").exists() && game.join("mods/a.jar").exists());
        assert!(game.join("mods/mine.jar").exists(), "свой мод остался");
        assert_eq!(
            std::fs::read_to_string(game.join("config/x.toml")).unwrap(),
            "мой = 1",
            "свой конфиг не перезаписан"
        );
        assert_eq!(
            std::fs::read_to_string(game.join("config/y.toml")).unwrap(),
            "y"
        );
        let pack = store.get(&id).unwrap().pack.unwrap();
        assert_eq!(
            (
                pack.version.as_str(),
                pack.version_id.as_deref(),
                pack.project_id.as_deref()
            ),
            ("2.0", Some("NEWVER"), Some("PACK"))
        );
        // Старый формат ссылки на сборку тоже читается.
        let old: crate::instance::PackRef =
            toml::from_str("name = \"Пак\"\nversion = \"1.0\"").unwrap();
        assert!(!old.pinned && old.project_id.is_none());
        assert!(
            !crate::snapshot::list(&store.dir(&id).unwrap())
                .unwrap()
                .is_empty(),
            "перед обновлением снимок"
        );
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[tokio::test]
    async fn pack_update_merges_player_config_with_author_changes() {
        let (store, id, root) = installed_pack("mrmerge");
        let (game, dir) = (store.game_dir(&id).unwrap(), store.dir(&id).unwrap());
        // Как было у автора в 1.0 и как поправил игрок.
        std::fs::create_dir_all(dir.join("pack-base/config")).unwrap();
        std::fs::write(dir.join("pack-base/config/x.toml"), "a = 1\nb = 2\n").unwrap();
        std::fs::write(game.join("config/x.toml"), "a = 1\nb = 20\n").unwrap();
        let p = root.join("p2.mrpack");
        mrpack(
            &p,
            &index("2.0", &[mr_file("mods/a.jar", "a1", "required")]),
            &[("config/x.toml", "a = 10\nb = 2\n")],
        );
        let plan = plan_pack_update(&store, &id, &p).unwrap();
        assert_eq!(plan.merged, ["config/x.toml"]);
        assert!(plan.kept_configs.is_empty());
        apply_pack_update(&store, &id, &p, &[], None, &Cancel::new())
            .await
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(game.join("config/x.toml")).unwrap(),
            "a = 10\nb = 20\n"
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("pack-base/config/x.toml")).unwrap(),
            "a = 10\nb = 2\n",
            "основа для следующего обновления — новая версия автора"
        );
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn export_mrpack_and_report() {
        let (store, id, root) = installed_pack("mrexport");
        let dir = store.dir(&id).unwrap();
        let mut lock = PackLock::load(&dir).unwrap();
        lock.upsert(LockEntry {
            path: "mods/a.jar".into(),
            sha1: "a1".into(),
            sha512: Some("s512".into()),
            source: Source::Modrinth,
            project_id: Some("AAA".into()),
            version_id: Some("VVV".into()),
            version_number: Some("1.2".into()),
            title: Some("Мод А".into()),
            layer: Layer::Base,
            installed_at: 0,
            pinned: false,
        });
        lock.save(&dir).unwrap();
        let out = root.join("export.mrpack");
        let r = export_mrpack(&store, &id, &out, "Моя сборка", "1.0").unwrap();
        // a.jar — с Modrinth (ссылкой), b.jar и mine.jar без проекта — в overrides.
        assert_eq!(r.linked, 1);
        assert_eq!(r.bundled, ["mods/b.jar", "mods/mine.jar"]);
        let index = read_index(&out).unwrap();
        assert_eq!(
            index.files[0].downloads[0],
            "https://cdn.modrinth.com/data/AAA/versions/VVV/a.jar"
        );
        assert_eq!(
            index.dependencies.get("fabric-loader").map(String::as_str),
            Some("0.19.5")
        );
        let names = zip_names(&out).unwrap();
        assert!(
            names.contains(&"overrides/config/x.toml".to_string())
                && names.contains(&"overrides/mods/b.jar".to_string())
        );
        let md = modpack_report(
            "Моя сборка",
            "1.0",
            &r,
            &[("AAA".into(), "MIT".into())].into(),
        );
        assert!(
            md.contains("Мод А")
                && md.contains("MIT")
                && md.contains("mine.jar")
                && md.contains("проверьте")
        );
        std::fs::remove_dir_all(&root).unwrap();
    }
}
