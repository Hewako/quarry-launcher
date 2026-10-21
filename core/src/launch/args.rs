//! Сборка команды запуска из JSON версии: `arguments.jvm/game` с правилами и возможностями
//! (Quick Play, демо, разрешение окна) либо старый `minecraftArguments`. Подстановка
//! переменных `${…}` как у официального лаунчера.

use crate::install::Installed;
use crate::logmask;
use crate::metadata::Environment;
use crate::metadata::version::Argument;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Данные входа, которые получает игра. Токен не логируется и не показывается.
#[derive(Clone, Serialize, Deserialize)]
pub struct GameAuth {
    pub name: String,
    /// UUID без дефисов.
    pub uuid: String,
    pub access_token: String,
    /// `msa` для Microsoft, `mojang` для Yggdrasil-совместимых (Ely.by), `legacy` для dev-launch.
    pub user_type: String,
    pub xuid: String,
    pub client_id: String,
}

impl std::fmt::Debug for GameAuth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GameAuth")
            .field("name", &self.name)
            .field("uuid", &self.uuid)
            .field("access_token", &logmask::TOKEN)
            .field("user_type", &self.user_type)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "target", rename_all = "snake_case")]
pub enum QuickPlay {
    /// Имя папки мира в `saves/`.
    Singleplayer(String),
    /// `адрес[:порт]`.
    Multiplayer(String),
    Realms(String),
}

#[derive(Debug, Clone)]
pub struct LaunchSpec<'a> {
    pub installed: &'a Installed,
    pub java: &'a Path,
    pub game_dir: &'a Path,
    pub auth: &'a GameAuth,
    pub env: Environment,
    pub min_memory_mb: u32,
    pub max_memory_mb: u32,
    /// Пользовательские JVM-аргументы — после аргументов версии, чтобы их переопределять.
    pub extra_jvm_args: Vec<String>,
    pub resolution: Option<(u32, u32)>,
    pub quick_play: Option<QuickPlay>,
    /// `-javaagent:<jar>=<api>` для authlib-injector (только профили Ely.by).
    pub authlib_injector: Option<(PathBuf, String)>,
    pub launcher_name: &'a str,
    pub launcher_version: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchCommand {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    /// Переменные, которых не оказалось в таблице подстановки (для диагностики).
    pub unresolved: Vec<String>,
}

impl LaunchCommand {
    /// Команда для показа пользователю: токен скрыт, остальное как есть.
    pub fn preview(&self, auth: &GameAuth) -> String {
        let masker = logmask::Masker::new()
            .with_home(None)
            .with_secret(&auth.access_token);
        let mut parts = vec![quote(&self.program.to_string_lossy())];
        let mut hide_next = false;
        for a in &self.args {
            if hide_next {
                parts.push(logmask::TOKEN.to_string());
                hide_next = false;
                continue;
            }
            hide_next = a == "--accessToken" || a == "--session";
            parts.push(quote(&masker.mask(a)));
        }
        parts.join(" ")
    }
}

fn quote(s: &str) -> String {
    if s.is_empty() || s.contains([' ', '"', '\'']) {
        format!("\"{}\"", s.replace('"', "\\\""))
    } else {
        s.to_string()
    }
}

