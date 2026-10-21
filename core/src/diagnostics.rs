//! Диагностика: архив отчётов о крашах, отчёт поддержки одним действием (с предпросмотром
//! и маскированием), место на диске и безопасная очистка кеша.

use crate::error::{Error, IoContext, Result};
use crate::instance::InstanceStore;
use crate::logmask::Masker;
use serde::Serialize;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize)]
pub struct CrashReportInfo {
    pub file: String,
    /// Время из имени файла `crash-YYYY-MM-DD_HH.MM.SS-client.txt` (как написала игра).
    pub when: String,
    pub size: u64,
    /// Строка `Description:` из отчёта.
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReportFile {
    pub name: String,
    pub content: String,
}

/// Отчёты о крашах игры, новые первыми.
pub fn crash_reports(game_dir: &Path) -> Vec<CrashReportInfo> {
    let Ok(rd) = std::fs::read_dir(game_dir.join("crash-reports")) else {
        return Vec::new();
    };
    let mut out: Vec<CrashReportInfo> = rd
        .flatten()
        .filter_map(|e| {
            let file = e.file_name().to_string_lossy().into_owned();
            if !file.ends_with(".txt") {
                return None;
            }
            // crash-2026-10-20_18.00.01-client.txt → 2026-10-20 18:00:01
            let when = file
                .strip_prefix("crash-")
                .and_then(|r| r.get(..19))
                .map(|t| t.replacen('_', " ", 1).replace('.', ":"))
                .unwrap_or_default();
            let description = std::fs::read_to_string(e.path())
                .ok()
                .and_then(|t| {
                    t.lines()
                        .find_map(|l| l.strip_prefix("Description:").map(|d| d.trim().to_string()))
                })
                .unwrap_or_default();
            Some(CrashReportInfo {
                size: e.metadata().map(|m| m.len()).unwrap_or(0),
                file,
                when,
                description,
            })
        })
        .collect();
    out.sort_by(|a, b| b.file.cmp(&a.file));
    out
}

fn crash_path(game_dir: &Path, file: &str) -> Result<PathBuf> {
    if file.contains(['/', '\\']) || file.contains("..") || !file.ends_with(".txt") {
        return Err(Error::invalid(format!("Нет такого отчёта: {file}")));
    }
    Ok(game_dir.join("crash-reports").join(file))
}

/// Текст отчёта о краше с маскированием (домашняя папка, IP).
pub fn read_crash_report(game_dir: &Path, file: &str) -> Result<String> {
    let path = crash_path(game_dir, file)?;
    let text = std::fs::read_to_string(&path).at(&path)?;
    let m = Masker::new();
    Ok(text
        .lines()
        .map(|l| m.mask(l))
        .collect::<Vec<_>>()
        .join("\n"))
}

fn newest_file(dir: &Path, ext: &str) -> Option<PathBuf> {
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().ends_with(ext))
        .max_by_key(|e| e.metadata().and_then(|m| m.modified()).ok())
        .map(|e| e.path())
}

fn tail(text: &str, lines: usize) -> String {
    let all: Vec<&str> = text.lines().collect();
    all[all.len().saturating_sub(lines)..].join("\n")
}

