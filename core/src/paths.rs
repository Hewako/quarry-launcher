//! Раскладка папки данных (план, Приложение В). Общий кеш отделён от данных профилей,
//! секретов в файлах нет.
//!
//! ```text
//! <данные>/
//! ├── launcher.toml        глобальные настройки
//! ├── accounts.toml        провайдер + UUID + имя (токены — в хранилище ОС)
//! ├── cache/{meta,libraries,assets,versions,natives,skins,catalog}
//! ├── runtimes/            Java, скачанные лаунчером
//! ├── instances/<id>/      instance.toml, journal.jsonl, snapshots/, logs/, game/
//! ├── backups/<id>/        бэкапы миров
//! ├── trash/               удалённые профили (восстанавливаются вручную)
//! └── logs/                логи лаунчера (уже маскированные)
//! ```

use crate::error::{IoContext, Result};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct DataDir {
    root: PathBuf,
}

impl DataDir {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Папка по умолчанию: `~/.quarry` (так было с первой версии, не переносим без нужды).
    pub fn default_root() -> Option<PathBuf> {
        dirs::home_dir().map(|h| h.join(".quarry"))
    }

    /// Открыть папку данных: создать при необходимости и перенести старую раскладку.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self> {
        let dir = Self::new(root);
        std::fs::create_dir_all(&dir.root).at(&dir.root)?;
        dir.migrate_legacy_layout()?;
        Ok(dir)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn launcher_toml(&self) -> PathBuf {
        self.root.join("launcher.toml")
    }
    pub fn accounts_toml(&self) -> PathBuf {
        self.root.join("accounts.toml")
    }
    pub fn cache(&self) -> PathBuf {
        self.root.join("cache")
    }
    pub fn meta(&self) -> PathBuf {
        self.cache().join("meta")
    }
    pub fn libraries(&self) -> PathBuf {
        self.cache().join("libraries")
    }
    pub fn assets(&self) -> PathBuf {
        self.cache().join("assets")
    }
    /// Клиентские JAR и logging-конфиги версий.
    pub fn versions(&self) -> PathBuf {
        self.cache().join("versions")
    }
    pub fn natives(&self) -> PathBuf {
        self.cache().join("natives")
    }
    pub fn skins(&self) -> PathBuf {
        self.cache().join("skins")
    }
    pub fn catalog(&self) -> PathBuf {
        self.cache().join("catalog")
    }
    pub fn runtimes(&self) -> PathBuf {
        self.root.join("runtimes")
    }
    pub fn instances(&self) -> PathBuf {
        self.root.join("instances")
    }
    pub fn backups(&self) -> PathBuf {
        self.root.join("backups")
    }
    pub fn trash(&self) -> PathBuf {
        self.root.join("trash")
    }
    pub fn logs(&self) -> PathBuf {
        self.root.join("logs")
    }
    /// Временные файлы загрузок (на том же диске, что и кеш, чтобы rename был атомарным).
    pub fn tmp(&self) -> PathBuf {
        self.cache().join("tmp")
    }

    /// Перенос раскладки до версии 0.1 (`profiles/`, `libraries/`, `assets/`, `versions/`,
    /// `natives/`, `java/` в корне) в новую. Только переименования внутри одной папки:
    /// ничего не удаляется и не перезаписывается. Если цель уже существует — источник остаётся
    /// на месте, и об этом пишется в `logs/migration.log`.
    fn migrate_legacy_layout(&self) -> Result<()> {
        let moves: [(&str, PathBuf); 6] = [
            ("profiles", self.instances()),
            ("libraries", self.libraries()),
            ("assets", self.assets()),
            ("versions", self.versions()),
            ("natives", self.natives()),
            ("java", self.runtimes().join("legacy-java")),
        ];
        let mut log = Vec::new();
        for (old, new) in moves {
            let src = self.root.join(old);
            if !src.is_dir() {
                continue;
            }
            if new.exists() {
                log.push(format!(
                    "пропущено: {} — {} уже существует",
                    src.display(),
                    new.display()
                ));
                continue;
            }
            if let Some(parent) = new.parent() {
                std::fs::create_dir_all(parent).at(parent)?;
            }
            std::fs::rename(&src, &new).at(&src)?;
            log.push(format!("перенесено: {} → {}", src.display(), new.display()));
        }
        // Старые профили: profile.toml → instance.toml делает InstanceStore при чтении.
        if !log.is_empty() {
            std::fs::create_dir_all(self.logs()).at(self.logs())?;
            let path = self.logs().join("migration.log");
            let mut text = std::fs::read_to_string(&path).unwrap_or_default();
            for line in log {
                text.push_str(&line);
                text.push('\n');
            }
            std::fs::write(&path, text).at(&path)?;
        }
        Ok(())
    }
}

/// Атомарная запись: во временный файл рядом, затем переименование.
pub fn write_atomic(path: &Path, data: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).at(parent)?;
    }
    let tmp = unique_sibling(path, "tmp");
    std::fs::write(&tmp, data).at(&tmp)?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        crate::error::Error::io(path, e)
    })
}

/// Уникальное имя рядом с файлом: `<имя>.<тег>-<случайное>`. Не совпадает у разных файлов
/// одной папки и у параллельных загрузок одного файла.
pub fn unique_sibling(path: &Path, tag: &str) -> PathBuf {
    use rand::Rng;
    let suffix: u64 = rand::thread_rng().r#gen();
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    path.with_file_name(format!("{name}.{tag}-{suffix:016x}"))
}

