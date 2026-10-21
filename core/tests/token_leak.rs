//! Токен не попадает ни в файл лога, ни в события интерфейса, ни в предпросмотр команды —
//! даже если «игра» печатает все свои аргументы. Вместо Java — скрипт, который выводит argv.
#![cfg(unix)]

use quarry_core::download::{Cancel, Verify};
use quarry_core::install::{Installed, Installer};
use quarry_core::launch::args::{self, GameAuth, LaunchSpec};
use quarry_core::launch::process::{self, GameEvent};
use quarry_core::logmask::Masker;
use quarry_core::metadata::{Environment, VersionJson};
use quarry_core::paths::DataDir;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

const TOKEN: &str = "eyJhbGciOiJIUzI1NiJ9.SECRET-PAYLOAD-0123456789.signature-xyz";

#[tokio::test]
async fn token_never_reaches_logs_events_or_preview() {
    let root = std::env::temp_dir().join(format!("quarry-leak-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();

    // «Java», печатающая все аргументы и ещё раз токен в stderr.
    let fake_java = root.join("java");
    std::fs::write(&fake_java, "#!/bin/sh\nfor a in \"$@\"; do echo \"arg: $a\"; done\necho \"Session token=$QUARRY_T\" >&2\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&fake_java, std::fs::Permissions::from_mode(0o755)).unwrap();

    let fixture = format!(
        "{}/tests/fixtures/versions/26.3.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let version = VersionJson::parse(&std::fs::read_to_string(fixture).unwrap()).unwrap();
    let data = DataDir::new(root.join("data"));
    let env = Environment::current();
    let plan = Installer {
        data: &data,
        env: env.clone(),
        verify: Verify::Quick,
        cancel: Cancel::new(),
        on_event: None,
    }
    .plan(&version, "26.3")
    .unwrap();
    let installed = Installed {
        assets_index_name: plan.index_ref.id.clone(),
        version,
        client_jar: plan.client_jar,
        libraries: plan.libraries,
        natives_dir: root.join("natives"),
        assets_root: root.join("assets"),
        game_assets: root.join("assets"),
        logging_argument: plan.logging_argument,
        unverified: vec![],
    };
    let auth = GameAuth {
        name: "Steve".into(),
        uuid: "0123456789abcdef0123456789abcdef".into(),
        access_token: TOKEN.into(),
        user_type: "msa".into(),
        xuid: "0".into(),
        client_id: String::new(),
    };
    let game_dir = root.join("game");
    let cmd = args::build(&LaunchSpec {
        installed: &installed,
        java: &fake_java,
        game_dir: &game_dir,
        auth: &auth,
        env,
        min_memory_mb: 1024,
        max_memory_mb: 1024,
        extra_jvm_args: vec![],
        resolution: None,
        quick_play: None,
        authlib_injector: None,
        launcher_name: "Quarry",
        launcher_version: "test",
    });
    assert!(cmd.args.iter().any(|a| a == TOKEN), "игра получает токен");
    assert!(
        !cmd.preview(&auth).contains("SECRET-PAYLOAD"),
        "предпросмотр без токена"
    );

    let events: Arc<Mutex<Vec<String>>> = Arc::default();
    let ev = events.clone();
    let log_file: PathBuf = root.join("logs/game.log");
    // Переменная окружения процесса тоже содержит токен — проверяем вывод из stderr.
    unsafe { std::env::set_var("QUARRY_T", TOKEN) };
    let proc = process::spawn(
        &cmd,
        &log_file,
        Masker::new().with_secret(TOKEN),
        Some(Arc::new(move |e| {
            if let GameEvent::Log { line, .. } = e {
                ev.lock().unwrap().push(line);
            }
        })),
    )
    .await
    .unwrap();
    let info = proc.wait().await.unwrap();
    assert_eq!(info.code, Some(0));

    let text = std::fs::read_to_string(&log_file).unwrap();
    assert!(
        text.contains("arg: --accessToken"),
        "лог действительно содержит вывод"
    );
    assert!(!text.contains("SECRET-PAYLOAD"), "токена нет в файле лога");
    let events = events.lock().unwrap();
    assert!(!events.is_empty());
    assert!(
        events.iter().all(|l| !l.contains("SECRET-PAYLOAD")),
        "токена нет в событиях"
    );
    assert!(
        info.tail.iter().all(|l| !l.contains("SECRET-PAYLOAD")),
        "и в хвосте для разбора краша"
    );
    std::fs::remove_dir_all(&root).unwrap();
}