/// Подставить `${имя}`. Неизвестные переменные остаются как есть и попадают в `unresolved`.
pub fn substitute(
    arg: &str,
    vars: &BTreeMap<&str, String>,
    unresolved: &mut Vec<String>,
) -> String {
    let mut out = String::with_capacity(arg.len());
    let mut rest = arg;
    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        match after.find('}') {
            Some(end) => {
                let name = &after[..end];
                match vars.get(name) {
                    Some(v) => out.push_str(v),
                    None => {
                        out.push_str(&rest[start..start + 3 + end]);
                        if !unresolved.iter().any(|u| u == name) {
                            unresolved.push(name.to_string());
                        }
                    }
                }
                rest = &after[end + 1..];
            }
            None => {
                out.push_str(&rest[start..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

fn uses_feature(args: &[Argument], feature: &str) -> bool {
    args.iter().any(|a| match a {
        Argument::Conditional { rules, .. } => rules
            .iter()
            .any(|r| r.features.as_ref().is_some_and(|f| f.contains_key(feature))),
        _ => false,
    })
}

pub fn build(spec: &LaunchSpec) -> LaunchCommand {
    let inst = spec.installed;
    let version = &inst.version;
    let sep = spec.env.os.classpath_separator();
    let classpath = inst
        .classpath()
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join(sep);

    let game_args_json = version
        .arguments
        .as_ref()
        .map(|a| a.game.as_slice())
        .unwrap_or(&[]);
    let modern_quick_play = uses_feature(game_args_json, "is_quick_play_singleplayer");

    let mut env = spec.env.clone();
    if spec.resolution.is_some() {
        env.features.insert("has_custom_resolution".into());
    }
    let mut vars: BTreeMap<&str, String> = BTreeMap::new();
    let path = |p: &Path| p.to_string_lossy().into_owned();
    vars.insert("auth_player_name", spec.auth.name.clone());
    vars.insert("version_name", version.id.clone());
    vars.insert("game_directory", path(spec.game_dir));
    vars.insert("assets_root", path(&inst.assets_root));
    vars.insert("game_assets", path(&inst.game_assets));
    vars.insert("assets_index_name", inst.assets_index_name.clone());
    vars.insert("auth_uuid", spec.auth.uuid.clone());
    vars.insert("auth_access_token", spec.auth.access_token.clone());
    vars.insert(
        "auth_session",
        format!("token:{}:{}", spec.auth.access_token, spec.auth.uuid),
    );
    vars.insert("clientid", spec.auth.client_id.clone());
    vars.insert("auth_xuid", spec.auth.xuid.clone());
    vars.insert("user_type", spec.auth.user_type.clone());
    vars.insert("user_properties", "{}".into());
    vars.insert(
        "version_type",
        version.kind.clone().unwrap_or_else(|| "release".into()),
    );
    vars.insert("natives_directory", path(&inst.natives_dir));
    vars.insert("launcher_name", spec.launcher_name.to_string());
    vars.insert("launcher_version", spec.launcher_version.to_string());
    vars.insert("classpath", classpath);
    vars.insert("classpath_separator", sep.to_string());
    if let Some((w, h)) = spec.resolution {
        vars.insert("resolution_width", w.to_string());
        vars.insert("resolution_height", h.to_string());
    }
    if modern_quick_play {
        match &spec.quick_play {
            Some(QuickPlay::Singleplayer(w)) => {
                env.features.insert("is_quick_play_singleplayer".into());
                vars.insert("quickPlaySingleplayer", w.clone());
            }
            Some(QuickPlay::Multiplayer(s)) => {
                env.features.insert("is_quick_play_multiplayer".into());
                vars.insert("quickPlayMultiplayer", s.clone());
            }
            Some(QuickPlay::Realms(r)) => {
                env.features.insert("is_quick_play_realms".into());
                vars.insert("quickPlayRealms", r.clone());
            }
            None => {}
        }
    }
    // library_directory нужна только NeoForge/Forge; задаём честно — папку кеша библиотек.
    if let Some(lib_root) = library_root(inst) {
        vars.insert("library_directory", lib_root);
    }

    let mut unresolved = Vec::new();
    let mut args = Vec::new();

    // JVM: аргументы версии (или умолчания для старых версий).
    match version.arguments.as_ref().filter(|a| !a.jvm.is_empty()) {
        Some(a) => {
            for arg in &a.jvm {
                for v in arg.resolve(&env) {
                    args.push(substitute(&v, &vars, &mut unresolved));
                }
            }
        }
        None => {
            for v in [
                "-Djava.library.path=${natives_directory}",
                "-cp",
                "${classpath}",
            ] {
                args.push(substitute(v, &vars, &mut unresolved));
            }
        }
    }
    if let Some(logging) = &inst.logging_argument {
        args.push(logging.clone());
    }
    if let Some((jar, api)) = &spec.authlib_injector {
        args.push(format!("-javaagent:{}={api}", jar.to_string_lossy()));
    }
    args.push(format!(
        "-Xms{}M",
        spec.min_memory_mb.min(spec.max_memory_mb)
    ));
    args.push(format!("-Xmx{}M", spec.max_memory_mb));
    args.extend(spec.extra_jvm_args.iter().cloned());

    args.push(version.main_class.clone().unwrap_or_default());

    // Игра.
    if !game_args_json.is_empty() {
        for arg in game_args_json {
            for v in arg.resolve(&env) {
                args.push(substitute(&v, &vars, &mut unresolved));
            }
        }
    } else if let Some(legacy) = &version.minecraft_arguments {
        for token in legacy.split_whitespace() {
            args.push(substitute(token, &vars, &mut unresolved));
        }
        if let Some((w, h)) = spec.resolution {
            args.extend([
                "--width".into(),
                w.to_string(),
                "--height".into(),
                h.to_string(),
            ]);
        }
    }
    // Старые версии: вход на сервер через --server/--port.
    if !modern_quick_play && let Some(QuickPlay::Multiplayer(addr)) = &spec.quick_play {
        let (host, port) = match addr.rsplit_once(':') {
            Some((h, p)) if p.parse::<u16>().is_ok() => (h.to_string(), p.to_string()),
            _ => (addr.clone(), "25565".to_string()),
        };
        args.extend(["--server".into(), host, "--port".into(), port]);
    }

    LaunchCommand {
        program: spec.java.to_path_buf(),
        args,
        cwd: spec.game_dir.to_path_buf(),
        unresolved,
    }
}

/// Папка кеша библиотек — общий префикс путей библиотек (все лежат в cache/libraries).
fn library_root(inst: &Installed) -> Option<String> {
    let first = inst.libraries.first()?;
    first
        .ancestors()
        .find(|a| a.file_name().is_some_and(|n| n == "libraries"))
        .map(|p| p.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn substitution() {
        let mut vars = BTreeMap::new();
        vars.insert("a", "1".to_string());
        let mut un = Vec::new();
        assert_eq!(substitute("x${a}y${a}", &vars, &mut un), "x1y1");
        assert_eq!(
            substitute("${missing}-${a}", &vars, &mut un),
            "${missing}-1"
        );
        assert_eq!(un, ["missing"]);
        assert_eq!(substitute("broken ${", &vars, &mut un), "broken ${");
        assert_eq!(substitute("plain", &vars, &mut un), "plain");
    }

    #[test]
    fn debug_hides_token() {
        let auth = GameAuth {
            name: "Steve".into(),
            uuid: "u".into(),
            access_token: "secret-token-value".into(),
            user_type: "msa".into(),
            xuid: "0".into(),
            client_id: String::new(),
        };
        assert!(!format!("{auth:?}").contains("secret-token-value"));
    }
}
