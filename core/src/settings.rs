//! Настройки запуска и их наследование: профиль → сборка → глобальные → умолчание.
//! У каждого итогового значения известен источник (показывается в интерфейсе).

use crate::error::{Error, Result};
use crate::paths::{DataDir, write_atomic};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Переопределения на одном уровне. `None` — «наследовать».
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaunchOverrides {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_memory_mb: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_memory_mb: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub java_path: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jvm_args: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution: Option<(u32, u32)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Profile,
    Pack,
    Global,
    Default,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Resolved<T> {
    pub value: T,
    pub source: Source,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResolvedLaunch {
    pub max_memory_mb: Resolved<u32>,
    pub min_memory_mb: Resolved<u32>,
    pub java_path: Resolved<Option<PathBuf>>,
    pub jvm_args: Resolved<Vec<String>>,
    pub resolution: Resolved<Option<(u32, u32)>>,
}

pub const DEFAULT_MAX_MEMORY_MB: u32 = 4096;

fn pick<T: Clone>(
    profile: Option<&T>,
    pack: Option<&T>,
    global: Option<&T>,
    default: T,
) -> Resolved<T> {
    if let Some(v) = profile {
        return Resolved {
            value: v.clone(),
            source: Source::Profile,
        };
    }
    if let Some(v) = pack {
        return Resolved {
            value: v.clone(),
            source: Source::Pack,
        };
    }
    if let Some(v) = global {
        return Resolved {
            value: v.clone(),
            source: Source::Global,
        };
    }
    Resolved {
        value: default,
        source: Source::Default,
    }
}

pub fn resolve(
    profile: &LaunchOverrides,
    pack: &LaunchOverrides,
    global: &LaunchOverrides,
) -> ResolvedLaunch {
    let max = pick(
        profile.max_memory_mb.as_ref(),
        pack.max_memory_mb.as_ref(),
        global.max_memory_mb.as_ref(),
        DEFAULT_MAX_MEMORY_MB,
    );
    // Xms по умолчанию равен Xmx: куча не перевыделяется посреди игры.
    let min = pick(
        profile.min_memory_mb.as_ref(),
        pack.min_memory_mb.as_ref(),
        global.min_memory_mb.as_ref(),
        max.value,
    );
    let min = Resolved {
        value: min.value.min(max.value),
        source: min.source,
    };
    ResolvedLaunch {
        max_memory_mb: max,
        min_memory_mb: min,
        java_path: pick(
            profile.java_path.as_ref().map(|p| Some(p.clone())).as_ref(),
            pack.java_path.as_ref().map(|p| Some(p.clone())).as_ref(),
            global.java_path.as_ref().map(|p| Some(p.clone())).as_ref(),
            None,
        ),
        jvm_args: pick(
            profile.jvm_args.as_ref(),
            pack.jvm_args.as_ref(),
            global.jvm_args.as_ref(),
            Vec::new(),
        ),
        resolution: pick(
            profile.resolution.map(Some).as_ref(),
            pack.resolution.map(Some).as_ref(),
            global.resolution.map(Some).as_ref(),
            None,
        ),
    }
}

/// Проверка значений, введённых пользователем.
pub fn validate(o: &LaunchOverrides) -> Result<()> {
    if let Some(m) = o.max_memory_mb
        && !(512..=262_144).contains(&m)
    {
        return Err(Error::invalid("Память: от 512 МБ до 256 ГБ"));
    }
    if let (Some(min), Some(max)) = (o.min_memory_mb, o.max_memory_mb)
        && min > max
    {
        return Err(Error::invalid(
            "Начальная память (Xms) больше максимальной (Xmx)",
        ));
    }
    if let Some(args) = &o.jvm_args {
        for a in args {
            if a.starts_with("-Xmx") || a.starts_with("-Xms") {
                return Err(Error::invalid(
                    "Память задаётся ползунком, а не аргументами -Xmx/-Xms",
                ));
            }
            if a.starts_with("-javaagent") {
                return Err(Error::invalid(
                    "-javaagent в аргументах запрещён: агенты подключает только лаунчер (authlib-injector для Ely.by)",
                ));
            }
        }
    }
    if let Some((w, h)) = o.resolution
        && (w < 320 || h < 240)
    {
        return Err(Error::invalid("Разрешение окна меньше 320×240"));
    }
    Ok(())
}

/// Глобальные настройки лаунчера (`launcher.toml`). Токенов здесь нет.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalSettings {
    #[serde(default)]
    pub launch: LaunchOverrides,
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default)]
    pub streamer_mode: bool,
    /// Аккаунт по умолчанию: `провайдер:uuid`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_account: Option<String>,
    /// Язык интерфейса: `ru` или `en`.
    #[serde(default = "default_language")]
    pub language: String,
    /// Акцентный цвет интерфейса (CSS-цвет) и масштаб (проценты).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accent: Option<String>,
    #[serde(default = "default_scale")]
    pub ui_scale: u32,
    /// Мастер первого запуска пройден.
    #[serde(default)]
    pub first_run_done: bool,
    /// Бэкап последнего мира перед «рискованным» запуском (моды менялись после прошлой игры).
    #[serde(default = "yes")]
    pub backup_before_risky_launch: bool,
    /// Бэкап последнего мира после выхода из игры.
    #[serde(default)]
    pub backup_after_exit: bool,
    /// Проверять, вышла ли новая версия Quarry (страница релизов GitHub).
    #[serde(default = "yes")]
    pub check_launcher_updates: bool,
}