/// Собрать отчёт поддержки: сводка, моды, Mod Doctor, журнал, последний лог и краш.
/// Всё проходит маскирование; `hide_nick` — ник для скрытия (режим стримера).
pub fn support_report(
    store: &InstanceStore,
    id: &str,
    hide_nick: Option<&str>,
) -> Result<Vec<ReportFile>> {
    let inst = store.get(id)?;
    let game = store.game_dir(id)?;
    let dir = store.dir(id)?;
    let masker = Masker::new().with_nick(hide_nick);
    let mask = |t: &str| {
        t.lines()
            .map(|l| masker.mask(l))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let global = crate::settings::GlobalSettings::load(store.data()).unwrap_or_default();
    let resolved = crate::settings::resolve(&inst.settings, &inst.pack_settings, &global.launch);
    let files = crate::mods::inventory(&game, &dir).unwrap_or_default();

    let mut summary = format!(
        "Отчёт поддержки Quarry\n\nQuarry: {}\nСистема: {} {}\n\nВерсия игры: {}\nЗагрузчик: {} {}\nВход: {:?}\nПамять: {} МБ (откуда: {:?})\nJava: {}\nМодов: {} (включено {})\n",
        crate::launch::LAUNCHER_VERSION,
        std::env::consts::OS,
        std::env::consts::ARCH,
        inst.game_version,
        inst.loader.display(),
        inst.loader_version.as_deref().unwrap_or("—"),
        inst.auth_mode,
        resolved.max_memory_mb.value,
        resolved.max_memory_mb.source,
        resolved
            .java_path
            .value
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "подбор по версии игры".into()),
        files.len(),
        files.iter().filter(|f| f.enabled).count(),
    );
    if let Some(l) = &inst.last_launch {
        summary.push_str(&format!(
            "\nПоследний запуск: {:?}, код {:?}, {} с, память до {:?} МБ, ЦП в среднем {:?} %\n{}\n",
            l.outcome,
            l.exit_code,
            l.duration_secs,
            l.peak_ram_mb,
            l.cpu_percent,
            l.message.as_deref().unwrap_or("")
        ));
    }
    let mut out = vec![ReportFile {
        name: "summary.txt".into(),
        content: mask(&summary),
    }];

    let mods: String = files
        .iter()
        .map(|f| {
            format!(
                "{} {} {} ({}){}\n",
                if f.enabled { "[вкл]" } else { "[выкл]" },
                f.display_name(),
                f.primary().map(|m| m.version.as_str()).unwrap_or("?"),
                f.file_name,
                f.source
                    .as_ref()
                    .map(|s| format!(" · {:?}", s.source))
                    .unwrap_or_default()
            )
        })
        .collect();
    out.push(ReportFile {
        name: "mods.txt".into(),
        content: mask(&mods),
    });

    if inst.loader != crate::loader::LoaderKind::Vanilla {
        let r = crate::mods::doctor::check(
            &files,
            inst.loader,
            &inst.game_version,
            inst.loader_version.as_deref(),
        );
        let text: String = r
            .findings
            .iter()
            .map(|f| {
                format!(
                    "[{:?}] {}\n  {}\n  {}\n",
                    f.severity, f.title, f.detail, f.evidence
                )
            })
            .collect();
        out.push(ReportFile {
            name: "mod-doctor.txt".into(),
            content: mask(if text.is_empty() {
                "Проблем не найдено.\n"
            } else {
                &text
            }),
        });
    }

    let journal: String = crate::mods::manage::journal(&dir)
        .iter()
        .rev()
        .take(50)
        .map(|j| format!("{} {} {} — {}\n", j.at, j.action, j.what, j.reason))
        .collect();
    if !journal.is_empty() {
        out.push(ReportFile {
            name: "journal.txt".into(),
            content: mask(&journal),
        });
    }
    if let Some(log) = newest_file(&store.logs_dir(id)?, ".log")
        && let Ok(t) = std::fs::read_to_string(&log)
    {
        out.push(ReportFile {
            name: "launcher-log.txt".into(),
            content: mask(&tail(&t, 3000)),
        });
    }
    if let Some(c) = crash_reports(&game).first()
        && let Ok(t) = std::fs::read_to_string(game.join("crash-reports").join(&c.file))
    {
        out.push(ReportFile {
            name: "crash-report.txt".into(),
            content: mask(&tail(&t, 3000)),
        });
    }
    Ok(out)
}

#[derive(Debug, Clone, Serialize)]
pub struct StoragePart {
    pub label: String,
    pub size: u64,
    /// Можно очистить без вреда: скачается или соберётся заново.
    pub cleanable: bool,
}

/// Безопасно удаляемое: временные файлы, распакованные natives, кеш каталога и скинов.
fn safe_dirs(data: &crate::paths::DataDir) -> [PathBuf; 4] {
    [data.tmp(), data.natives(), data.catalog(), data.skins()]
}

/// Сколько места занимают данные Quarry, по частям.
pub fn storage(data: &crate::paths::DataDir) -> Vec<StoragePart> {
    use crate::paths::dir_size;
    let safe: u64 = safe_dirs(data).iter().map(|d| dir_size(d)).sum();
    let mut parts = vec![
        StoragePart {
            label: "Профили (миры, моды, настройки)".into(),
            size: dir_size(&data.instances()),
            cleanable: false,
        },
        StoragePart {
            label: "Библиотеки и ресурсы игры (общие для профилей)".into(),
            size: dir_size(&data.libraries())
                + dir_size(&data.assets())
                + dir_size(&data.versions()),
            cleanable: false,
        },
        StoragePart {
            label: "Java".into(),
            size: dir_size(&data.runtimes()),
            cleanable: false,
        },
        StoragePart {
            label: "Бэкапы миров".into(),
            size: dir_size(&data.backups()),
            cleanable: false,
        },
        StoragePart {
            label: "Корзина (удалённые профили и файлы)".into(),
            size: dir_size(&data.trash()),
            cleanable: false,
        },
        StoragePart {
            label: "Временные файлы и кеш".into(),
            size: safe,
            cleanable: true,
        },
        StoragePart {
            label: "Логи лаунчера".into(),
            size: dir_size(&data.logs()),
            cleanable: false,
        },
    ];
    parts.retain(|p| p.size > 0);
    parts.sort_by_key(|p| std::cmp::Reverse(p.size));
    parts
}

/// Очистить только то, что скачается или соберётся заново. Миры, конфиги, библиотеки
/// и корзина не трогаются. Возвращает, сколько байт освобождено.
pub fn clean_cache(data: &crate::paths::DataDir) -> Result<u64> {
    let mut freed = 0;
    for dir in safe_dirs(data) {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            let size = if p.is_dir() {
                crate::paths::dir_size(&p)
            } else {
                e.metadata().map(|m| m.len()).unwrap_or(0)
            };
            let res = if p.is_dir() {
                std::fs::remove_dir_all(&p)
            } else {
                std::fs::remove_file(&p)
            };
            if res.is_ok() {
                freed += size;
            }
        }
    }
    Ok(freed)
}

