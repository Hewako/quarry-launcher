//! Редактор конфигов: список и поиск по `config/` и options.txt, проверка синтаксиса,
//! сравнение «было/стало», история правок и отметка «правил игрок» — такие файлы
//! обновление сборки не перезаписывает.

use crate::error::{Error, IoContext, Result};
use crate::paths::{safe_join, write_atomic};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize)]
pub struct ConfigFile {
    /// Путь от папки игры через `/`: `config/sodium-options.json`, `options.txt`.
    pub path: String,
    pub size: u64,
    /// Правил игрок в Quarry — обновление сборки этот файл не заменит.
    pub user_edited: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Hit {
    pub path: String,
    pub line: usize,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DiffLine {
    /// `=` без изменений, `+` добавлено, `-` удалено.
    pub kind: char,
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct HistoryEntry {
    pub at: u64,
    pub size: u64,
}

const EXTS: [&str; 12] = [
    "json",
    "json5",
    "toml",
    "properties",
    "cfg",
    "txt",
    "yml",
    "yaml",
    "snbt",
    "ini",
    "conf",
    "mcmeta",
];
const MAX_SIZE: u64 = 1024 * 1024;
const KEEP_HISTORY: usize = 10;
const USER_FILE: &str = "user-configs.toml";

fn has_text_ext(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| EXTS.contains(&e.to_ascii_lowercase().as_str()))
}

fn walk(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if depth > 6 || out.len() > 3000 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        match e.file_type() {
            Ok(t) if t.is_dir() => walk(&e.path(), depth + 1, out),
            Ok(t) if t.is_file() && has_text_ext(&e.path()) => out.push(e.path()),
            _ => {}
        }
    }
}

fn rel_of(game_dir: &Path, p: &Path) -> String {
    p.strip_prefix(game_dir)
        .unwrap_or(p)
        .components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct UserEdited {
    #[serde(default)]
    files: Vec<String>,
}

fn user_edited(instance_dir: &Path) -> UserEdited {
    std::fs::read_to_string(instance_dir.join(USER_FILE))
        .ok()
        .and_then(|t| toml::from_str(&t).ok())
        .unwrap_or_default()
}

pub fn is_user_edited(instance_dir: &Path, rel: &str) -> bool {
    user_edited(instance_dir).files.iter().any(|f| f == rel)
}

/// Все текстовые конфиги: `config/**` и options.txt.
pub fn list(game_dir: &Path, instance_dir: &Path) -> Vec<ConfigFile> {
    let mut paths = Vec::new();
    walk(&game_dir.join("config"), 0, &mut paths);
    let options = game_dir.join("options.txt");
    if options.is_file() {
        paths.push(options);
    }
    let edited = user_edited(instance_dir);
    let mut out: Vec<ConfigFile> = paths
        .iter()
        .map(|p| {
            let path = rel_of(game_dir, p);
            ConfigFile {
                size: std::fs::metadata(p).map(|m| m.len()).unwrap_or(0),
                user_edited: edited.files.contains(&path),
                path,
            }
        })
        .collect();
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

/// Поиск по именам файлов и по строкам (без учёта регистра). Не больше 300 совпадений.
pub fn search(game_dir: &Path, query: &str) -> Vec<Hit> {
    let q = query.trim().to_lowercase();
    let mut out = Vec::new();
    if q.is_empty() {
        return out;
    }
    for f in list(game_dir, Path::new("")) {
        if out.len() >= 300 {
            break;
        }
        if f.path.to_lowercase().contains(&q) {
            out.push(Hit {
                path: f.path.clone(),
                line: 0,
                text: String::new(),
            });
        }
        if f.size > MAX_SIZE {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(game_dir.join(&f.path)) else {
            continue;
        };
        for (i, line) in text.lines().enumerate() {
            if line.to_lowercase().contains(&q) {
                out.push(Hit {
                    path: f.path.clone(),
                    line: i + 1,
                    text: line.trim().chars().take(200).collect(),
                });
                if out.len() >= 300 {
                    break;
                }
            }
        }
    }
    out
}

/// Путь к конфигу из интерфейса: только options.txt и файлы внутри `config/`.
fn config_path(game_dir: &Path, rel: &str) -> Result<PathBuf> {
    let bad = || Error::invalid(format!("Это не файл настроек: {rel}"));
    if rel != "options.txt" && !rel.starts_with("config/") {
        return Err(bad());
    }
    if rel.contains('\\') || rel.split('/').any(|p| p == ".." || p.is_empty()) {
        return Err(bad());
    }
    let path = safe_join(game_dir, rel).ok_or_else(bad)?;
    if !has_text_ext(&path) {
        return Err(bad());
    }
    Ok(path)
}

pub fn read(game_dir: &Path, rel: &str) -> Result<String> {
    let path = config_path(game_dir, rel)?;
    let meta = std::fs::metadata(&path).at(&path)?;
    if meta.len() > MAX_SIZE {
        return Err(Error::invalid(
            "Файл больше 1 МБ — откройте его в текстовом редакторе",
        ));
    }
    String::from_utf8(std::fs::read(&path).at(&path)?)
        .map_err(|_| Error::invalid("Это не текстовый файл"))
}

/// Проверить синтаксис по расширению: JSON и TOML строго, остальное — как есть.
pub fn validate(rel: &str, text: &str) -> std::result::Result<(), String> {
    let ext = rel.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "json" | "mcmeta" => serde_json::from_str::<serde_json::Value>(text)
            .map(|_| ())
            .map_err(|e| format!("Ошибка в JSON, строка {}: {e}", e.line())),
        "toml" => toml::from_str::<toml::Value>(text)
            .map(|_| ())
            .map_err(|e| format!("Ошибка в TOML: {}", e.message())),
        _ => Ok(()),
    }
}

/// Построчное сравнение (наибольшая общая подпоследовательность).
pub fn diff(old: &str, new: &str) -> Vec<DiffLine> {
    let a: Vec<&str> = old.lines().collect();
    let b: Vec<&str> = new.lines().collect();
    let line = |kind, t: &str| DiffLine {
        kind,
        text: t.to_string(),
    };
    if a.len().saturating_mul(b.len()) > 4_000_000 {
        // Слишком большие файлы: показываем «всё старое → всё новое».
        return a
            .iter()
            .map(|t| line('-', t))
            .chain(b.iter().map(|t| line('+', t)))
            .collect();
    }
    let (n, m) = (a.len(), b.len());
    let mut lcs = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i][j] = if a[i] == b[j] {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }
    let (mut i, mut j, mut out) = (0, 0, Vec::new());
    while i < n || j < m {
        if i < n && j < m && a[i] == b[j] {
            out.push(line('=', a[i]));
            i += 1;
            j += 1;
        } else if i < n && (j == m || lcs[i + 1][j] >= lcs[i][j + 1]) {
            out.push(line('-', a[i]));
            i += 1;
        } else {
            out.push(line('+', b[j]));
            j += 1;
        }
    }
    out
}

fn history_dir(instance_dir: &Path, rel: &str) -> PathBuf {
    instance_dir.join("config-history").join(rel)
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Сохранить конфиг: проверка синтаксиса, прежняя версия — в историю (10 последних),
/// файл отмечается как «правил игрок».
pub fn save(game_dir: &Path, instance_dir: &Path, rel: &str, text: &str) -> Result<()> {
    let path = config_path(game_dir, rel)?;
    validate(rel, text).map_err(Error::invalid)?;
    if let Ok(old) = std::fs::read(&path) {
        let dir = history_dir(instance_dir, rel);
        std::fs::create_dir_all(&dir).at(&dir)?;
        let mut at = now_ms();
        while dir.join(format!("{at}.txt")).exists() {
            at += 1;
        }
        let h = dir.join(format!("{at}.txt"));
        std::fs::write(&h, old).at(&h)?;
        for e in history(instance_dir, rel).iter().skip(KEEP_HISTORY) {
            let _ = std::fs::remove_file(dir.join(format!("{}.txt", e.at)));
        }
    }
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p).at(p)?;
    }
    write_atomic(&path, text.as_bytes())?;
    let mut edited = user_edited(instance_dir);
    if !edited.files.iter().any(|f| f == rel) {
        edited.files.push(rel.to_string());
        edited.files.sort();
        let t = toml::to_string_pretty(&edited).map_err(|e| Error::parse(USER_FILE, e))?;
        write_atomic(&instance_dir.join(USER_FILE), t.as_bytes())?;
    }
    Ok(())
}

/// Прежние версии файла, новые первыми.
pub fn history(instance_dir: &Path, rel: &str) -> Vec<HistoryEntry> {
    let Ok(rd) = std::fs::read_dir(history_dir(instance_dir, rel)) else {
        return Vec::new();
    };
    let mut out: Vec<HistoryEntry> = rd
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            Some(HistoryEntry {
                at: name.strip_suffix(".txt")?.parse().ok()?,
                size: e.metadata().map(|m| m.len()).unwrap_or(0),
            })
        })
        .collect();
    out.sort_by_key(|h| std::cmp::Reverse(h.at));
    out
}