/// Соединить базовую папку с относительным путём из недоверенного источника (JSON версии,
/// архив, .mrpack). Абсолютные пути, `..` и префиксы дисков отклоняются.
pub fn safe_join(base: &Path, relative: &str) -> Option<PathBuf> {
    use std::path::Component;
    let rel = Path::new(relative);
    let mut out = base.to_path_buf();
    let mut depth = 0usize;
    for comp in rel.components() {
        match comp {
            Component::Normal(part) => {
                out.push(part);
                depth += 1;
            }
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    // Обратная косая черта в имени на Unix — тоже подозрительно (zip из Windows).
    if depth == 0 || relative.contains('\\') {
        return None;
    }
    Some(out)
}

/// Рекурсивно скопировать папку (символические ссылки пропускаются).
pub fn copy_dir(from: &Path, to: &Path) -> Result<u64> {
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

/// Свободное место на диске, где лежит `path` (байты). Через `df` или PowerShell —
/// без лишних зависимостей. Ближайшая существующая папка выше по пути тоже подходит.
pub fn free_space(path: &Path) -> Option<u64> {
    let mut p = path.to_path_buf();
    while !p.exists() {
        p = p.parent()?.to_path_buf();
    }
    if cfg!(windows) {
        let script = format!(
            "(Get-Item -LiteralPath '{}').PSDrive.Free",
            p.display().to_string().replace('\'', "''")
        );
        let mut c = std::process::Command::new("powershell");
        c.args(["-NoProfile", "-Command", &script]);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW: без мигающей консоли
        }
        let out = c.output().ok()?;
        return String::from_utf8_lossy(&out.stdout).trim().parse().ok();
    }
    let out = std::process::Command::new("df")
        .arg("-Pk")
        .arg(&p)
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    // Вторая строка: файловая система, размер, занято, свободно (КБ), …
    let kb: u64 = text
        .lines()
        .nth(1)?
        .split_whitespace()
        .nth(3)?
        .parse()
        .ok()?;
    Some(kb * 1024)
}

/// Размер папки со всем содержимым (символические ссылки не считаются).
pub fn dir_size(p: &Path) -> u64 {
    let Ok(rd) = std::fs::read_dir(p) else {
        return 0;
    };
    rd.flatten()
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() => dir_size(&e.path()),
            Ok(t) if t.is_file() => e.metadata().map(|m| m.len()).unwrap_or(0),
            _ => 0,
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_join_accepts_normal_paths() {
        let base = Path::new("/data");
        assert_eq!(
            safe_join(base, "org/lwjgl/lwjgl.jar"),
            Some(PathBuf::from("/data/org/lwjgl/lwjgl.jar"))
        );
        assert_eq!(safe_join(base, "./a/b"), Some(PathBuf::from("/data/a/b")));
    }

    #[test]
    fn safe_join_rejects_traversal() {
        let base = Path::new("/data");
        assert_eq!(safe_join(base, "../etc/passwd"), None);
        assert_eq!(safe_join(base, "a/../../b"), None);
        assert_eq!(safe_join(base, "/etc/passwd"), None);
        assert_eq!(safe_join(base, "a\\..\\b"), None);
        assert_eq!(safe_join(base, ""), None);
    }

    #[test]
    fn unique_siblings_differ() {
        let p = Path::new("/x/file.jar");
        let a = unique_sibling(p, "part");
        let b = unique_sibling(p, "part");
        assert_ne!(a, b);
        assert!(a.to_string_lossy().starts_with("/x/file.jar.part-"));
    }

    #[test]
    fn legacy_layout_is_moved_without_loss() {
        let root = std::env::temp_dir().join(format!("quarry-migrate-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("profiles/my")).unwrap();
        std::fs::write(root.join("profiles/my/profile.toml"), "x").unwrap();
        std::fs::create_dir_all(root.join("libraries/a")).unwrap();
        // Цель для assets уже есть — источник должен остаться на месте.
        std::fs::create_dir_all(root.join("assets")).unwrap();
        std::fs::create_dir_all(root.join("cache/assets")).unwrap();

        let dir = DataDir::open(&root).unwrap();
        assert!(dir.instances().join("my/profile.toml").exists());
        assert!(dir.libraries().join("a").is_dir());
        assert!(!root.join("profiles").exists());
        assert!(
            root.join("assets").is_dir(),
            "существующая цель не перезаписывается"
        );
        let log = std::fs::read_to_string(dir.logs().join("migration.log")).unwrap();
        assert!(log.contains("пропущено"));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn atomic_write_replaces_content() {
        let root = std::env::temp_dir().join(format!("quarry-atomic-{}", std::process::id()));
        let path = root.join("a.toml");
        write_atomic(&path, b"one").unwrap();
        write_atomic(&path, b"two").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "two");
        assert_eq!(
            std::fs::read_dir(&root).unwrap().count(),
            1,
            "временных файлов не осталось"
        );
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn free_space_is_known() {
        let free = free_space(&std::env::temp_dir()).expect("df/PowerShell доступны");
        assert!(free > 1024 * 1024, "{free}");
    }
}