pub fn write_zip(files: &[ReportFile], path: &Path) -> Result<()> {
    let f = std::fs::File::create(path).at(path)?;
    let mut z = zip::ZipWriter::new(f);
    let opts = zip::write::SimpleFileOptions::default();
    for file in files {
        z.start_file(file.name.as_str(), opts)
            .map_err(|e| Error::parse("архив отчёта", e))?;
        z.write_all(file.content.as_bytes()).at(path)?;
    }
    z.finish().map_err(|e| Error::parse("архив отчёта", e))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loader::LoaderKind;
    use crate::paths::DataDir;

    fn setup(name: &str) -> (InstanceStore, String, std::path::PathBuf) {
        let root = std::env::temp_dir().join(format!("quarry-diag-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let store = InstanceStore::new(DataDir::open(&root).unwrap());
        let inst = store
            .create("Сборка", "26.3", LoaderKind::Fabric, Some("0.19".into()))
            .unwrap();
        let game = store.game_dir(&inst.id).unwrap();
        std::fs::create_dir_all(game.join("crash-reports")).unwrap();
        std::fs::write(
            game.join("crash-reports/crash-2026-10-20_18.00.01-client.txt"),
            "---- Minecraft Crash Report ----\nDescription: Rendering overlay\n\njava.lang.NullPointerException\n",
        )
        .unwrap();
        std::fs::write(
            game.join("crash-reports/crash-2026-10-21_09.30.00-client.txt"),
            "Description: Unexpected error\n",
        )
        .unwrap();
        (store, inst.id, root)
    }

    #[test]
    fn crash_archive_newest_first() {
        let (store, id, root) = setup("crash");
        let game = store.game_dir(&id).unwrap();
        let list = crash_reports(&game);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].description, "Unexpected error");
        assert_eq!(list[1].when, "2026-10-20 18:00:01");
        assert!(
            read_crash_report(&game, &list[1].file)
                .unwrap()
                .contains("NullPointer")
        );
        assert!(read_crash_report(&game, "../instance.toml").is_err());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn report_is_masked_and_complete() {
        let (store, id, root) = setup("report");
        let home = dirs::home_dir().unwrap().to_string_lossy().into_owned();
        let logs = store.logs_dir(&id).unwrap();
        std::fs::create_dir_all(&logs).unwrap();
        std::fs::write(
            logs.join("game-1.log"),
            format!("[INFO] Steve_Builder joined from 93.184.216.34:25565\n[INFO] path {home}/x\n"),
        )
        .unwrap();
        let files = support_report(&store, &id, Some("Steve_Builder")).unwrap();
        let names: Vec<&str> = files.iter().map(|f| f.name.as_str()).collect();
        for n in [
            "summary.txt",
            "mods.txt",
            "launcher-log.txt",
            "crash-report.txt",
        ] {
            assert!(names.contains(&n), "{n} в {names:?}");
        }
        let all: String = files.iter().map(|f| f.content.as_str()).collect();
        assert!(all.contains("26.3") && all.contains("Fabric"));
        assert!(!all.contains("93.184.216.34"), "IP скрыт");
        assert!(!all.contains(&home), "домашняя папка скрыта");
        assert!(
            !all.contains("Steve_Builder"),
            "ник скрыт в режиме стримера"
        );
        assert!(all.contains("Unexpected error"), "последний отчёт о краше");

        let zip_path = root.join("report.zip");
        write_zip(&files, &zip_path).unwrap();
        let z = zip::ZipArchive::new(std::fs::File::open(&zip_path).unwrap()).unwrap();
        assert_eq!(z.len(), files.len());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn cleanup_removes_only_safe_cache() {
        let root = std::env::temp_dir().join(format!("quarry-clean-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let data = DataDir::open(&root).unwrap();
        for (p, n) in [
            (data.tmp().join("a.part"), 100usize),
            (data.natives().join("26.3/lwjgl.dylib"), 200),
            (data.catalog().join("x.json"), 50),
            (data.libraries().join("org/lib.jar"), 1000),
            (data.instances().join("p/game/saves/W/level.dat"), 10),
            (data.trash().join("old/game/options.txt"), 10),
        ] {
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, vec![0u8; n]).unwrap();
        }
        let before = storage(&data);
        assert!(before.iter().any(|p| p.cleanable && p.size >= 350));
        let freed = clean_cache(&data).unwrap();
        assert_eq!(freed, 350);
        assert!(!data.tmp().join("a.part").exists() && !data.natives().join("26.3").exists());
        // Библиотеки, миры и корзина не тронуты.
        assert!(data.libraries().join("org/lib.jar").exists());
        assert!(data.instances().join("p/game/saves/W/level.dat").exists());
        assert!(data.trash().join("old/game/options.txt").exists());
        std::fs::remove_dir_all(&root).unwrap();
    }
}
