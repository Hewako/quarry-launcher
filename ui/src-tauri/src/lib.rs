//! Тонкие команды Tauri поверх quarry-core. Логики здесь нет: только преобразование
//! ошибок в текст для интерфейса и пересылка событий запуска.

use quarry_core::account::{AccountRef, AccountStore, KeyringStore};
use quarry_core::download::Cancel;
use quarry_core::instance::{AuthMode, BrokenInstance, Instance, InstanceStore};
use quarry_core::launch::{self, LaunchEvent, LaunchOptions, LaunchRegistry, args::QuickPlay};
use quarry_core::loader::{self, LoaderKind, LoaderVersion};
use quarry_core::metadata::{self, Freshness};
use quarry_core::paths::DataDir;
use quarry_core::settings::{self, GlobalSettings, LaunchOverrides, ResolvedLaunch};
use serde::Serialize;
use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager, State};
use tauri_plugin_opener::OpenerExt;

struct AppState {
    data: DataDir,
    registry: LaunchRegistry,
    /// Текущий вход (Microsoft device code или Ely.by), чтобы его можно было отменить.
    login_cancel: Mutex<Option<Cancel>>,
}

type CmdResult<T> = Result<T, String>;

fn err(e: quarry_core::Error) -> String {
    e.to_string()
}

#[derive(Serialize)]
struct AppInfo {
    version: &'static str,
    dev_launch: bool,
    data_dir: String,
    microsoft_configured: bool,
    os: &'static str,
}

#[tauri::command]
fn app_info(state: State<AppState>) -> AppInfo {
    AppInfo {
        version: launch::LAUNCHER_VERSION,
        dev_launch: launch::DEV_LAUNCH,
        data_dir: state.data.root().display().to_string(),
        microsoft_configured: quarry_core::account::microsoft::CLIENT_ID.is_some(),
        os: std::env::consts::OS,
    }
}

#[derive(Serialize)]
struct InstanceList {
    instances: Vec<Instance>,
    broken: Vec<BrokenInstance>,
    running: Vec<String>,
}

#[tauri::command]
fn list_instances(state: State<AppState>) -> CmdResult<InstanceList> {
    let (instances, broken) = InstanceStore::new(state.data.clone()).list().map_err(err)?;
    Ok(InstanceList {
        instances,
        broken,
        running: state.registry.running_ids(),
    })
}

#[tauri::command]
async fn create_instance(
    state: State<'_, AppState>,
    name: String,
    game_version: String,
    loader: String,
    loader_version: Option<String>,
) -> CmdResult<Instance> {
    let kind = LoaderKind::parse(&loader).map_err(err)?;
    // Совместимость версии игры и загрузчика проверяется до создания и скачивания.
    let lv = loader::resolve_version(kind, &game_version, loader_version.as_deref())
        .await
        .map_err(err)?;
    InstanceStore::new(state.data.clone())
        .create(&name, &game_version, kind, lv)
        .map_err(err)
}

#[tauri::command]
fn trash_instance(state: State<AppState>, id: String) -> CmdResult<String> {
    if state.registry.is_running(&id) {
        return Err("Сначала закройте игру в этом профиле".into());
    }
    InstanceStore::new(state.data.clone())
        .trash(&id)
        .map(|p| p.display().to_string())
        .map_err(err)
}

#[tauri::command]
fn update_instance(
    state: State<AppState>,
    id: String,
    name: Option<String>,
    settings: Option<LaunchOverrides>,
    auth_mode: Option<AuthMode>,
    // `None` — не менять, пустая строка — аккаунт по умолчанию.
    account: Option<String>,
) -> CmdResult<Instance> {
    let store = InstanceStore::new(state.data.clone());
    let mut inst = store.get(&id).map_err(err)?;
    if let Some(n) = name {
        let n = n.trim();
        if n.is_empty() {
            return Err("Имя профиля не может быть пустым".into());
        }
        inst.name = n.to_string();
    }
    if let Some(s) = settings {
        inst.settings = s;
    }
    if let Some(m) = auth_mode {
        if m == AuthMode::ElyBy {
            AccountStore::new(&state.data, &KeyringStore)
                .ensure_ely_allowed()
                .map_err(err)?;
        }
        inst.auth_mode = m;
    }
    if let Some(a) = account {
        inst.account = (!a.is_empty()).then_some(a);
    }
    store.save(&inst).map_err(err)?;
    Ok(inst)
}

