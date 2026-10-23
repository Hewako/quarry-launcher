//! Golden-тесты команды запуска: каждая фикстура версии × каждая ОС. Без сети.
//! Эталоны лежат в `tests/golden/`. Обновить после осознанного изменения:
//! `UPDATE_GOLDEN=1 cargo test -p quarry-core --test golden`.

use quarry_core::download::{Cancel, Verify};
use quarry_core::install::{Installed, Installer};
use quarry_core::launch::args::{self, GameAuth, LaunchSpec, QuickPlay};
use quarry_core::metadata::{Arch, Environment, Os, VersionJson};
use quarry_core::paths::DataDir;
use std::path::{Path, PathBuf};

fn fixture(name: &str) -> VersionJson {
    let path = format!(
        "{}/tests/fixtures/versions/{name}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    VersionJson::parse(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn environments() -> Vec<(&'static str, Environment)> {
    vec![
        (
            "macos-arm64",
            Environment::new(Os::Osx, Arch::Arm64, "15.1"),
        ),
        (
            "windows-x64",
            Environment::new(Os::Windows, Arch::X86_64, "10.0.19045"),
        ),
        (
            "linux-x64",
            Environment::new(Os::Linux, Arch::X86_64, "6.8.0"),
        ),
    ]
}

fn auth() -> GameAuth {
    GameAuth {
        name: "Steve".into(),
        uuid: "0123456789abcdef0123456789abcdef".into(),
        access_token: "SECRET-ACCESS-TOKEN-0123456789".into(),
        user_type: "msa".into(),
        xuid: "0".into(),
        client_id: "client-id".into(),
    }
}

/// Пути одинаковые на всех ОС, чтобы эталоны не зависели от машины.
fn installed(version: VersionJson, game_version: &str, env: &Environment) -> Installed {
    let data = DataDir::new("/q");
    let installer = Installer {
        data: &data,
        env: env.clone(),
        verify: Verify::Quick,
        cancel: Cancel::new(),
        on_event: None,
    };
    let plan = installer.plan(&version, game_version).unwrap();
    Installed {
        assets_index_name: plan.index_ref.id.clone(),
        version,
        client_jar: plan.client_jar,
        libraries: plan.libraries,
        natives_dir: PathBuf::from("/q/cache/natives/test"),
        assets_root: PathBuf::from("/q/cache/assets"),
        game_assets: PathBuf::from("/q/cache/assets"),
        logging_argument: plan.logging_argument,
        unverified: vec![],
    }
}

/// Разделители путей хоста приводятся к `/`: эталоны одинаковы при прогоне на любой ОС
/// (целевая ОС задаётся Environment, а не машиной, где идёт тест).
fn render(cmd: &args::LaunchCommand) -> String {
    let mut out = format!(
        "program: {}\ncwd: {}\n",
        cmd.program.display().to_string().replace('\\', "/"),
        cmd.cwd.display().to_string().replace('\\', "/")
    );
    for a in &cmd.args {
        out.push_str(&a.replace('\\', "/"));
        out.push('\n');
    }
    out
}

fn build(
    name: &str,
    game: &str,
    env: &Environment,
    quick: Option<QuickPlay>,
) -> args::LaunchCommand {
    let version = match name.strip_prefix("fabric-loader-") {
        Some(g) => fixture(name).merged_onto(fixture(g)),
        None => fixture(name),
    };
    let inst = installed(version, game, env);
    let a = auth();
    let spec = LaunchSpec {
        installed: &inst,
        java: Path::new("/java/bin/java"),
        game_dir: Path::new("/q/instances/test/game"),
        auth: &a,
        env: env.clone(),
        min_memory_mb: 4096,
        max_memory_mb: 4096,
        extra_jvm_args: vec!["-XX:+UseG1GC".into()],
        resolution: None,
        extra_game_args: vec![],
        extra_env: vec![],
        quick_play: quick,
        authlib_injector: None,
        launcher_name: "Axel",
        launcher_version: "0.1.0",
    };
    args::build(&spec)
}

fn check_golden(file: &str, actual: &str) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(file);
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, actual).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("нет эталона {file}: запустите с UPDATE_GOLDEN=1"));
    assert_eq!(expected, actual, "команда запуска {file} изменилась");
}

