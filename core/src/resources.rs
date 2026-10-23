//! Ресурспаки и шейдеры профиля: список, совместимость по pack_format, порядок и включение
//! (options.txt), выбранный шейдер Iris (config/iris.properties), установка с Modrinth.

use crate::download::{self, Cancel, Expected, Task, Verify};
use crate::error::{Error, IoContext, Result};
use crate::instance::{InstanceStore, now};
use crate::mods::lock::{Layer, LockEntry, PackLock, Source};
use crate::paths::write_atomic;
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Resourcepack,
    Shader,
    /// Датапаки лежат внутри мира: `saves/<мир>/datapacks`.
    Datapack,
}

#[derive(Debug, Clone, Serialize)]
pub struct PackInfo {
    pub file_name: String,
    pub description: String,
    pub size: u64,
    pub is_dir: bool,
    /// Ресурспак включён (для шейдеров — выбран в Iris).
    pub enabled: bool,
    /// Диапазон pack_format, для которого сделан ресурспак.
    pub formats: Option<(i64, i64)>,
    /// Подходит ли к версии игры; `None` — неизвестно (игра ещё не скачана).
    pub compatible: Option<bool>,
    /// Картинка пака (pack.png) как `data:`-адрес.
    pub icon: Option<String>,
}

impl Kind {
    pub fn folder(self) -> &'static str {
        match self {
            Kind::Resourcepack => "resourcepacks",
            Kind::Shader => "shaderpacks",
            Kind::Datapack => "datapacks",
        }
    }
}

/// Папка для файлов этого вида. Для датапаков нужен мир (имя папки в `saves/`).
pub fn target_dir(game_dir: &Path, kind: Kind, world: Option<&str>) -> Result<PathBuf> {
    match (kind, world) {
        (Kind::Datapack, Some(w)) => {
            let bad = w.is_empty() || w.starts_with('.') || w.contains(['/', '\\']);
            let dir = game_dir.join("saves").join(w);
            if bad || !dir.join("level.dat").is_file() {
                return Err(Error::invalid(format!("Нет такого мира: {w}")));
            }
            Ok(dir.join(kind.folder()))
        }
        (Kind::Datapack, None) => Err(Error::invalid("Для датапака нужно выбрать мир")),
        _ => Ok(game_dir.join(kind.folder())),
    }
}

/// Диапазон pack_format из pack.mcmeta во всех форматах разных версий игры.
pub fn pack_formats(mcmeta: &serde_json::Value) -> Option<(i64, i64)> {
    let p = mcmeta.get("pack")?;
    // С 1.21.9 версия — число или [старшая, младшая]; совместимость решает старшая.
    let major = |v: &serde_json::Value| v.as_i64().or_else(|| v.get(0)?.as_i64());
    if let (Some(a), Some(b)) = (
        p.get("min_format").and_then(major),
        p.get("max_format").and_then(major),
    ) {
        return Some((a, b));
    }
    if let Some(sf) = p.get("supported_formats") {
        if let Some(n) = sf.as_i64() {
            return Some((n, n));
        }
        let pair = |a: Option<&serde_json::Value>, b: Option<&serde_json::Value>| {
            Some((a?.as_i64()?, b?.as_i64()?))
        };
        if let Some(r) = pair(sf.get(0), sf.get(1))
            .or_else(|| pair(sf.get("min_inclusive"), sf.get("max_inclusive")))
        {
            return Some(r);
        }
    }
    let n = p.get("pack_format")?.as_i64()?;
    Some((n, n))
}

/// pack.png из ресурспака или датапака (не больше 256 КБ).
fn read_icon(path: &Path) -> Option<String> {
    use base64::Engine as _;
    const MAX: u64 = 256 * 1024;
    let mut buf = Vec::new();
    if path.is_dir() {
        let p = path.join("pack.png");
        if std::fs::metadata(&p).ok()?.len() > MAX {
            return None;
        }
        buf = std::fs::read(p).ok()?;
    } else {
        let mut zip = zip::ZipArchive::new(std::fs::File::open(path).ok()?).ok()?;
        let e = zip.by_name("pack.png").ok()?;
        if e.size() > MAX {
            return None;
        }
        e.take(MAX).read_to_end(&mut buf).ok()?;
    }
    Some(format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(buf)
    ))
}