fn default_theme() -> String {
    "dark".into()
}
fn default_language() -> String {
    "ru".into()
}
fn default_scale() -> u32 {
    100
}
fn yes() -> bool {
    true
}

impl Default for GlobalSettings {
    fn default() -> Self {
        Self {
            launch: LaunchOverrides::default(),
            theme: default_theme(),
            streamer_mode: false,
            default_account: None,
            language: default_language(),
            accent: None,
            ui_scale: default_scale(),
            first_run_done: false,
            backup_before_risky_launch: true,
            backup_after_exit: false,
            check_launcher_updates: true,
        }
    }
}

impl GlobalSettings {
    /// Прочитать настройки. Битый файл не ломает запуск: берутся умолчания, а файл
    /// сохраняется рядом как `launcher.toml.broken-…`.
    pub fn load(data: &DataDir) -> Result<Self> {
        let path = data.launcher_toml();
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(Error::io(&path, e)),
        };
        match toml::from_str(&text) {
            Ok(s) => Ok(s),
            Err(_) => {
                let aside = crate::paths::unique_sibling(&path, "broken");
                let _ = std::fs::rename(&path, aside);
                Ok(Self::default())
            }
        }
    }

    pub fn save(&self, data: &DataDir) -> Result<()> {
        validate(&self.launch)?;
        let text = toml::to_string_pretty(self).map_err(|e| Error::parse("launcher.toml", e))?;
        write_atomic(&data.launcher_toml(), text.as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearest_level_wins_and_source_is_known() {
        let profile = LaunchOverrides {
            max_memory_mb: Some(8192),
            ..Default::default()
        };
        let pack = LaunchOverrides {
            max_memory_mb: Some(6144),
            jvm_args: Some(vec!["-XX:+UseG1GC".into()]),
            ..Default::default()
        };
        let global = LaunchOverrides {
            java_path: Some("/opt/java".into()),
            jvm_args: Some(vec!["-Dglobal".into()]),
            ..Default::default()
        };
        let r = resolve(&profile, &pack, &global);
        assert_eq!(
            r.max_memory_mb,
            Resolved {
                value: 8192,
                source: Source::Profile
            }
        );
        assert_eq!(r.jvm_args.source, Source::Pack);
        assert_eq!(
            r.java_path.value.as_deref(),
            Some(std::path::Path::new("/opt/java"))
        );
        assert_eq!(r.java_path.source, Source::Global);
        assert_eq!(r.resolution.source, Source::Default);
        assert_eq!(r.min_memory_mb.value, 8192, "Xms = Xmx по умолчанию");
    }

    #[test]
    fn defaults_without_overrides() {
        let r = resolve(
            &Default::default(),
            &Default::default(),
            &Default::default(),
        );
        assert_eq!(r.max_memory_mb.value, DEFAULT_MAX_MEMORY_MB);
        assert_eq!(r.max_memory_mb.source, Source::Default);
        assert!(r.jvm_args.value.is_empty());
    }

    #[test]
    fn validation() {
        assert!(
            validate(&LaunchOverrides {
                max_memory_mb: Some(100),
                ..Default::default()
            })
            .is_err()
        );
        assert!(
            validate(&LaunchOverrides {
                min_memory_mb: Some(4096),
                max_memory_mb: Some(2048),
                ..Default::default()
            })
            .is_err()
        );
        assert!(
            validate(&LaunchOverrides {
                jvm_args: Some(vec!["-Xmx2G".into()]),
                ..Default::default()
            })
            .is_err()
        );
        assert!(
            validate(&LaunchOverrides {
                jvm_args: Some(vec!["-javaagent:x.jar".into()]),
                ..Default::default()
            })
            .is_err()
        );
        assert!(
            validate(&LaunchOverrides {
                jvm_args: Some(vec!["-XX:+UseZGC".into()]),
                ..Default::default()
            })
            .is_ok()
        );
    }

    #[test]
    fn broken_settings_file_is_kept_aside() {
        let root = std::env::temp_dir().join(format!("quarry-settings-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let data = DataDir::new(&root);
        std::fs::write(data.launcher_toml(), "not = [valid").unwrap();
        let s = GlobalSettings::load(&data).unwrap();
        assert_eq!(s.theme, "dark");
        assert!(
            std::fs::read_dir(&root)
                .unwrap()
                .flatten()
                .any(|e| e.file_name().to_string_lossy().contains("broken"))
        );
        std::fs::remove_dir_all(&root).unwrap();
    }
}