#[derive(Serialize)]
struct LaunchSettingsView {
    resolved: ResolvedLaunch,
    profile: LaunchOverrides,
    global: LaunchOverrides,
    total_memory_mb: Option<u64>,
}

#[tauri::command]
fn launch_settings(state: State<AppState>, id: String) -> CmdResult<LaunchSettingsView> {
    let inst = InstanceStore::new(state.data.clone())
        .get(&id)
        .map_err(err)?;
    let global = GlobalSettings::load(&state.data).map_err(err)?;
    Ok(LaunchSettingsView {
        resolved: settings::resolve(&inst.settings, &inst.pack_settings, &global.launch),
        profile: inst.settings,
        global: global.launch,
        total_memory_mb: total_memory_mb(),
    })
}

/// Объём ОЗУ для подсказки по памяти. Не подтверждено для всех ОС: macOS — sysctl,
/// Linux — /proc/meminfo, Windows — пока нет (подсказка без числа).
fn total_memory_mb() -> Option<u64> {
    if cfg!(target_os = "macos") {
        let out = std::process::Command::new("sysctl")
            .args(["-n", "hw.memsize"])
            .output()
            .ok()?;
        String::from_utf8_lossy(&out.stdout)
            .trim()
            .parse::<u64>()
            .ok()
            .map(|b| b / 1_048_576)
    } else if cfg!(target_os = "linux") {
        let text = std::fs::read_to_string("/proc/meminfo").ok()?;
        let kb: u64 = text
            .lines()
            .find(|l| l.starts_with("MemTotal:"))?
            .split_whitespace()
            .nth(1)?
            .parse()
            .ok()?;
        Some(kb / 1024)
    } else {
        None
    }
}

#[tauri::command]
fn global_settings(state: State<AppState>) -> CmdResult<GlobalSettings> {
    GlobalSettings::load(&state.data).map_err(err)
}

#[tauri::command]
fn save_global_settings(state: State<AppState>, settings: GlobalSettings) -> CmdResult<()> {
    settings.save(&state.data).map_err(err)
}

#[derive(Serialize)]
struct VersionList {
    versions: Vec<metadata::VersionEntry>,
    latest_release: String,
    offline: bool,
}

#[tauri::command]
async fn list_versions(
    state: State<'_, AppState>,
    snapshots: bool,
    old: bool,
) -> CmdResult<VersionList> {
    let (m, fresh) = metadata::manifest(&state.data).await.map_err(err)?;
    Ok(VersionList {
        versions: m.filtered(snapshots, old).into_iter().cloned().collect(),
        latest_release: m.latest.release.clone(),
        offline: fresh == Freshness::Cached,
    })
}

#[derive(Serialize)]
struct VersionInfo {
    java_major: u32,
    java_reason: String,
}

#[tauri::command]
async fn version_info(state: State<'_, AppState>, id: String) -> CmdResult<VersionInfo> {
    let v = metadata::vanilla_version(&state.data, &id)
        .await
        .map_err(err)?;
    let (java_major, java_reason) = v.required_java();
    Ok(VersionInfo {
        java_major,
        java_reason,
    })
}

#[tauri::command]
async fn loader_versions(loader: String, game_version: String) -> CmdResult<Vec<LoaderVersion>> {
    let kind = LoaderKind::parse(&loader).map_err(err)?;
    loader::versions(kind, &game_version).await.map_err(err)
}

#[derive(Clone, Serialize)]
struct LaunchPayload {
    id: String,
    event: LaunchEvent,
}

#[derive(Clone, Serialize)]
struct LaunchFinished {
    id: String,
    error: Option<String>,
}