fn read_mcmeta(path: &Path) -> Option<serde_json::Value> {
    let text = if path.is_dir() {
        std::fs::read_to_string(path.join("pack.mcmeta")).ok()?
    } else {
        let mut zip = zip::ZipArchive::new(std::fs::File::open(path).ok()?).ok()?;
        let mut s = String::new();
        zip.by_name("pack.mcmeta")
            .ok()?
            .take(1024 * 1024)
            .read_to_string(&mut s)
            .ok()?;
        s
    };
    // Встречается BOM в начале файла.
    serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()
}

/// Значение ключа из файла `ключ:значение` (options.txt) или `ключ=значение` (.properties).
fn get_key<'a>(text: &'a str, key: &str, sep: char) -> Option<&'a str> {
    text.lines()
        .find_map(|l| l.strip_prefix(key)?.strip_prefix(sep))
}

fn set_keys(text: &str, sep: char, values: &[(&str, String)]) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut done = vec![false; values.len()];
    for line in text.lines() {
        match values
            .iter()
            .position(|(k, _)| line.strip_prefix(k).is_some_and(|r| r.starts_with(sep)))
        {
            Some(i) => {
                out.push(format!("{}{sep}{}", values[i].0, values[i].1));
                done[i] = true;
            }
            None => out.push(line.to_string()),
        }
    }
    for (i, (k, v)) in values.iter().enumerate() {
        if !done[i] {
            out.push(format!("{k}{sep}{v}"));
        }
    }
    out.join("\n") + "\n"
}

fn json_list(v: Option<&str>) -> Vec<String> {
    v.and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default()
}

/// Включённые ресурспаки, главный первым (в options.txt — наоборот, главный последним).
fn enabled_packs(game_dir: &Path) -> Vec<String> {
    let text = std::fs::read_to_string(game_dir.join("options.txt")).unwrap_or_default();
    json_list(get_key(&text, "resourcePacks", ':'))
        .into_iter()
        .rev()
        .filter_map(|p| p.strip_prefix("file/").map(str::to_string))
        .collect()
}

const SHADER_CONFIGS: [&str; 2] = ["config/iris.properties", "config/oculus.properties"];

/// Выбранный шейдер Iris/Oculus; `None` — шейдеры выключены.
pub fn active_shader(game_dir: &Path) -> Option<String> {
    SHADER_CONFIGS.iter().find_map(|c| {
        let text = std::fs::read_to_string(game_dir.join(c)).ok()?;
        if get_key(&text, "enableShaders", '=') == Some("false") {
            return None;
        }
        get_key(&text, "shaderPack", '=')
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    })
}

pub fn list(game_dir: &Path, kind: Kind, game_format: Option<i64>) -> Vec<PackInfo> {
    let order = match kind {
        Kind::Resourcepack => enabled_packs(game_dir),
        Kind::Shader => active_shader(game_dir).into_iter().collect(),
        // Включение датапаков хранится в level.dat; новые игра включает сама.
        Kind::Datapack => Vec::new(),
    };
    let Ok(rd) = std::fs::read_dir(game_dir.join(kind.folder())) else {
        return Vec::new();
    };
    let mut out: Vec<PackInfo> = rd
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            let file_name = e.file_name().to_string_lossy().into_owned();
            let is_dir = path.is_dir();
            if !is_dir && !file_name.to_lowercase().ends_with(".zip") {
                return None;
            }
            let has_meta = kind != Kind::Shader;
            let meta = has_meta.then(|| read_mcmeta(&path)).flatten();
            if has_meta && is_dir && meta.is_none() {
                return None; // папка без pack.mcmeta — не ресурспак
            }
            let formats = meta.as_ref().and_then(pack_formats);
            Some(PackInfo {
                icon: has_meta.then(|| read_icon(&path)).flatten(),
                description: meta
                    .as_ref()
                    .map(|m| crate::servers::plain_text(&m["pack"]["description"]))
                    .unwrap_or_default(),
                size: if is_dir {
                    crate::paths::dir_size(&path)
                } else {
                    e.metadata().map(|m| m.len()).unwrap_or(0)
                },
                is_dir,
                enabled: order.contains(&file_name),
                compatible: match (formats, game_format) {
                    (Some((a, b)), Some(g)) => Some((a..=b).contains(&g)),
                    _ => None,
                },
                formats,
                file_name,
            })
        })
        .collect();
    out.sort_by_key(|p| {
        (
            order
                .iter()
                .position(|o| *o == p.file_name)
                .unwrap_or(usize::MAX),
            p.file_name.to_lowercase(),
        )
    });
    out
}

