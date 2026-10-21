//! Настройки игры (options.txt): чтение и правка без потери неизвестных строк, конфликты
//! клавиш, пресеты графики/звука/управления, импорт и экспорт раскладки.

use crate::error::{Error, IoContext, Result};
use crate::paths::write_atomic;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    Graphics,
    Sound,
    Controls,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct KeyConflict {
    /// Клавиша, например `key.keyboard.r`.
    pub key: String,
    /// Действия, которым она назначена (`key.drop`, `key.sprint` …).
    pub actions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Preset {
    pub name: String,
    pub category: Category,
    pub count: usize,
}

/// Параметры графики, которые стоит хранить в пресете (названия из options.txt).
const GRAPHICS: [&str; 22] = [
    "renderDistance",
    "simulationDistance",
    "graphicsMode",
    "maxFps",
    "enableVsync",
    "fullscreen",
    "guiScale",
    "fov",
    "gamma",
    "renderClouds",
    "particles",
    "entityShadows",
    "biomeBlendRadius",
    "mipmapLevels",
    "ao",
    "bobView",
    "entityDistanceScaling",
    "fovEffectScale",
    "screenEffectScale",
    "prioritizeChunkUpdates",
    "menuBackgroundBlurriness",
    "cloudRange",
];
const CONTROLS: [&str; 7] = [
    "mouseSensitivity",
    "invertYMouse",
    "toggleCrouch",
    "toggleSprint",
    "autoJump",
    "rawMouseInput",
    "mouseWheelSensitivity",
];

/// Строки `ключ:значение` по порядку; строки без двоеточия пропускаются.
pub fn parse(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

pub fn read(game_dir: &Path) -> Result<Vec<(String, String)>> {
    let path = game_dir.join("options.txt");
    match std::fs::read_to_string(&path) {
        Ok(t) => Ok(parse(&t)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(Error::io(&path, e)),
    }
}

fn check_entry(k: &str, v: &str) -> Result<()> {
    if k.is_empty() || k.contains([':', '\n', '\r']) || v.contains(['\n', '\r']) {
        return Err(Error::invalid(format!("Недопустимая настройка: {k:?}")));
    }
    Ok(())
}

/// Изменить значения, сохранив порядок и все остальные строки; новые ключи — в конец.
pub fn set(game_dir: &Path, changes: &BTreeMap<String, String>) -> Result<()> {
    for (k, v) in changes {
        check_entry(k, v)?;
    }
    let path = game_dir.join("options.txt");
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(Error::io(&path, e)),
    };
    let mut done = std::collections::BTreeSet::new();
    let mut out: Vec<String> = text
        .lines()
        .map(|line| match line.split_once(':') {
            Some((k, _)) if changes.contains_key(k) => {
                done.insert(k.to_string());
                format!("{k}:{}", changes[k])
            }
            _ => line.to_string(),
        })
        .collect();
    for (k, v) in changes {
        if !done.contains(k) {
            out.push(format!("{k}:{v}"));
        }
    }
    write_atomic(&path, (out.join("\n") + "\n").as_bytes())
}

pub fn category_of(key: &str) -> Option<Category> {
    if key.starts_with("key_") || CONTROLS.contains(&key) {
        Some(Category::Controls)
    } else if key.starts_with("soundCategory_") || key == "soundDevice" {
        Some(Category::Sound)
    } else if GRAPHICS.contains(&key) {
        Some(Category::Graphics)
    } else {
        None
    }
}

/// Одна клавиша на несколько действий. Неназначенные (`unknown`) не считаются.
pub fn conflicts(entries: &[(String, String)]) -> Vec<KeyConflict> {
    let mut by_key: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for (k, v) in entries {
        if let Some(action) = k.strip_prefix("key_")
            && !v.ends_with(".unknown")
        {
            by_key
                .entry(v.as_str())
                .or_default()
                .push(action.to_string());
        }
    }
    by_key
        .into_iter()
        .filter(|(_, a)| a.len() > 1)
        .map(|(key, actions)| KeyConflict {
            key: key.to_string(),
            actions,
        })
        .collect()
}

fn preset_path(dir: &Path, name: &str) -> Result<std::path::PathBuf> {
    let n = name.trim();
    if n.is_empty()
        || n.chars().count() > 48
        || n.starts_with('.')
        || n.contains(['/', '\\', ':', '\n'])
    {
        return Err(Error::invalid(
            "Название пресета: до 48 символов, без / \\ :",
        ));
    }
    Ok(dir.join(format!("{n}.txt")))
}

fn category_name(c: Category) -> &'static str {
    match c {
        Category::Graphics => "graphics",
        Category::Sound => "sound",
        Category::Controls => "controls",
    }
}

/// Сохранить пресет: только настройки своей категории.
pub fn save_preset(
    presets_dir: &Path,
    name: &str,
    category: Category,
    entries: &[(String, String)],
) -> Result<Preset> {
    let path = preset_path(presets_dir, name)?;
    let picked: Vec<&(String, String)> = entries
        .iter()
        .filter(|(k, _)| category_of(k) == Some(category))
        .collect();
    if picked.is_empty() {
        return Err(Error::invalid(
            "В options.txt нет настроек этой категории — сначала запустите игру",
        ));
    }
    let mut text = format!("# quarry-preset: {}\n", category_name(category));
    for (k, v) in &picked {
        text.push_str(&format!("{k}:{v}\n"));
    }
    std::fs::create_dir_all(presets_dir).at(presets_dir)?;
    write_atomic(&path, text.as_bytes())?;
    Ok(Preset {
        name: name.trim().to_string(),
        category,
        count: picked.len(),
    })
}

pub fn list_presets(presets_dir: &Path) -> Vec<Preset> {
    let Ok(rd) = std::fs::read_dir(presets_dir) else {
        return Vec::new();
    };
    let mut out: Vec<Preset> = rd
        .flatten()
        .filter_map(|e| {
            let name = e
                .file_name()
                .to_string_lossy()
                .strip_suffix(".txt")?
                .to_string();
            let text = std::fs::read_to_string(e.path()).ok()?;
            let cat = match text.lines().next()?.strip_prefix("# quarry-preset: ")? {
                "graphics" => Category::Graphics,
                "sound" => Category::Sound,
                "controls" => Category::Controls,
                _ => return None,
            };
            Some(Preset {
                name,
                category: cat,
                count: parse(&text).len(),
            })
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

pub fn load_preset(presets_dir: &Path, name: &str) -> Result<BTreeMap<String, String>> {
    let path = preset_path(presets_dir, name)?;
    let text = std::fs::read_to_string(&path).at(&path)?;
    Ok(parse(&text)
        .into_iter()
        .filter(|(k, v)| category_of(k).is_some() && check_entry(k, v).is_ok())
        .collect())
}

/// Раскладка управления из файла (только строки `key_*`).
pub fn import_layout(text: &str) -> Result<BTreeMap<String, String>> {
    let keys: BTreeMap<String, String> = parse(text)
        .into_iter()
        .filter(|(k, v)| k.starts_with("key_") && check_entry(k, v).is_ok())
        .collect();
    if keys.is_empty() {
        return Err(Error::invalid(
            "В файле нет назначений клавиш (строк key_…)",
        ));
    }
    Ok(keys)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("quarry-opts-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn set_keeps_order_and_unknown_lines() {
        let d = dir("set");
        std::fs::write(
            d.join("options.txt"),
            "version:4671\nfov:0.0\nmodded_thing:42\nkey_key.jump:key.keyboard.space\n",
        )
        .unwrap();
        set(
            &d,
            &[
                ("fov".to_string(), "0.25".to_string()),
                ("newKey".to_string(), "true".to_string()),
            ]
            .into(),
        )
        .unwrap();
        let text = std::fs::read_to_string(d.join("options.txt")).unwrap();
        assert_eq!(
            text,
            "version:4671\nfov:0.25\nmodded_thing:42\nkey_key.jump:key.keyboard.space\nnewKey:true\n"
        );
        assert_eq!(
            read(&d).unwrap()[1],
            ("fov".to_string(), "0.25".to_string())
        );
        assert!(set(&d, &[("bad\nkey".to_string(), "x".to_string())].into()).is_err());
        assert!(set(&d, &[("fov".to_string(), "1\n2".to_string())].into()).is_err());
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn categories() {
        assert_eq!(category_of("key_key.jump"), Some(Category::Controls));
        assert_eq!(category_of("mouseSensitivity"), Some(Category::Controls));
        assert_eq!(category_of("soundCategory_music"), Some(Category::Sound));
        assert_eq!(category_of("renderDistance"), Some(Category::Graphics));
        assert_eq!(category_of("lang"), None);
    }

    #[test]
    fn finds_key_conflicts() {
        let e = parse(
            "key_key.drop:key.keyboard.q\nkey_key.sprint:key.keyboard.q\nkey_key.jump:key.keyboard.space\nkey_key.chat:key.keyboard.unknown\nkey_key.command:key.keyboard.unknown\n",
        );
        assert_eq!(e.len(), 5);
        assert_eq!(
            conflicts(&e),
            [KeyConflict {
                key: "key.keyboard.q".into(),
                actions: vec!["key.drop".into(), "key.sprint".into()]
            }]
        );
    }

    #[test]
    fn presets_store_only_their_category() {
        let d = dir("presets");
        let entries = parse(
            "renderDistance:8\nsoundCategory_music:0.0\nfov:0.5\nkey_key.jump:key.keyboard.space\n",
        );
        let p = save_preset(&d, "Слабый ноутбук", Category::Graphics, &entries).unwrap();
        assert_eq!(p.count, 2);
        assert_eq!(list_presets(&d), [p]);
        let loaded = load_preset(&d, "Слабый ноутбук").unwrap();
        assert_eq!(loaded.get("renderDistance").map(String::as_str), Some("8"));
        assert!(!loaded.contains_key("soundCategory_music"));
        assert!(save_preset(&d, "../x", Category::Sound, &entries).is_err());
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn layout_import_takes_only_keys() {
        let l = import_layout("key_key.jump:key.keyboard.space\nfov:1.0\n# комментарий\nkey_key.drop:key.keyboard.g\n").unwrap();
        assert_eq!(l.len(), 2);
        assert!(import_layout("fov:1.0\n").is_err());
    }
}