/// Запуск идёт в фоне; ход и итог приходят событиями `launch-event` и `launch-finished`.
#[tauri::command]
fn launch_instance(
    app: tauri::AppHandle,
    state: State<AppState>,
    id: String,
    quick_play: Option<QuickPlay>,
    full_verify: bool,
    safe_mode: Option<bool>,
) -> CmdResult<()> {
    if state.registry.is_running(&id) {
        return Err(format!("Профиль «{id}» уже запускается или запущен"));
    }
    let data = state.data.clone();
    let registry = state.registry.clone();
    let emitter = app.clone();
    let event_id = id.clone();
    let emit: launch::LaunchEventFn = Arc::new(move |event| {
        let _ = emitter.emit(
            "launch-event",
            LaunchPayload {
                id: event_id.clone(),
                event,
            },
        );
    });
    tauri::async_runtime::spawn(async move {
        let result = launch::launch(
            &data,
            &KeyringStore,
            &registry,
            &id,
            LaunchOptions {
                quick_play,
                full_verify,
                safe_mode: safe_mode.unwrap_or(false),
            },
            emit,
        )
        .await;
        let error = match result {
            Ok(_) => None,
            Err(quarry_core::Error::Cancelled) => Some("Запуск отменён".into()),
            Err(e) => Some(e.to_string()),
        };
        let _ = app.emit("launch-finished", LaunchFinished { id, error });
    });
    Ok(())
}

#[tauri::command]
async fn preview_command(state: State<'_, AppState>, id: String) -> CmdResult<String> {
    launch::preview(&state.data, &KeyringStore, &id)
        .await
        .map_err(err)
}

#[tauri::command]
fn stop_instance(state: State<AppState>, id: String) -> bool {
    state.registry.stop(&id)
}

#[tauri::command]
fn open_instance_folder(
    app: tauri::AppHandle,
    state: State<AppState>,
    id: String,
    sub: Option<String>,
) -> CmdResult<()> {
    let store = InstanceStore::new(state.data.clone());
    let mut dir = store.game_dir(&id).map_err(err)?;
    if let Some(sub) = sub {
        // Только известные подпапки игры.
        if ![
            "mods",
            "saves",
            "screenshots",
            "resourcepacks",
            "shaderpacks",
            "config",
            "logs",
            "crash-reports",
        ]
        .contains(&sub.as_str())
        {
            return Err("Неизвестная папка".into());
        }
        dir = dir.join(sub);
    }
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    app.opener()
        .open_path(dir.to_string_lossy(), None::<&str>)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn read_log_file(
    state: State<AppState>,
    id: String,
    file: Option<String>,
) -> CmdResult<Vec<String>> {
    let logs = InstanceStore::new(state.data.clone())
        .logs_dir(&id)
        .map_err(err)?;
    let path = match file {
        Some(name) => {
            // Только файлы из папки логов профиля.
            if name.contains(['/', '\\']) || name.contains("..") {
                return Err("Недопустимое имя файла".into());
            }
            logs.join(name)
        }
        None => {
            let mut files: Vec<_> = std::fs::read_dir(&logs)
                .map_err(|_| "Логов этого профиля пока нет".to_string())?
                .flatten()
                .map(|e| e.path())
                .filter(|p| {
                    p.file_name()
                        .is_some_and(|n| n.to_string_lossy().starts_with("game-"))
                })
                .collect();
            files.sort();
            files.pop().ok_or("Логов этого профиля пока нет")?
        }
    };
    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let lines: Vec<String> = text.lines().map(str::to_string).collect();
    // Последние 5000 строк — достаточно для разбора, не перегружает интерфейс.
    Ok(lines[lines.len().saturating_sub(5000)..].to_vec())
}

#[tauri::command]
fn list_accounts(state: State<AppState>) -> CmdResult<Vec<AccountRef>> {
    AccountStore::new(&state.data, &KeyringStore)
        .list()
        .map_err(err)
}

#[tauri::command]
async fn account_avatar(state: State<'_, AppState>, key: String) -> CmdResult<Option<String>> {
    let account = AccountStore::new(&state.data, &KeyringStore)
        .get(&key)
        .map_err(err)?;
    Ok(match account {
        Some(a) => quarry_core::skin::avatar_data_url(&state.data, &a).await,
        None => None,
    })
}

#[tauri::command]
fn set_default_account(state: State<AppState>, key: String) -> CmdResult<()> {
    let mut g = GlobalSettings::load(&state.data).map_err(err)?;
    g.default_account = Some(key);
    g.save(&state.data).map_err(err)
}

#[tauri::command]
fn remove_account(state: State<AppState>, key: String) -> CmdResult<()> {
    AccountStore::new(&state.data, &KeyringStore)
        .remove(&key)
        .map_err(err)
}

#[tauri::command]
async fn microsoft_start(
    state: State<'_, AppState>,
) -> CmdResult<quarry_core::account::microsoft::DeviceCode> {
    let code = quarry_core::account::microsoft::start_device_code()
        .await
        .map_err(err)?;
    *state.login_cancel.lock().unwrap_or_else(|p| p.into_inner()) = Some(Cancel::new());
    Ok(code)
}

#[tauri::command]
async fn microsoft_finish(
    state: State<'_, AppState>,
    code: quarry_core::account::microsoft::DeviceCode,
) -> CmdResult<AccountRef> {
    let cancel = state
        .login_cancel
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .clone()
        .unwrap_or_default();
    let secrets = KeyringStore;
    let store = AccountStore::new(&state.data, &secrets);
    quarry_core::account::microsoft::complete_login(&store, &code, &cancel)
        .await
        .map_err(err)
}

#[tauri::command]
async fn ely_login(app: tauri::AppHandle, state: State<'_, AppState>) -> CmdResult<AccountRef> {
    let cancel = Cancel::new();
    *state.login_cancel.lock().unwrap_or_else(|p| p.into_inner()) = Some(cancel.clone());
    let secrets = KeyringStore;
    let store = AccountStore::new(&state.data, &secrets);
    quarry_core::account::ely::login(
        &store,
        |url| {
            app.opener()
                .open_url(url, None::<&str>)
                .map_err(|e| quarry_core::Error::Auth(format!("Не удалось открыть браузер: {e}")))
        },
        &cancel,
    )
    .await
    .map_err(err)
}

#[derive(Serialize)]
struct PasswordLoginResult {
    account: Option<AccountRef>,
    need_totp: bool,
}

/// Вход Ely.by логином и паролем. Пароль не сохраняется и не логируется.
#[tauri::command]
async fn ely_login_password(
    state: State<'_, AppState>,
    login: String,
    password: String,
    totp: Option<String>,
) -> CmdResult<PasswordLoginResult> {
    let secrets = KeyringStore;
    let store = AccountStore::new(&state.data, &secrets);
    match quarry_core::account::ely::login_password(&store, &login, &password, totp.as_deref())
        .await
        .map_err(err)?
    {
        quarry_core::account::ely::PasswordLogin::Done(a) => Ok(PasswordLoginResult {
            account: Some(a),
            need_totp: false,
        }),
        quarry_core::account::ely::PasswordLogin::NeedTotp => Ok(PasswordLoginResult {
            account: None,
            need_totp: true,
        }),
    }
}

#[tauri::command]
fn cancel_login(state: State<AppState>) {
    if let Some(c) = state
        .login_cancel
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .take()
    {
        c.cancel();
    }
}

/// Открыть ссылку в системном браузере. Только адреса входа Microsoft и сайты проекта.
#[tauri::command]
fn open_link(app: tauri::AppHandle, url: String) -> CmdResult<()> {
    const ALLOWED: [&str; 5] = [
        "https://www.microsoft.com/link",
        "https://microsoft.com/devicelogin",
        "https://www.microsoft.com/devicelogin",
        "https://ely.by",
        "https://github.com/Hewako/quarry-launcher",
    ];
    if !ALLOWED.iter().any(|a| url.starts_with(a)) {
        return Err("Эту ссылку лаунчер не открывает".into());
    }
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| e.to_string())
}