fn check_name(dir: &Path, name: &str) -> Result<()> {
    if !crate::paths::is_plain_file_name(name) || !dir.join(name).exists() {
        return Err(Error::invalid(format!("Нет такого файла: {name}")));
    }
    Ok(())
}

/// Включить ресурспаки в заданном порядке (главный первым). Встроенные паки игры и модов
/// (`vanilla`, `fabric` …) остаются. Несовместимые записываются в incompatibleResourcePacks —
/// иначе игра сама выключит их при запуске.
pub fn set_enabled(game_dir: &Path, enabled: &[String], game_format: Option<i64>) -> Result<()> {
    let dir = game_dir.join(Kind::Resourcepack.folder());
    for n in enabled {
        check_name(&dir, n)?;
    }
    let path = game_dir.join("options.txt");
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(Error::io(&path, e)),
    };
    let not_file = |v: Vec<String>| -> Vec<String> {
        v.into_iter().filter(|p| !p.starts_with("file/")).collect()
    };
    let mut packs = not_file(json_list(get_key(&text, "resourcePacks", ':')));
    if packs.is_empty() {
        packs.push("vanilla".into());
    }
    packs.extend(enabled.iter().rev().map(|n| format!("file/{n}")));
    let info = list(game_dir, Kind::Resourcepack, game_format);
    let mut incompatible = not_file(json_list(get_key(&text, "incompatibleResourcePacks", ':')));
    incompatible.extend(
        enabled
            .iter()
            .filter(|n| {
                info.iter()
                    .any(|p| &p.file_name == *n && p.compatible == Some(false))
            })
            .map(|n| format!("file/{n}")),
    );
    let json = |v: &Vec<String>| serde_json::to_string(v).unwrap_or_else(|_| "[]".into());
    let text = set_keys(
        &text,
        ':',
        &[
            ("resourcePacks", json(&packs)),
            ("incompatibleResourcePacks", json(&incompatible)),
        ],
    );
    write_atomic(&path, text.as_bytes())
}

/// Выбрать шейдер для Iris/Oculus или выключить шейдеры (`None`).
pub fn set_shader(game_dir: &Path, shader: Option<&str>) -> Result<()> {
    if let Some(s) = shader {
        check_name(&game_dir.join(Kind::Shader.folder()), s)?;
    }
    let mut values = vec![("enableShaders", shader.is_some().to_string())];
    if let Some(s) = shader {
        values.push(("shaderPack", s.to_string()));
    }
    for (i, c) in SHADER_CONFIGS.iter().enumerate() {
        let path = game_dir.join(c);
        // Конфиг Oculus трогаем, только если он уже есть; Iris — всегда.
        if i > 0 && !path.is_file() {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p).at(p)?;
        }
        write_atomic(&path, set_keys(&text, '=', &values).as_bytes())?;
    }
    Ok(())
}

/// Убрать файл ресурспака или шейдера в корзину Quarry (не удаляя насовсем).
pub fn remove(
    store: &InstanceStore,
    id: &str,
    kind: Kind,
    world: Option<&str>,
    file_name: &str,
) -> Result<()> {
    let game = store.game_dir(id)?;
    let dir = target_dir(&game, kind, world)?;
    check_name(&dir, file_name)?;
    let trash = store
        .data()
        .trash()
        .join(format!("{id}-{}-{}", kind.folder(), now()));
    std::fs::create_dir_all(&trash).at(&trash)?;
    let from = dir.join(file_name);
    std::fs::rename(&from, trash.join(file_name)).at(&from)?;
    let mut lock = PackLock::load(&store.dir(id)?)?;
    lock.remove(&rel_path(&game, &dir.join(file_name)));
    lock.save(&store.dir(id)?)
}

