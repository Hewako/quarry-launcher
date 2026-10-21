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

/// Рекурсивно скопировать папку (символические ссылки пропускаются).
fn copy_dir(from: &Path, to: &Path) -> Result<u64> {
    let mut n = 0;
    std::fs::create_dir_all(to).at(to)?;
    for e in std::fs::read_dir(from).at(from)?.flatten() {
        let src = e.path();
        let dst = to.join(e.file_name());
        let ft = e.file_type().at(&src)?;
        if ft.is_dir() {
            n += copy_dir(&src, &dst)?;
        } else if ft.is_file() {
            std::fs::copy(&src, &dst).at(&dst)?;
            n += 1;
        }
    }
    Ok(n)
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
            let n = copy_dir(&src, &store.game_dir(&id)?)?;
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
                        .filter(|x| !x.starts_with("-Xm") && !x.starts_with("-javaagent"))
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
fn extract_prefix(zip_path: &Path, prefix: &str, dest: &Path) -> Result<u64> {
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
        if e.is_dir() || rel.is_empty() || e.is_symlink() {
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

pub async fn import_mrpack(
    store: &InstanceStore,
    path: &Path,
    cancel: &Cancel,
) -> Result<ImportResult> {
    let index: MrIndex = serde_json::from_str(&read_zip_entry(path, "modrinth.index.json")?)
        .map_err(|e| Error::parse("modrinth.index.json", e))?;
    if index.format_version != 1 || (index.game != "minecraft" && !index.game.is_empty()) {
        return Err(Error::invalid(format!(
            "Неподдерживаемый формат сборки (formatVersion {})",
            index.format_version
        )));
    }
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
    let inst = store.create(&name, &game, loader, loader_version)?;
    let id = inst.id.clone();
    let r = async {
        let game_dir = store.game_dir(&id)?;
        let mut tasks = Vec::new();
        let mut lock = PackLock::default();
        let mut skipped = 0;
        for f in &index.files {
            if f.env
                .as_ref()
                .and_then(|e| e.get("client"))
                .is_some_and(|c| c == "unsupported")
            {
                skipped += 1;
                continue;
            }
            let dest = safe_join(&game_dir, &f.path).ok_or_else(|| {
                Error::invalid(format!("Сборка содержит недопустимый путь: {}", f.path))
            })?;
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
            tasks.push(Task {
                url: url.clone(),
                dest,
                expected: Expected::Sha1(sha1.to_ascii_lowercase()),
                size: Some(f.file_size).filter(|s| *s > 0),
            });
            // project_id и version_id видны в адресе cdn.modrinth.com/data/<проект>/versions/<версия>/.
            let parts: Vec<&str> = url.split('/').collect();
            let (project_id, version_id) = match parts.iter().position(|p| *p == "data") {
                Some(i) if parts.get(i + 2) == Some(&"versions") => (
                    parts.get(i + 1).map(|s| s.to_string()),
                    parts.get(i + 3).map(|s| s.to_string()),
                ),
                _ => (None, None),
            };
            lock.upsert(LockEntry {
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
            });
        }
        download::download_all(tasks, 6, Verify::Full, None, cancel).await?;
        let mut overrides = extract_prefix(path, "overrides/", &game_dir)?;
        overrides += extract_prefix(path, "client-overrides/", &game_dir)?;
        lock.save(&store.dir(&id)?)?;
        let mut notes = vec![format!(
            "Файлов сборки: {}, из архива: {overrides}",
            index.files.len() - skipped
        )];
        if skipped > 0 {
            notes.push(format!("Пропущено файлов только для сервера: {skipped}"));
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
        let n = extract_prefix(path, &prefix, &game_dir)?;
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
        return Err(Error::invalid("Эта папка уже принадлежит Quarry"));
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
        std::fs::write(src.join("instance.cfg"), "[General]\nname=Технология+\nOverrideMemory=true\nMaxMemAlloc=6144\nOverrideJavaArgs=true\nJvmArgs=-Xmx2G -XX:+UseG1GC\n").unwrap();
        std::fs::write(src.join("mmc-pack.json"), r#"{"components":[{"uid":"net.minecraft","version":"1.21.11"},{"uid":"net.fabricmc.fabric-loader","version":"0.17.2"}],"formatVersion":1}"#).unwrap();
        let before = dir_fingerprint(&src);
        let r = import_mmc(&s, &src).unwrap();
        assert_eq!(r.instance.name, "Технология+");
        assert_eq!(r.instance.loader, LoaderKind::Fabric);
        assert_eq!(r.instance.settings.max_memory_mb, Some(6144));
        assert_eq!(
            r.instance.settings.jvm_args.as_deref(),
            Some(&["-XX:+UseG1GC".to_string()][..]),
            "-Xmx отфильтрован"
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
}
