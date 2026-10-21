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
    /// Дополнительные аргументы игры (после аргументов версии).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub game_args: Option<Vec<String>>,
    /// Переменные окружения для процесса игры.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env: Option<std::collections::BTreeMap<String, String>>,
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
    pub game_args: Resolved<Vec<String>>,
    pub env: Resolved<std::collections::BTreeMap<String, String>>,
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
        game_args: pick(
            profile.game_args.as_ref(),
            pack.game_args.as_ref(),
            global.game_args.as_ref(),
            Vec::new(),
        ),
        env: pick(
            profile.env.as_ref(),
            pack.env.as_ref(),
            global.env.as_ref(),
            Default::default(),
        ),
    }
}

/// Аргументы игры, которые задаёт сам лаунчер: их подмена сломала бы вход или изоляцию.
const MANAGED_GAME_ARGS: [&str; 9] = [
    "--accessToken",
    "--uuid",
    "--username",
    "--userType",
    "--xuid",
    "--clientId",
    "--gameDir",
    "--assetsDir",
    "--version",
];

/// Переменные окружения, через которые в Java или процесс можно подгрузить чужой код.
fn env_forbidden(name: &str) -> bool {
    matches!(
        name,
        "JAVA_TOOL_OPTIONS" | "_JAVA_OPTIONS" | "JDK_JAVA_OPTIONS" | "CLASSPATH" | "JAVA_HOME"
    ) || name.starts_with("LD_")
        || name.starts_with("DYLD_")
}

/// Советы по памяти: доля от ОЗУ компьютера, рекомендация сборки, крайние значения.
pub fn memory_advice(max_mb: u32, total_mb: Option<u64>, pack_mb: Option<u32>) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(total) = total_mb
        && u64::from(max_mb) * 4 > total * 3
    {
        out.push(format!(
            "Отдано {} из {} ГБ памяти компьютера — системе и браузеру может не хватить, игра начнёт тормозить",
            max_mb / 1024,
            total / 1024
        ));
    }
    if let Some(p) = pack_mb
        && max_mb < p
    {
        out.push(format!("Сборка рекомендует не меньше {p} МБ"));
    }
    if max_mb < 2048 {
        out.push(
            "Меньше 2 ГБ — мало для современных версий: возможны вылеты из-за нехватки памяти"
                .into(),
        );
    }
    if max_mb > 16_384 {
        out.push(
            "Больше 16 ГБ почти никогда не нужно: длинные паузы сборщика мусора дают подлагивания"
                .into(),
        );
    }
    out
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
    if let Some(args) = &o.game_args
        && let Some(a) = args
            .iter()
            .find(|a| MANAGED_GAME_ARGS.contains(&a.as_str()))
    {
        return Err(Error::invalid(format!(
            "Аргумент {a} задаёт сам лаунчер — его нельзя менять"
        )));
    }
    if let Some(env) = &o.env {
        for name in env.keys() {
            let valid = name
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
                && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
            if !valid {
                return Err(Error::invalid(format!(
                    "Имя переменной {name:?}: только латинские буквы, цифры и _, не с цифры"
                )));
            }
            if env_forbidden(name) {
                return Err(Error::invalid(format!(
                    "Переменную {name} задавать нельзя: через неё в игру можно подгрузить посторонний код"
                )));
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
    /// Компактный режим: меньше отступов.
    #[serde(default)]
    pub compact: bool,
    /// Мастер первого запуска пройден.
    #[serde(default)]
    pub first_run_done: bool,
    /// Бэкап последнего мира перед «рискованным» запуском (моды менялись после прошлой игры).
    #[serde(default = "yes")]
    pub backup_before_risky_launch: bool,
    /// Бэкап последнего мира после выхода из игры.
    #[serde(default)]
    pub backup_after_exit: bool,
    /// Бэкап по расписанию: раз в N дней перед запуском, если в мир играли. 0 — выключено.
    #[serde(default)]
    pub backup_every_days: u32,
    /// Ограничение скорости загрузок, КБ/с; 0 — без ограничения.
    #[serde(default)]
    pub download_limit_kbps: u32,
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
            compact: false,
            first_run_done: false,
            backup_before_risky_launch: true,
            backup_after_exit: false,
            backup_every_days: 0,
            download_limit_kbps: 0,
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

    #[test]
    fn game_args_and_env_are_checked() {
        let ok = LaunchOverrides {
            game_args: Some(vec!["--fullscreen".into()]),
            env: Some([("MESA_GL_VERSION_OVERRIDE".to_string(), "4.5".to_string())].into()),
            ..Default::default()
        };
        assert!(validate(&ok).is_ok());
        for bad in ["--accessToken", "--uuid", "--gameDir", "--username"] {
            let o = LaunchOverrides {
                game_args: Some(vec![bad.into(), "x".into()]),
                ..Default::default()
            };
            assert!(validate(&o).is_err(), "{bad}");
        }
        for bad in [
            "JAVA_TOOL_OPTIONS",
            "_JAVA_OPTIONS",
            "JDK_JAVA_OPTIONS",
            "LD_PRELOAD",
            "DYLD_INSERT_LIBRARIES",
            "1BAD",
            "A B",
        ] {
            let o = LaunchOverrides {
                env: Some([(bad.to_string(), "x".to_string())].into()),
                ..Default::default()
            };
            assert!(validate(&o).is_err(), "{bad}");
        }
    }

    #[test]
    fn memory_advice_warns_on_extremes() {
        // Почти вся память компьютера.
        assert!(
            memory_advice(15_000, Some(16_384), None)
                .iter()
                .any(|w| w.contains("системе"))
        );
        // Сборка рекомендует больше.
        assert!(
            memory_advice(4096, Some(32_768), Some(8192))
                .iter()
                .any(|w| w.contains("8192"))
        );
        // Слишком мало для современной игры.
        assert!(
            memory_advice(1024, Some(16_384), None)
                .iter()
                .any(|w| w.contains("мало"))
        );
        // Очень много — паузы сборщика мусора.
        assert!(
            memory_advice(24_576, Some(65_536), None)
                .iter()
                .any(|w| w.contains("16 ГБ"))
        );
        assert!(memory_advice(6144, Some(16_384), None).is_empty());
    }
}