const CASES: [(&str, &str); 7] = [
    ("26.3", "26.3"),
    ("1.21.11", "1.21.11"),
    ("1.20.1", "1.20.1"),
    ("1.16.5", "1.16.5"),
    ("1.12.2", "1.12.2"),
    ("1.6.4", "1.6.4"),
    ("fabric-loader-26.3", "26.3"),
];

#[test]
fn golden_launch_commands() {
    for (name, game) in CASES {
        for (os_name, env) in environments() {
            let cmd = build(name, game, &env, None);
            assert!(
                cmd.unresolved.is_empty(),
                "{name}/{os_name}: {:?}",
                cmd.unresolved
            );
            check_golden(&format!("{name}-{os_name}.txt"), &render(&cmd));
        }
    }
}

#[test]
fn every_command_has_main_class_classpath_and_token_only_once() {
    for (name, game) in CASES {
        for (os_name, env) in environments() {
            let cmd = build(name, game, &env, None);
            let main = cmd
                .args
                .iter()
                .position(|a| a.contains("net.minecraft") || a.contains("KnotClient"));
            assert!(main.is_some(), "{name}/{os_name}: нет главного класса");
            let cp = cmd.args.iter().position(|a| a == "-cp").expect("есть -cp");
            assert!(cp < main.unwrap(), "classpath до главного класса");
            let sep = if env.os == Os::Windows { ";" } else { ":" };
            assert!(
                cmd.args[cp + 1].contains(sep),
                "{name}/{os_name}: разделитель classpath"
            );
            assert!(
                cmd.args[cp + 1].ends_with(&format!("{game}.jar")),
                "клиент в конце classpath"
            );
            let token_hits = cmd
                .args
                .iter()
                .filter(|a| a.contains("SECRET-ACCESS-TOKEN"))
                .count();
            assert_eq!(
                token_hits, 1,
                "{name}/{os_name}: токен ровно в одном аргументе"
            );
            let preview = cmd.preview(&auth());
            assert!(
                !preview.contains("SECRET-ACCESS-TOKEN"),
                "в предпросмотре токена нет"
            );
            let start_first = cmd.args.iter().any(|a| a == "-XstartOnFirstThread");
            // -XstartOnFirstThread только на macOS и только если его требует сама версия (LWJGL 3).
            let lwjgl3 = !matches!(name, "1.12.2" | "1.6.4");
            assert_eq!(start_first, env.os == Os::Osx && lwjgl3, "{name}/{os_name}");
        }
    }
}

#[test]
fn log4shell_protection_for_vulnerable_versions() {
    for (name, game) in [("1.16.5", "1.16.5"), ("1.12.2", "1.12.2")] {
        let cmd = build(name, game, &environments()[0].1, None);
        assert!(
            cmd.args
                .iter()
                .any(|a| a.starts_with("-Dlog4j.configurationFile=")
                    && a.contains("client-1.12.xml")),
            "{name}: logging-конфиг из JSON версии подставлен"
        );
    }
}

#[test]
fn quick_play_modern_and_legacy() {
    let env = &environments()[2].1;
    let cmd = build(
        "26.3",
        "26.3",
        env,
        Some(QuickPlay::Singleplayer("База у реки".into())),
    );
    let i = cmd
        .args
        .iter()
        .position(|a| a == "--quickPlaySingleplayer")
        .unwrap();
    assert_eq!(cmd.args[i + 1], "База у реки");
    let cmd = build(
        "1.12.2",
        "1.12.2",
        env,
        Some(QuickPlay::Multiplayer("play.example.net:25570".into())),
    );
    let i = cmd.args.iter().position(|a| a == "--server").unwrap();
    assert_eq!(
        &cmd.args[i + 1..i + 4],
        ["play.example.net", "--port", "25570"]
    );
}

#[test]
fn legacy_1_6_4_uses_session_and_game_assets() {
    let cmd = build("1.6.4", "1.6.4", &environments()[2].1, None);
    let i = cmd.args.iter().position(|a| a == "--session").unwrap();
    assert!(cmd.args[i + 1].starts_with("token:SECRET"));
    assert!(
        cmd.args
            .iter()
            .any(|a| a.starts_with("-Djava.library.path=")),
        "умолчания JVM для старых версий"
    );
}
