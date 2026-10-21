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
    /// В сборку подставлен ключ CurseForge.
    curseforge: bool,
    os: &'static str,
}

#[tauri::command]
fn app_info(state: State<AppState>) -> AppInfo {
    AppInfo {
        version: launch::LAUNCHER_VERSION,
        dev_launch: launch::DEV_LAUNCH,
        data_dir: state.data.root().display().to_string(),
        microsoft_configured: quarry_core::account::microsoft::CLIENT_ID.is_some(),
        curseforge: quarry_core::curseforge::api_key().is_some(),
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
    meta: Option<ProfileMeta>,
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
    if let Some(m) = meta {
        let text = |v: Option<String>, max: usize| -> CmdResult<Option<String>> {
            let v = v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
            if v.as_ref().is_some_and(|s| s.chars().count() > max) {
                return Err(format!("Не длиннее {max} символов"));
            }
            Ok(v)
        };
        inst.group = text(m.group, 40)?;
        inst.description = text(m.description, 300)?;
        inst.color = text(m.color, 16)?;
        if inst
            .color
            .as_ref()
            .is_some_and(|c| !c.starts_with('#') || !c[1..].chars().all(|c| c.is_ascii_hexdigit()))
        {
            return Err("Цвет — в виде #RRGGBB".into());
        }
        inst.favorite = m.favorite;
        inst.archived = m.archived;
    }
    store.save(&inst).map_err(err)?;
    Ok(inst)
}

/// Оформление профиля: группа, заметка, цвет, избранное, архив.
#[derive(serde::Deserialize)]
struct ProfileMeta {
    group: Option<String>,
    description: Option<String>,
    color: Option<String>,
    favorite: bool,
    archived: bool,
}

#[tauri::command]
fn clone_instance(
    state: State<AppState>,
    id: String,
    name: String,
    parts: quarry_core::instance::ops::CloneParts,
) -> CmdResult<Instance> {
    quarry_core::instance::ops::clone_instance(
        &InstanceStore::new(state.data.clone()),
        &id,
        &name,
        parts,
    )
    .map_err(err)
}

#[tauri::command]
async fn disk_usage(
    state: State<'_, AppState>,
    id: String,
) -> CmdResult<quarry_core::instance::ops::DiskUsage> {
    let store = InstanceStore::new(state.data.clone());
    tauri::async_runtime::spawn_blocking(move || {
        quarry_core::instance::ops::disk_usage(&store, &id)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(err)
}

#[tauri::command]
fn copy_between(
    state: State<AppState>,
    from: String,
    to: String,
    items: Vec<String>,
) -> CmdResult<()> {
    if state.registry.is_running(&to) {
        return Err("Сначала закройте игру в профиле, куда переносите".into());
    }
    quarry_core::instance::ops::copy_between(
        &InstanceStore::new(state.data.clone()),
        &from,
        &to,
        &items,
    )
    .map_err(err)
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
#[tauri::command]
fn memory_advice(max_mb: u32, pack_mb: Option<u32>) -> Vec<String> {
    quarry_core::settings::memory_advice(max_mb, total_memory_mb(), pack_mb)
}

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
    settings.save(&state.data).map_err(err)?;
    quarry_core::download::throttle().set_limit_kbps(settings.download_limit_kbps.into());
    Ok(())
}

/// Пауза всех загрузок (и снятие паузы).
#[tauri::command]
fn downloads_pause(paused: bool) {
    quarry_core::download::throttle().set_paused(paused);
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
    const ALLOWED: [&str; 7] = [
        "https://www.microsoft.com/link",
        "https://microsoft.com/devicelogin",
        "https://www.microsoft.com/devicelogin",
        "https://ely.by",
        "https://github.com/Hewako/quarry-launcher",
        "https://www.curseforge.com/",
        "https://modrinth.com/",
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
fn mods_pin(state: State<AppState>, id: String, file: String, pinned: bool) -> CmdResult<()> {
    let store = InstanceStore::new(state.data.clone());
    quarry_core::mods::manage::set_pinned(&store, &id, &file, pinned).map_err(err)
}

#[tauri::command]
async fn mods_add_files(
    state: State<'_, AppState>,
    id: String,
    paths: Vec<String>,
) -> CmdResult<Vec<quarry_core::mods::manage::AddedFile>> {
    if state.registry.is_running(&id) {
        return Err("Сначала закройте игру в этом профиле".into());
    }
    let store = InstanceStore::new(state.data.clone());
    let paths: Vec<std::path::PathBuf> = paths.into_iter().map(std::path::PathBuf::from).collect();
    quarry_core::mods::manage::add_files(&store, &id, &paths)
        .await
        .map(|(_, added)| added)
        .map_err(err)
}

#[tauri::command]
async fn catalog_search(
    state: State<'_, AppState>,
    id: String,
    query: String,
    kind: quarry_core::catalog::ProjectType,
    offset: u32,
    category: Option<String>,
    source: Option<String>,
) -> CmdResult<quarry_core::catalog::SearchResult> {
    let inst = InstanceStore::new(state.data.clone())
        .get(&id)
        .map_err(err)?;
    if source.as_deref() == Some("curseforge") {
        let hits =
            quarry_core::curseforge::search(&query, kind, inst.loader, &inst.game_version, offset)
                .await
                .map_err(err)?;
        return Ok(quarry_core::catalog::SearchResult {
            total_hits: hits.len() as u64,
            hits,
        });
    }
    let loaders = quarry_core::catalog::loaders_for(inst.loader);
    quarry_core::catalog::search(
        &query,
        kind,
        &loaders,
        &inst.game_version,
        offset,
        category.as_deref(),
    )
    .await
    .map_err(err)
}

#[tauri::command]
async fn mods_plan_install(
    state: State<'_, AppState>,
    id: String,
    project: String,
    channel: Option<quarry_core::catalog::Channel>,
    source: Option<String>,
) -> CmdResult<quarry_core::mods::manage::Plan> {
    let store = InstanceStore::new(state.data.clone());
    if source.as_deref() == Some("curseforge") {
        let mod_id: u64 = project
            .parse()
            .map_err(|_| "Неверный номер мода CurseForge".to_string())?;
        return quarry_core::curseforge::plan_install(
            &store,
            &id,
            mod_id,
            channel.unwrap_or_default(),
        )
        .await
        .map_err(err);
    }
    quarry_core::mods::manage::plan_install(&store, &id, &project, channel.unwrap_or_default())
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
fn worlds_list(
    state: State<AppState>,
    id: String,
) -> CmdResult<Vec<quarry_core::worlds::WorldInfo>> {
    let game = InstanceStore::new(state.data.clone())
        .game_dir(&id)
        .map_err(err)?;
    Ok(quarry_core::worlds::list(
        &game,
        &quarry_core::worlds::backups_dir(&state.data, &id),
    ))
}

#[tauri::command]
fn world_backup(
    state: State<AppState>,
    id: String,
    world: String,
) -> CmdResult<quarry_core::snapshot::WorldBackup> {
    let store = InstanceStore::new(state.data.clone());
    let game = store.game_dir(&id).map_err(err)?;
    quarry_core::snapshot::backup_world(
        &game,
        &quarry_core::worlds::backups_dir(&state.data, &id),
        &world,
    )
    .map_err(err)
}

#[tauri::command]
fn world_restore(state: State<AppState>, id: String, file: String) -> CmdResult<String> {
    if state.registry.is_running(&id) {
        return Err("Сначала закройте игру в этом профиле".into());
    }
    let game = InstanceStore::new(state.data.clone())
        .game_dir(&id)
        .map_err(err)?;
    quarry_core::worlds::restore(
        &game,
        &quarry_core::worlds::backups_dir(&state.data, &id),
        std::path::Path::new(&file),
    )
    .map_err(err)
}

#[tauri::command]
async fn move_game_dir(
    state: State<'_, AppState>,
    id: String,
    target: Option<String>,
) -> CmdResult<String> {
    if state.registry.is_running(&id) {
        return Err("Сначала закройте игру в этом профиле".into());
    }
    let store = InstanceStore::new(state.data.clone());
    tauri::async_runtime::spawn_blocking(move || {
        quarry_core::instance::ops::move_game_dir(
            &store,
            &id,
            target.as_deref().map(std::path::Path::new),
        )
    })
    .await
    .map_err(|e| e.to_string())?
    .map(|p| p.display().to_string())
    .map_err(err)
}

#[derive(Serialize)]
struct ResourcesView {
    packs: Vec<quarry_core::resources::PackInfo>,
    /// Формат ресурспаков игры; `None` — игра ещё не скачана.
    game_format: Option<i64>,
    /// Установлен Iris или Oculus — без них шейдеры не работают.
    shader_loader: bool,
}

/// Форматы ресурспаков и датапаков игры (из скачанного клиента); `None` — ещё не скачан.
fn game_pack_format(
    state: &AppState,
    game_version: &str,
    kind: quarry_core::resources::Kind,
) -> Option<i64> {
    let jar = state
        .data
        .versions()
        .join(game_version)
        .join(format!("{game_version}.jar"));
    let info = quarry_core::worlds::game_jar_info(&jar).ok()?;
    match kind {
        quarry_core::resources::Kind::Datapack => info.data_pack_format,
        _ => info.resource_pack_format,
    }
}

#[tauri::command]
fn resources_list(
    state: State<AppState>,
    id: String,
    kind: quarry_core::resources::Kind,
    world: Option<String>,
) -> CmdResult<ResourcesView> {
    let store = InstanceStore::new(state.data.clone());
    let inst = store.get(&id).map_err(err)?;
    let game = store.game_dir(&id).map_err(err)?;
    let game_format = game_pack_format(&state, &inst.game_version, kind);
    // Список ищет папку вида внутри: для датапаков это папка мира.
    let base = match kind {
        quarry_core::resources::Kind::Datapack => {
            quarry_core::resources::target_dir(&game, kind, world.as_deref())
                .map_err(err)?
                .parent()
                .map(std::path::Path::to_path_buf)
                .unwrap_or_default()
        }
        _ => game.clone(),
    };
    let shader_loader = quarry_core::mods::inventory(&game, &store.dir(&id).map_err(err)?)
        .unwrap_or_default()
        .iter()
        .any(|f| f.enabled && f.mods.iter().any(|m| m.id == "iris" || m.id == "oculus"));
    Ok(ResourcesView {
        packs: quarry_core::resources::list(&base, kind, game_format),
        game_format,
        shader_loader,
    })
}

#[tauri::command]
fn resources_set_enabled(
    state: State<AppState>,
    id: String,
    enabled: Vec<String>,
) -> CmdResult<()> {
    // Игра перезаписывает options.txt при выходе.
    if state.registry.is_running(&id) {
        return Err("Сначала закройте игру: она перезапишет настройки при выходе".into());
    }
    let store = InstanceStore::new(state.data.clone());
    let inst = store.get(&id).map_err(err)?;
    let game = store.game_dir(&id).map_err(err)?;
    quarry_core::resources::set_enabled(
        &game,
        &enabled,
        game_pack_format(
            &state,
            &inst.game_version,
            quarry_core::resources::Kind::Resourcepack,
        ),
    )
    .map_err(err)
}

#[tauri::command]
fn shader_set(state: State<AppState>, id: String, shader: Option<String>) -> CmdResult<()> {
    if state.registry.is_running(&id) {
        return Err("Сначала закройте игру: шейдер меняется в самой игре или после выхода".into());
    }
    let game = InstanceStore::new(state.data.clone())
        .game_dir(&id)
        .map_err(err)?;
    quarry_core::resources::set_shader(&game, shader.as_deref()).map_err(err)
}

#[tauri::command]
fn resource_remove(
    state: State<AppState>,
    id: String,
    kind: quarry_core::resources::Kind,
    world: Option<String>,
    file: String,
) -> CmdResult<()> {
    if state.registry.is_running(&id) {
        return Err("Сначала закройте игру в этом профиле".into());
    }
    quarry_core::resources::remove(
        &InstanceStore::new(state.data.clone()),
        &id,
        kind,
        world.as_deref(),
        &file,
    )
    .map_err(err)
}

#[tauri::command]
async fn resource_install(
    state: State<'_, AppState>,
    id: String,
    project: String,
    kind: quarry_core::resources::Kind,
    world: Option<String>,
) -> CmdResult<String> {
    quarry_core::resources::install(
        &InstanceStore::new(state.data.clone()),
        &id,
        &project,
        kind,
        world.as_deref(),
    )
    .await
    .map_err(err)
}

#[derive(Serialize)]
struct GroupsView {
    names: Vec<&'static str>,
    groups: std::collections::BTreeMap<String, String>,
}

#[tauri::command]
fn mods_groups(state: State<AppState>, id: String) -> CmdResult<GroupsView> {
    let dir = InstanceStore::new(state.data.clone())
        .dir(&id)
        .map_err(err)?;
    Ok(GroupsView {
        names: quarry_core::mods::groups::GROUPS.to_vec(),
        groups: quarry_core::mods::groups::ModGroups::load(&dir)
            .map_err(err)?
            .groups,
    })
}

#[tauri::command]
fn mods_set_group(
    state: State<AppState>,
    id: String,
    file: String,
    group: Option<String>,
) -> CmdResult<()> {
    quarry_core::mods::groups::set_group(
        &InstanceStore::new(state.data.clone()),
        &id,
        &file,
        group.as_deref(),
    )
    .map_err(err)
}

#[tauri::command]
fn mods_toggle_group(
    state: State<AppState>,
    id: String,
    group: String,
    enabled: bool,
) -> CmdResult<Vec<String>> {
    if state.registry.is_running(&id) {
        return Err("Сначала закройте игру в этом профиле".into());
    }
    quarry_core::mods::groups::toggle_group(
        &InstanceStore::new(state.data.clone()),
        &id,
        &group,
        enabled,
    )
    .map_err(err)
}

#[tauri::command]
async fn mods_auto_groups(state: State<'_, AppState>, id: String) -> CmdResult<usize> {
    quarry_core::mods::groups::auto_assign(&InstanceStore::new(state.data.clone()), &id)
        .await
        .map_err(err)
}

fn config_dirs(state: &AppState, id: &str) -> CmdResult<(std::path::PathBuf, std::path::PathBuf)> {
    let store = InstanceStore::new(state.data.clone());
    Ok((
        store.game_dir(id).map_err(err)?,
        store.dir(id).map_err(err)?,
    ))
}

#[tauri::command]
fn configs_list(
    state: State<AppState>,
    id: String,
) -> CmdResult<Vec<quarry_core::configs::ConfigFile>> {
    let (game, dir) = config_dirs(&state, &id)?;
    Ok(quarry_core::configs::list(&game, &dir))
}

#[tauri::command]
async fn configs_search(
    state: State<'_, AppState>,
    id: String,
    query: String,
) -> CmdResult<Vec<quarry_core::configs::Hit>> {
    let (game, _) = config_dirs(&state, &id)?;
    tauri::async_runtime::spawn_blocking(move || quarry_core::configs::search(&game, &query))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn config_read(state: State<AppState>, id: String, path: String) -> CmdResult<String> {
    let (game, _) = config_dirs(&state, &id)?;
    quarry_core::configs::read(&game, &path).map_err(err)
}

#[derive(Serialize)]
struct ConfigPreview {
    diff: Vec<quarry_core::configs::DiffLine>,
    error: Option<String>,
}

#[tauri::command]
fn config_preview(
    state: State<AppState>,
    id: String,
    path: String,
    text: String,
) -> CmdResult<ConfigPreview> {
    let (game, _) = config_dirs(&state, &id)?;
    let old = quarry_core::configs::read(&game, &path).unwrap_or_default();
    Ok(ConfigPreview {
        diff: quarry_core::configs::diff(&old, &text),
        error: quarry_core::configs::validate(&path, &text).err(),
    })
}

#[tauri::command]
fn config_save(state: State<AppState>, id: String, path: String, text: String) -> CmdResult<()> {
    // Игра и моды перезаписывают свои настройки при выходе.
    if state.registry.is_running(&id) {
        return Err("Сначала закройте игру: она перезапишет настройки при выходе".into());
    }
    let (game, dir) = config_dirs(&state, &id)?;
    quarry_core::configs::save(&game, &dir, &path, &text).map_err(err)
}

#[tauri::command]
fn config_history(
    state: State<AppState>,
    id: String,
    path: String,
) -> CmdResult<Vec<quarry_core::configs::HistoryEntry>> {
    let (_, dir) = config_dirs(&state, &id)?;
    Ok(quarry_core::configs::history(&dir, &path))
}

#[tauri::command]
fn config_history_read(
    state: State<AppState>,
    id: String,
    path: String,
    at: u64,
) -> CmdResult<String> {
    let (_, dir) = config_dirs(&state, &id)?;
    quarry_core::configs::history_read(&dir, &path, at).map_err(err)
}

#[derive(Serialize)]
struct OptionsView {
    entries: Vec<(String, String)>,
    conflicts: Vec<quarry_core::options::KeyConflict>,
    presets: Vec<quarry_core::options::Preset>,
}

fn presets_dir(state: &AppState) -> std::path::PathBuf {
    state.data.root().join("option-presets")
}

#[tauri::command]
fn options_view(state: State<AppState>, id: String) -> CmdResult<OptionsView> {
    let game = InstanceStore::new(state.data.clone())
        .game_dir(&id)
        .map_err(err)?;
    let entries = quarry_core::options::read(&game).map_err(err)?;
    Ok(OptionsView {
        conflicts: quarry_core::options::conflicts(&entries),
        presets: quarry_core::options::list_presets(&presets_dir(&state)),
        entries,
    })
}

/// Записать настройки игры; перед этим — снимок (в него входит options.txt).
fn options_apply(
    state: &AppState,
    id: &str,
    changes: &std::collections::BTreeMap<String, String>,
    reason: &str,
) -> CmdResult<()> {
    // Игра перезаписывает options.txt при выходе.
    if state.registry.is_running(id) {
        return Err("Сначала закройте игру: она перезапишет настройки при выходе".into());
    }
    let store = InstanceStore::new(state.data.clone());
    let game = store.game_dir(id).map_err(err)?;
    quarry_core::snapshot::create(&store.dir(id).map_err(err)?, &game, reason, true)
        .map_err(err)?;
    quarry_core::options::set(&game, changes).map_err(err)
}

#[tauri::command]
fn options_set(
    state: State<AppState>,
    id: String,
    changes: std::collections::BTreeMap<String, String>,
) -> CmdResult<()> {
    options_apply(&state, &id, &changes, "Перед изменением настроек игры")
}

#[tauri::command]
fn options_preset_save(
    state: State<AppState>,
    id: String,
    name: String,
    category: quarry_core::options::Category,
) -> CmdResult<quarry_core::options::Preset> {
    let game = InstanceStore::new(state.data.clone())
        .game_dir(&id)
        .map_err(err)?;
    let entries = quarry_core::options::read(&game).map_err(err)?;
    quarry_core::options::save_preset(&presets_dir(&state), &name, category, &entries).map_err(err)
}

#[tauri::command]
fn options_preset_apply(state: State<AppState>, id: String, name: String) -> CmdResult<()> {
    let changes = quarry_core::options::load_preset(&presets_dir(&state), &name).map_err(err)?;
    options_apply(&state, &id, &changes, &format!("Перед пресетом «{name}»"))
}

#[tauri::command]
fn options_layout_export(state: State<AppState>, id: String, path: String) -> CmdResult<()> {
    let game = InstanceStore::new(state.data.clone())
        .game_dir(&id)
        .map_err(err)?;
    let keys: String = quarry_core::options::read(&game)
        .map_err(err)?
        .into_iter()
        .filter(|(k, _)| k.starts_with("key_"))
        .map(|(k, v)| format!("{k}:{v}\n"))
        .collect();
    if keys.is_empty() {
        return Err("Назначений клавиш пока нет — запустите игру хотя бы раз".into());
    }
    std::fs::write(
        &path,
        format!("# Раскладка управления Minecraft (Quarry)\n{keys}"),
    )
    .map_err(|e| e.to_string())
}

#[tauri::command]
fn options_layout_import(state: State<AppState>, id: String, path: String) -> CmdResult<usize> {
    let meta = std::fs::metadata(&path).map_err(|e| e.to_string())?;
    if meta.len() > 256 * 1024 {
        return Err("Файл раскладки слишком большой".into());
    }
    let text = std::fs::read_to_string(&path).map_err(|_| "Это не текстовый файл".to_string())?;
    let keys = quarry_core::options::import_layout(&text).map_err(err)?;
    options_apply(&state, &id, &keys, "Перед импортом раскладки")?;
    Ok(keys.len())
}

#[tauri::command]
fn crash_reports(
    state: State<AppState>,
    id: String,
) -> CmdResult<Vec<quarry_core::diagnostics::CrashReportInfo>> {
    let game = InstanceStore::new(state.data.clone())
        .game_dir(&id)
        .map_err(err)?;
    Ok(quarry_core::diagnostics::crash_reports(&game))
}

#[tauri::command]
fn crash_report_read(state: State<AppState>, id: String, file: String) -> CmdResult<String> {
    let game = InstanceStore::new(state.data.clone())
        .game_dir(&id)
        .map_err(err)?;
    quarry_core::diagnostics::read_crash_report(&game, &file).map_err(err)
}

/// Отчёт поддержки. В режиме стримера скрывается и ник аккаунта профиля.
fn build_report(
    state: &AppState,
    id: &str,
) -> CmdResult<Vec<quarry_core::diagnostics::ReportFile>> {
    let store = InstanceStore::new(state.data.clone());
    let global = GlobalSettings::load(&state.data).unwrap_or_default();
    let nick = if global.streamer_mode {
        let inst = store.get(id).map_err(err)?;
        let wanted = inst.account.or(global.default_account);
        let accounts = AccountStore::new(&state.data, &KeyringStore)
            .list()
            .unwrap_or_default();
        accounts
            .iter()
            .find(|a| wanted.as_deref() == Some(a.key().as_str()))
            .or(accounts.first())
            .map(|a| a.name.clone())
    } else {
        None
    };
    quarry_core::diagnostics::support_report(&store, id, nick.as_deref()).map_err(err)
}

#[tauri::command]
fn support_report_preview(
    state: State<AppState>,
    id: String,
) -> CmdResult<Vec<quarry_core::diagnostics::ReportFile>> {
    build_report(&state, &id)
}

#[tauri::command]
fn support_report_save(state: State<AppState>, id: String, path: String) -> CmdResult<()> {
    let files = build_report(&state, &id)?;
    quarry_core::diagnostics::write_zip(&files, std::path::Path::new(&path)).map_err(err)
}

#[tauri::command]
async fn storage_overview(
    state: State<'_, AppState>,
) -> CmdResult<Vec<quarry_core::diagnostics::StoragePart>> {
    let data = state.data.clone();
    tauri::async_runtime::spawn_blocking(move || quarry_core::diagnostics::storage(&data))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn cache_clean(state: State<'_, AppState>) -> CmdResult<u64> {
    // Распакованные natives нужны запущенной игре.
    if !state.registry.running_ids().is_empty() {
        return Err("Сначала закройте все запущенные игры".into());
    }
    let data = state.data.clone();
    tauri::async_runtime::spawn_blocking(move || quarry_core::diagnostics::clean_cache(&data))
        .await
        .map_err(|e| e.to_string())?
        .map_err(err)
}

#[derive(Serialize)]
struct FirstRunInfo {
    data_dir: String,
    free_bytes: Option<u64>,
    javas: Vec<quarry_core::java::JavaInstall>,
}

#[tauri::command]
async fn first_run_info(state: State<'_, AppState>) -> CmdResult<FirstRunInfo> {
    let data = state.data.clone();
    let root = data.root().to_path_buf();
    Ok(FirstRunInfo {
        data_dir: root.display().to_string(),
        free_bytes: tauri::async_runtime::spawn_blocking(move || {
            quarry_core::paths::free_space(&root)
        })
        .await
        .ok()
        .flatten(),
        javas: quarry_core::java::discover(Some(&data)).await,
    })
}

/// Мастер пройден: запомнить язык и больше не показывать.
#[tauri::command]
fn first_run_done(state: State<AppState>, language: String) -> CmdResult<()> {
    let mut g = GlobalSettings::load(&state.data).map_err(err)?;
    g.first_run_done = true;
    g.language = if language == "en" {
        "en".into()
    } else {
        "ru".into()
    };
    g.save(&state.data).map_err(err)
}

#[tauri::command]
fn servers_list(
    state: State<AppState>,
    id: String,
) -> CmdResult<Vec<quarry_core::servers::Server>> {
    let game = InstanceStore::new(state.data.clone())
        .game_dir(&id)
        .map_err(err)?;
    quarry_core::servers::read(&game).map_err(err)
}

#[tauri::command]
fn servers_save(
    state: State<AppState>,
    id: String,
    servers: Vec<quarry_core::servers::Server>,
) -> CmdResult<()> {
    // Игра перезаписывает servers.dat при выходе — правки во время игры потерялись бы.
    if state.registry.is_running(&id) {
        return Err(
            "Сначала закройте игру в этом профиле: она перезапишет список при выходе".into(),
        );
    }
    let game = InstanceStore::new(state.data.clone())
        .game_dir(&id)
        .map_err(err)?;
    quarry_core::servers::write(&game, &servers).map_err(err)
}

#[tauri::command]
async fn server_ping(address: String) -> CmdResult<quarry_core::servers::Status> {
    quarry_core::servers::ping(&address).await.map_err(err)
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
    skip_optional: Option<Vec<String>>,
) -> CmdResult<quarry_core::import::ImportResult> {
    let store = InstanceStore::new(state.data.clone());
    let p = std::path::PathBuf::from(&path);
    if p.is_dir() {
        quarry_core::import::import_mmc(&store, &p).map_err(err)
    } else if p.extension().is_some_and(|e| e == "mrpack") {
        quarry_core::import::import_mrpack(
            &store,
            &p,
            &skip_optional.unwrap_or_default(),
            &Cancel::new(),
        )
        .await
        .map_err(err)
    } else if p.extension().is_some_and(|e| e == "zip") {
        quarry_core::import::import_curseforge(&store, &p).map_err(err)
    } else {
        Err("Поддерживаются папка инстанса Prism/MultiMC, .mrpack и .zip CurseForge".into())
    }
}

#[tauri::command]
fn mrpack_preview(path: String) -> CmdResult<quarry_core::import::MrpackPreview> {
    quarry_core::import::preview_mrpack(std::path::Path::new(&path)).map_err(err)
}

#[tauri::command]
fn pack_update_plan(
    state: State<AppState>,
    id: String,
    path: String,
) -> CmdResult<quarry_core::import::PackUpdatePlan> {
    quarry_core::import::plan_pack_update(
        &InstanceStore::new(state.data.clone()),
        &id,
        std::path::Path::new(&path),
    )
    .map_err(err)
}

#[tauri::command]
async fn pack_update_apply(
    state: State<'_, AppState>,
    id: String,
    path: String,
    skip_optional: Vec<String>,
) -> CmdResult<quarry_core::snapshot::SnapshotInfo> {
    if state.registry.is_running(&id) {
        return Err("Сначала закройте игру в этом профиле".into());
    }
    quarry_core::import::apply_pack_update(
        &InstanceStore::new(state.data.clone()),
        &id,
        std::path::Path::new(&path),
        &skip_optional,
        &Cancel::new(),
    )
    .await
    .map_err(err)
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
            if let Ok(g) = GlobalSettings::load(&data) {
                quarry_core::download::throttle().set_limit_kbps(g.download_limit_kbps.into());
            }
            // Правила Mod Doctor: сначала кеш, свежие — в фоне (не чаще раза в сутки).
            quarry_core::mods::rules::load_cached(&data);
            let d = data.clone();
            tauri::async_runtime::spawn(async move {
                let _ = quarry_core::mods::rules::refresh(&d).await;
            });
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
            mods_pin,
            mods_add_files,
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
            world_restore,
            clone_instance,
            disk_usage,
            copy_between,
            servers_list,
            mrpack_preview,
            pack_update_plan,
            pack_update_apply,
            first_run_info,
            first_run_done,
            crash_reports,
            crash_report_read,
            support_report_preview,
            support_report_save,
            storage_overview,
            cache_clean,
            options_view,
            options_set,
            options_preset_save,
            options_preset_apply,
            options_layout_export,
            options_layout_import,
            downloads_pause,
            memory_advice,
            configs_list,
            configs_search,
            config_read,
            config_preview,
            config_save,
            config_history,
            config_history_read,
            mods_groups,
            mods_set_group,
            mods_toggle_group,
            mods_auto_groups,
            resources_list,
            resources_set_enabled,
            shader_set,
            resource_remove,
            resource_install,
            move_game_dir,
            servers_save,
            server_ping,
            import_sources,
            import_path,
            link_minecraft,
        ])
        .run(tauri::generate_context!())
        .expect("ошибка запуска приложения Quarry");
}