/// Путь относительно папки игры для pack.lock (всегда через `/`).
fn rel_path(game: &Path, file: &Path) -> String {
    file.strip_prefix(game)
        .unwrap_or(file)
        .components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// Установить ресурспак или шейдер с Modrinth (подходящую к версии игры версию).
pub async fn install(
    store: &InstanceStore,
    id: &str,
    project: &str,
    kind: Kind,
    world: Option<&str>,
    source: Source,
) -> Result<String> {
    let inst = store.get(id)?;
    let game = store.game_dir(id)?;
    let dir = target_dir(&game, kind, world)?;
    let f = if source == Source::CurseForge {
        curseforge_file(project, &inst.game_version).await?
    } else {
        modrinth_file(project, kind, &inst.game_version).await?
    };
    let name = &f.name;
    if !crate::paths::is_plain_file_name(name) {
        return Err(Error::invalid(format!("Недопустимое имя файла: {name}")));
    }
    std::fs::create_dir_all(&dir).at(&dir)?;
    download::download_all(
        vec![Task {
            url: f.url.clone(),
            dest: dir.join(name),
            expected: Expected::Sha1(f.sha1.clone()),
            size: Some(f.size),
        }],
        2,
        Verify::Full,
        None,
        &Cancel::new(),
    )
    .await?;
    let inst_dir = store.dir(id)?;
    let mut lock = PackLock::load(&inst_dir)?;
    lock.upsert(LockEntry {
        path: rel_path(&game, &dir.join(name)),
        sha1: f.sha1,
        sha512: f.sha512,
        source,
        project_id: Some(f.project_id),
        version_id: Some(f.version_id),
        version_number: Some(f.version_number),
        title: Some(f.title),
        layer: Layer::User,
        installed_at: now(),
        pinned: false,
    });
    lock.save(&inst_dir)?;
    Ok(name.clone())
}

/// Выбранный в каталоге файл: откуда качать, как проверить и что записать в pack.lock.
struct Picked {
    url: String,
    name: String,
    sha1: String,
    sha512: Option<String>,
    size: u64,
    project_id: String,
    version_id: String,
    version_number: String,
    title: String,
}

async fn modrinth_file(project: &str, kind: Kind, game_version: &str) -> Result<Picked> {
    let info = crate::catalog::project(project).await?;
    let loaders: &[&str] = match kind {
        Kind::Resourcepack => &["minecraft"],
        Kind::Shader => &["iris"],
        Kind::Datapack => &["datapack"],
    };
    let list = crate::catalog::versions(&info.id, loaders, game_version).await?;
    let v = crate::catalog::pick(&list, crate::catalog::Channel::Auto).ok_or_else(|| {
        Error::invalid(format!(
            "У «{}» нет версии для Minecraft {game_version}",
            info.title
        ))
    })?;
    let f = v
        .primary_file()
        .ok_or_else(|| Error::invalid(format!("У версии {} нет файла", v.name)))?;
    if !f.url.starts_with("https://") || crate::net::host_of(&f.url) != "cdn.modrinth.com" {
        return Err(Error::invalid(
            "Файл не с CDN Modrinth — установка отменена",
        ));
    }
    Ok(Picked {
        url: f.url.clone(),
        name: f.filename.clone(),
        sha1: f
            .hashes
            .get("sha1")
            .ok_or_else(|| Error::invalid("Modrinth не прислал хэш файла"))?
            .to_ascii_lowercase(),
        sha512: f.hashes.get("sha512").cloned(),
        size: f.size,
        project_id: info.id.clone(),
        version_id: v.id.clone(),
        version_number: v.version_number.clone(),
        title: info.title.clone(),
    })
}

/// Ресурспак, шейдер или датапак с CurseForge: без фильтра по загрузчику, только CDN CurseForge.
async fn curseforge_file(project: &str, game_version: &str) -> Result<Picked> {
    let mod_id: u64 = project
        .parse()
        .map_err(|_| Error::invalid("Неверный ID проекта CurseForge"))?;
    let (title, f) = crate::curseforge::resource_file(mod_id, game_version).await?;
    let url = f.trusted_url().ok_or_else(|| {
        Error::invalid(format!(
            "Автор «{title}» запретил скачивание в сторонних лаунчерах — скачайте файл вручную на сайте CurseForge"
        ))
    })?;
    Ok(Picked {
        url: url.to_string(),
        name: f.file_name.clone(),
        sha1: f
            .sha1()
            .ok_or_else(|| Error::invalid("CurseForge не прислал хэш файла"))?,
        sha512: None,
        size: f.file_length,
        project_id: mod_id.to_string(),
        version_id: f.id.to_string(),
        version_number: f.display_name.clone(),
        title,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::Write;

    fn zip_pack(path: &Path, mcmeta: &str) {
        let mut w = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
        w.start_file("pack.mcmeta", zip::write::SimpleFileOptions::default())
            .unwrap();
        w.write_all(mcmeta.as_bytes()).unwrap();
        w.finish().unwrap();
    }

    #[test]
    fn formats_from_all_mcmeta_styles() {
        assert_eq!(
            pack_formats(&json!({"pack": {"pack_format": 34}})),
            Some((34, 34))
        );
        assert_eq!(
            pack_formats(&json!({"pack": {"pack_format": 15, "supported_formats": [15, 34]}})),
            Some((15, 34))
        );
        assert_eq!(
            pack_formats(
                &json!({"pack": {"pack_format": 15, "supported_formats": {"min_inclusive": 18, "max_inclusive": 46}}})
            ),
            Some((18, 46))
        );
        // С 1.21.9: min_format/max_format, число или [старшая, младшая].
        assert_eq!(
            pack_formats(&json!({"pack": {"min_format": [69, 0], "max_format": 75}})),
            Some((69, 75))
        );
        assert_eq!(
            pack_formats(&json!({"pack": {"description": "без версии"}})),
            None
        );
    }

    fn game_dir(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("quarry-res-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("resourcepacks")).unwrap();
        d
    }

    #[test]
    fn list_marks_enabled_and_compatibility() {
        let g = game_dir("list");
        zip_pack(
            &g.join("resourcepacks/Faithful.zip"),
            r#"{"pack":{"pack_format":75,"description":"32x"}}"#,
        );
        zip_pack(
            &g.join("resourcepacks/Old.zip"),
            r#"{"pack":{"pack_format":15,"description":{"text":"старый"}}}"#,
        );
        std::fs::write(
            g.join("options.txt"),
            "resourcePacks:[\"vanilla\",\"file/Faithful.zip\"]\n",
        )
        .unwrap();
        let l = list(&g, Kind::Resourcepack, Some(75));
        let f = l.iter().find(|p| p.file_name == "Faithful.zip").unwrap();
        assert!(f.enabled);
        assert_eq!((f.compatible, f.description.as_str()), (Some(true), "32x"));
        let o = l.iter().find(|p| p.file_name == "Old.zip").unwrap();
        assert!(!o.enabled);
        assert_eq!(
            (o.compatible, o.description.as_str()),
            (Some(false), "старый")
        );
        // Включённые идут первыми.
        assert_eq!(l[0].file_name, "Faithful.zip");
        std::fs::remove_dir_all(&g).unwrap();
    }

    #[test]
    fn enabling_keeps_builtin_packs_and_other_options() {
        let g = game_dir("enable");
        zip_pack(
            &g.join("resourcepacks/A.zip"),
            r#"{"pack":{"pack_format":75}}"#,
        );
        zip_pack(
            &g.join("resourcepacks/Old.zip"),
            r#"{"pack":{"pack_format":15}}"#,
        );
        std::fs::write(
            g.join("options.txt"),
            "fov:0.5\nresourcePacks:[\"vanilla\",\"fabric\"]\nlang:ru_ru\n",
        )
        .unwrap();
        // В интерфейсе сверху — главный (Old перекрывает A).
        set_enabled(&g, &["Old.zip".into(), "A.zip".into()], Some(75)).unwrap();
        let text = std::fs::read_to_string(g.join("options.txt")).unwrap();
        assert!(
            text.contains("resourcePacks:[\"vanilla\",\"fabric\",\"file/A.zip\",\"file/Old.zip\"]"),
            "{text}"
        );
        // Несовместимый пак игра иначе выкинет при запуске.
        assert!(
            text.contains("incompatibleResourcePacks:[\"file/Old.zip\"]"),
            "{text}"
        );
        assert!(text.contains("fov:0.5") && text.contains("lang:ru_ru"));
        assert!(set_enabled(&g, &["../x.zip".into()], None).is_err());
        std::fs::remove_dir_all(&g).unwrap();
    }

    #[test]
    fn shader_choice_goes_to_iris_config() {
        let g = game_dir("shader");
        std::fs::create_dir_all(g.join("shaderpacks")).unwrap();
        std::fs::write(g.join("shaderpacks/BSL.zip"), b"zip").unwrap();
        std::fs::create_dir_all(g.join("config")).unwrap();
        std::fs::write(
            g.join("config/iris.properties"),
            "maxShadowRenderDistance=32\nshaderPack=\n",
        )
        .unwrap();
        set_shader(&g, Some("BSL.zip")).unwrap();
        let p = std::fs::read_to_string(g.join("config/iris.properties")).unwrap();
        assert!(
            p.contains("shaderPack=BSL.zip")
                && p.contains("enableShaders=true")
                && p.contains("maxShadowRenderDistance=32")
        );
        assert!(list(&g, Kind::Shader, None)[0].enabled);
        set_shader(&g, None).unwrap();
        assert!(
            std::fs::read_to_string(g.join("config/iris.properties"))
                .unwrap()
                .contains("enableShaders=false")
        );
        assert!(set_shader(&g, Some("Нет.zip")).is_err());
        std::fs::remove_dir_all(&g).unwrap();
    }

    #[test]
    fn datapacks_live_inside_a_world() {
        let g = game_dir("datapacks");
        std::fs::create_dir_all(g.join("saves/Мир/datapacks")).unwrap();
        std::fs::write(g.join("saves/Мир/level.dat"), b"x").unwrap();
        zip_pack(
            &g.join("saves/Мир/datapacks/towers.zip"),
            r#"{"pack":{"pack_format":61,"description":"Башни"}}"#,
        );
        let dir = target_dir(&g, Kind::Datapack, Some("Мир")).unwrap();
        assert_eq!(dir, g.join("saves/Мир/datapacks"));
        let l = list(&g.join("saves/Мир"), Kind::Datapack, Some(61));
        assert_eq!(
            (l[0].file_name.as_str(), l[0].compatible),
            ("towers.zip", Some(true))
        );
        assert!(target_dir(&g, Kind::Datapack, Some("../x")).is_err());
        assert!(target_dir(&g, Kind::Datapack, None).is_err());
        assert!(target_dir(&g, Kind::Datapack, Some("Нет")).is_err());
        assert_eq!(
            target_dir(&g, Kind::Shader, None).unwrap(),
            g.join("shaderpacks")
        );
        std::fs::remove_dir_all(&g).unwrap();
    }

    #[test]
    fn pack_icon_becomes_data_url() {
        let g = game_dir("icon");
        let mut w =
            zip::ZipWriter::new(std::fs::File::create(g.join("resourcepacks/I.zip")).unwrap());
        let o = zip::write::SimpleFileOptions::default();
        w.start_file("pack.mcmeta", o).unwrap();
        w.write_all(br#"{"pack":{"pack_format":75}}"#).unwrap();
        w.start_file("pack.png", o).unwrap();
        w.write_all(b"\x89PNG").unwrap();
        w.finish().unwrap();
        let l = list(&g, Kind::Resourcepack, None);
        assert_eq!(l[0].icon.as_deref(), Some("data:image/png;base64,iVBORw=="));
        std::fs::remove_dir_all(&g).unwrap();
    }

    /// Сеть: ресурспак с Modrinth ставится в resourcepacks/ и записывается в pack.lock.
    #[tokio::test]
    #[ignore]
    async fn install_resourcepack_from_modrinth() {
        let root = std::env::temp_dir().join(format!("quarry-res-{}", std::process::id()));
        let store = InstanceStore::new(crate::paths::DataDir::new(root.clone()));
        let inst = store
            .create("Р", "1.21.1", crate::loader::LoaderKind::Vanilla, None)
            .unwrap();
        let name = install(
            &store,
            &inst.id,
            "faithful-32x",
            Kind::Resourcepack,
            None,
            Source::Modrinth,
        )
        .await
        .unwrap();
        assert!(
            store
                .game_dir(&inst.id)
                .unwrap()
                .join("resourcepacks")
                .join(&name)
                .is_file()
        );
        let lock = PackLock::load(&store.dir(&inst.id).unwrap()).unwrap();
        assert!(
            lock.files
                .iter()
                .any(|f| f.source == Source::Modrinth && f.path.ends_with(&name))
        );
        std::fs::remove_dir_all(&root).unwrap();
    }
}