pub fn history_read(instance_dir: &Path, rel: &str, at: u64) -> Result<String> {
    config_path(Path::new("/"), rel)?; // та же проверка пути
    let p = history_dir(instance_dir, rel).join(format!("{at}.txt"));
    String::from_utf8(std::fs::read(&p).at(&p)?)
        .map_err(|_| Error::invalid("Это не текстовый файл"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dirs(name: &str) -> (std::path::PathBuf, std::path::PathBuf) {
        let root = std::env::temp_dir().join(format!("quarry-cfg-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let game = root.join("game");
        std::fs::create_dir_all(game.join("config/sodium")).unwrap();
        std::fs::write(
            game.join("config/sodium/options.json"),
            "{\n  \"fps\": 60\n}\n",
        )
        .unwrap();
        std::fs::write(game.join("config/iris.properties"), "shaderPack=BSL.zip\n").unwrap();
        std::fs::write(game.join("config/blob.bin"), [0u8, 159, 146, 150]).unwrap();
        std::fs::write(game.join("options.txt"), "fov:0.5\n").unwrap();
        (game, root)
    }

    #[test]
    fn lists_text_configs_and_options() {
        let (g, root) = dirs("list");
        let paths: Vec<String> = list(&g, &root).into_iter().map(|c| c.path).collect();
        assert_eq!(
            paths,
            [
                "config/iris.properties",
                "config/sodium/options.json",
                "options.txt"
            ]
        );
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn search_finds_lines_in_files() {
        let (g, root) = dirs("search");
        let hits = search(&g, "SHADERPACK");
        assert_eq!(
            hits,
            [Hit {
                path: "config/iris.properties".into(),
                line: 1,
                text: "shaderPack=BSL.zip".into()
            }]
        );
        // Имя файла тоже ищется.
        assert!(
            search(&g, "sodium")
                .iter()
                .any(|h| h.path == "config/sodium/options.json")
        );
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn read_rejects_paths_outside_configs() {
        let (g, root) = dirs("read");
        assert!(
            read(&g, "config/iris.properties")
                .unwrap()
                .starts_with("shaderPack")
        );
        for bad in [
            "../secret",
            "config/../../x",
            "mods/a.jar",
            "/etc/passwd",
            "config/blob.bin",
        ] {
            assert!(read(&g, bad).is_err(), "{bad}");
        }
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn validation_points_to_the_line() {
        assert!(validate("config/a.json", "{\"a\": 1}").is_ok());
        let e = validate("config/a.json", "{\n\"a\": 1,\n}").unwrap_err();
        assert!(e.contains("строка 3"), "{e}");
        assert!(validate("config/a.toml", "a = 1\nb = \"x\"").is_ok());
        assert!(validate("config/a.toml", "a = = 1").is_err());
        // Форматы без строгой проверки принимаются как есть.
        assert!(validate("config/a.properties", "что угодно").is_ok());
    }

    #[test]
    fn line_diff() {
        let d = diff("a\nb\nc\n", "a\nB\nc\nd\n");
        let kinds: String = d.iter().map(|l| l.kind).collect();
        assert_eq!(kinds, "=-+=+");
        assert_eq!(d[2].text, "B");
    }

    #[test]
    fn save_keeps_history_and_marks_user_edited() {
        let (g, root) = dirs("save");
        let rel = "config/sodium/options.json";
        assert!(!is_user_edited(&root, rel));
        assert!(save(&g, &root, rel, "{ broken").is_err());
        save(&g, &root, rel, "{\n  \"fps\": 144\n}\n").unwrap();
        assert!(read(&g, rel).unwrap().contains("144"));
        assert!(is_user_edited(&root, rel));
        let h = history(&root, rel);
        assert_eq!(h.len(), 1);
        assert!(history_read(&root, rel, h[0].at).unwrap().contains("60"));
        for i in 0..12 {
            save(&g, &root, rel, &format!("{{\"fps\": {i}}}")).unwrap();
        }
        assert!(history(&root, rel).len() <= 10);
        std::fs::remove_dir_all(&root).unwrap();
    }
}