// ---------- Моды, каталог, снимки, импорт ----------

#[derive(Serialize)]
struct ModsView {
    files: Vec<quarry_core::mods::ModFile>,
    report: quarry_core::mods::doctor::Report,
    journal: Vec<quarry_core::mods::manage::JournalEntry>,
}

#[tauri::command]
fn mods_view(state: State<AppState>, id: String) -> CmdResult<ModsView> {
    let store = InstanceStore::new(state.data.clone());
    let inst = store.get(&id).map_err(err)?;
    let (game, dir) = (
        store.game_dir(&id).map_err(err)?,
        store.dir(&id).map_err(err)?,
    );
    let files = quarry_core::mods::inventory(&game, &dir).map_err(err)?;
    let report = quarry_core::mods::doctor::check(
        &files,
        inst.loader,
        &inst.game_version,
        inst.loader_version.as_deref(),
    );
    let mut journal = quarry_core::mods::manage::journal(&dir);
    journal.reverse();
    journal.truncate(50);
    Ok(ModsView {
        files,
        report,
        journal,
    })
}

#[tauri::command]
fn mods_toggle(state: State<AppState>, id: String, file: String, enabled: bool) -> CmdResult<()> {
    if state.registry.is_running(&id) {
        return Err("Сначала закройте игру в этом профиле".into());
    }
    let store = InstanceStore::new(state.data.clone());
    quarry_core::mods::manage::toggle(&store, &id, &file, enabled, "вручную").map_err(err)
}

#[tauri::command]
fn mods_remove(
    state: State<AppState>,
    id: String,
    file: String,
) -> CmdResult<quarry_core::snapshot::SnapshotInfo> {
    if state.registry.is_running(&id) {
        return Err("Сначала закройте игру в этом профиле".into());
    }
    let store = InstanceStore::new(state.data.clone());
    quarry_core::mods::manage::remove(&store, &id, &file, "вручную").map_err(err)
}

#[tauri::command]
async fn catalog_search(
    state: State<'_, AppState>,
    id: String,
    query: String,
    kind: quarry_core::catalog::ProjectType,
    offset: u32,
) -> CmdResult<quarry_core::catalog::SearchResult> {
    let inst = InstanceStore::new(state.data.clone())
        .get(&id)
        .map_err(err)?;
    let loaders = quarry_core::catalog::loaders_for(inst.loader);
    quarry_core::catalog::search(&query, kind, &loaders, &inst.game_version, offset)
        .await
        .map_err(err)
}

#[tauri::command]
async fn mods_plan_install(
    state: State<'_, AppState>,
    id: String,
    project: String,
) -> CmdResult<quarry_core::mods::manage::Plan> {
    let store = InstanceStore::new(state.data.clone());
    quarry_core::mods::manage::plan_install(&store, &id, &project)
        .await
        .map_err(err)
}

#[tauri::command]
async fn mods_check_updates(
    state: State<'_, AppState>,
    id: String,
) -> CmdResult<quarry_core::mods::manage::Plan> {
    let store = InstanceStore::new(state.data.clone());
    quarry_core::mods::manage::check_updates(&store, &id)
        .await
        .map_err(err)
}

#[tauri::command]
async fn mods_apply(
    state: State<'_, AppState>,
    id: String,
    plan: quarry_core::mods::manage::Plan,
    reason: String,
) -> CmdResult<quarry_core::snapshot::SnapshotInfo> {
    if state.registry.is_running(&id) {
        return Err("Сначала закройте игру в этом профиле".into());
    }
    let store = InstanceStore::new(state.data.clone());
    quarry_core::mods::manage::apply(&store, &id, plan, &reason, &Cancel::new())
        .await
        .map_err(err)
}

#[tauri::command]
fn snapshots_list(
    state: State<AppState>,
    id: String,
) -> CmdResult<Vec<quarry_core::snapshot::SnapshotInfo>> {
    let store = InstanceStore::new(state.data.clone());
    quarry_core::snapshot::list(&store.dir(&id).map_err(err)?).map_err(err)
}

#[tauri::command]
fn snapshot_create(
    state: State<AppState>,
    id: String,
    reason: String,
) -> CmdResult<quarry_core::snapshot::SnapshotInfo> {
    let store = InstanceStore::new(state.data.clone());
    let reason = if reason.trim().is_empty() {
        "Создан вручную".to_string()
    } else {
        reason
    };
    quarry_core::snapshot::create(
        &store.dir(&id).map_err(err)?,
        &store.game_dir(&id).map_err(err)?,
        &reason,
        false,
    )
    .map_err(err)
}

#[tauri::command]
fn snapshot_diff(
    state: State<AppState>,
    id: String,
    snapshot: String,
) -> CmdResult<quarry_core::snapshot::Diff> {
    let store = InstanceStore::new(state.data.clone());
    quarry_core::snapshot::diff(
        &store.dir(&id).map_err(err)?,
        &store.game_dir(&id).map_err(err)?,
        &snapshot,
    )
    .map_err(err)
}

#[tauri::command]
fn snapshot_restore(
    state: State<AppState>,
    id: String,
    snapshot: String,
) -> CmdResult<quarry_core::snapshot::SnapshotInfo> {
    if state.registry.is_running(&id) {
        return Err("Сначала закройте игру в этом профиле".into());
    }
    let store = InstanceStore::new(state.data.clone());
    quarry_core::snapshot::restore(
        &store.dir(&id).map_err(err)?,
        &store.game_dir(&id).map_err(err)?,
        &snapshot,
    )
    .map_err(err)
}

#[tauri::command]
fn snapshot_pin(
    state: State<AppState>,
    id: String,
    snapshot: String,
    pinned: bool,
) -> CmdResult<()> {
    let store = InstanceStore::new(state.data.clone());
    quarry_core::snapshot::set_pinned(&store.dir(&id).map_err(err)?, &snapshot, pinned).map_err(err)
}

#[tauri::command]
fn worlds_list(state: State<AppState>, id: String) -> CmdResult<Vec<String>> {
    let game = InstanceStore::new(state.data.clone())
        .game_dir(&id)
        .map_err(err)?;
    let mut out: Vec<String> = std::fs::read_dir(game.join("saves"))
        .map(|rd| {
            rd.flatten()
                .filter(|e| e.path().join("level.dat").is_file())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    Ok(out)
}

#[tauri::command]
fn world_backup(
    state: State<AppState>,
    id: String,
    world: String,
) -> CmdResult<quarry_core::snapshot::WorldBackup> {
    let store = InstanceStore::new(state.data.clone());
    let game = store.game_dir(&id).map_err(err)?;
    quarry_core::snapshot::backup_world(&game, &state.data.backups().join(&id), &world).map_err(err)
}

#[derive(Serialize)]
struct ImportSources {
    prism: Vec<(String, String)>,
    official: Option<String>,
}

#[tauri::command]
fn import_sources() -> ImportSources {
    ImportSources {
        prism: quarry_core::import::find_prism_instances()
            .into_iter()
            .map(|p| {
                let name = quarry_core::import::detect_mmc(&p)
                    .map(|d| format!("{} ({} · {})", d.name, d.game_version, d.loader.display()))
                    .unwrap_or_else(|_| {
                        p.file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default()
                    });
                (p.display().to_string(), name)
            })
            .collect(),
        official: quarry_core::import::official_minecraft_dir().map(|p| p.display().to_string()),
    }
}

#[tauri::command]
async fn import_path(
    state: State<'_, AppState>,
    path: String,
) -> CmdResult<quarry_core::import::ImportResult> {
    let store = InstanceStore::new(state.data.clone());
    let p = std::path::PathBuf::from(&path);
    if p.is_dir() {
        quarry_core::import::import_mmc(&store, &p).map_err(err)
    } else if p.extension().is_some_and(|e| e == "mrpack") {
        quarry_core::import::import_mrpack(&store, &p, &Cancel::new())
            .await
            .map_err(err)
    } else if p.extension().is_some_and(|e| e == "zip") {
        quarry_core::import::import_curseforge(&store, &p).map_err(err)
    } else {
        Err("Поддерживаются папка инстанса Prism/MultiMC, .mrpack и .zip CurseForge".into())
    }
}

#[tauri::command]
fn link_minecraft(
    state: State<AppState>,
    path: String,
    name: String,
    game_version: String,
    loader: String,
) -> CmdResult<quarry_core::import::ImportResult> {
    let store = InstanceStore::new(state.data.clone());
    let kind = LoaderKind::parse(&loader).map_err(err)?;
    quarry_core::import::link_existing(
        &store,
        std::path::Path::new(&path),
        &name,
        &game_version,
        kind,
        None,
    )
    .map_err(err)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let root = DataDir::default_root().ok_or("Не удалось определить домашнюю папку")?;
            let data = DataDir::open(root)?;
            app.manage(AppState {
                data,
                registry: LaunchRegistry::default(),
                login_cancel: Mutex::new(None),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_info,
            list_instances,
            create_instance,
            trash_instance,
            update_instance,
            launch_settings,
            global_settings,
            save_global_settings,
            list_versions,
            version_info,
            loader_versions,
            launch_instance,
            stop_instance,
            preview_command,
            open_instance_folder,
            read_log_file,
            list_accounts,
            remove_account,
            account_avatar,
            set_default_account,
            microsoft_start,
            microsoft_finish,
            ely_login,
            ely_login_password,
            cancel_login,
            open_link,
            mods_view,
            mods_toggle,
            mods_remove,
            catalog_search,
            mods_plan_install,
            mods_check_updates,
            mods_apply,
            snapshots_list,
            snapshot_create,
            snapshot_diff,
            snapshot_restore,
            snapshot_pin,
            worlds_list,
            world_backup,
            import_sources,
            import_path,
            link_minecraft,
        ])
        .run(tauri::generate_context!())
        .expect("ошибка запуска приложения Quarry");
}
