//! Сценарий запуска профиля: аккаунт → версия и загрузчик → установка → Java → команда →
//! процесс → итог. Один запуск на профиль одновременно (single-flight).

pub mod args;
pub mod log4j;
pub mod process;

use crate::account::{self, AccountRef, AccountStore, Provider, SecretStore};
use crate::download::{Cancel, Verify};
use crate::error::{Error, Result};
use crate::install::{EventFn, InstallEvent, Installer, Stage};
use crate::instance::{AuthMode, Instance, InstanceStore, LastLaunch, LaunchOutcome, now};
use crate::java::{self, Choice, JavaSource, Requirement};
use crate::logmask::Masker;
use crate::metadata::Environment;
use crate::paths::DataDir;
use crate::settings::{self, GlobalSettings};
use args::{GameAuth, LaunchSpec, QuickPlay};
use process::{ExitInfo, GameEvent};
use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::Notify;

pub const LAUNCHER_NAME: &str = "Quarry";
pub const LAUNCHER_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Сборка с функцией `dev-launch` (только отладка, в релизе недоступна).
pub const DEV_LAUNCH: bool = cfg!(feature = "dev-launch");

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LaunchEvent {
    Stage {
        stage: Stage,
        label: &'static str,
    },
    Progress {
        stage: Stage,
        progress: crate::download::Progress,
    },
    Account {
        provider: String,
        name: String,
    },
    Java {
        label: String,
        reason: String,
    },
    Warning {
        message: String,
    },
    Started {
        pid: Option<u32>,
        command: String,
    },
    Log {
        line: String,
    },
    Exited {
        info: ExitInfo,
    },
}

pub type LaunchEventFn = Arc<dyn Fn(LaunchEvent) + Send + Sync>;

#[derive(Debug, Clone, Default)]
pub struct LaunchOptions {
    pub quick_play: Option<QuickPlay>,
    /// «Проверить файлы»: полный пересчёт хэшей перед запуском.
    pub full_verify: bool,
    /// «Безопасный запуск»: моды на время игры откладываются, постоянная конфигурация не меняется.
    pub safe_mode: bool,
}

const SAFE_MODE_DIR: &str = "mods.quarry-safe-mode";

/// Если лаунчер закрылся во время безопасного запуска — вернуть моды на место.
fn recover_safe_mode(game_dir: &std::path::Path) -> Option<String> {
    let aside = game_dir.join(SAFE_MODE_DIR);
    let mods = game_dir.join("mods");
    if !aside.is_dir() {
        return None;
    }
    if mods.exists()
        && std::fs::read_dir(&mods)
            .map(|mut d| d.next().is_none())
            .unwrap_or(false)
    {
        let _ = std::fs::remove_dir(&mods);
    }
    if mods.exists() {
        return Some(format!(
            "После прошлого безопасного запуска моды остались в {SAFE_MODE_DIR}, а в mods/ уже есть файлы — разберите вручную"
        ));
    }
    std::fs::rename(&aside, &mods).ok()?;
    Some("Моды после прошлого безопасного запуска возвращены на место".into())
}

/// Возвращает моды на место при выходе из безопасного запуска (в том числе при ошибке).
struct SafeModeGuard {
    game_dir: PathBuf,
}

impl Drop for SafeModeGuard {
    fn drop(&mut self) {
        let _ = recover_safe_mode(&self.game_dir);
    }
}

struct Running {
    cancel: Cancel,
    stop: Option<Arc<Notify>>,
}

/// Реестр запусков: не даёт запустить подготовку одного профиля дважды и позволяет
/// отменить подготовку или остановить игру.
#[derive(Clone, Default)]
pub struct LaunchRegistry {
    inner: Arc<Mutex<HashMap<String, Running>>>,
}

struct Guard {
    registry: LaunchRegistry,
    id: String,
}

impl Drop for Guard {
    fn drop(&mut self) {
        self.registry.lock().remove(&self.id);
    }
}

impl LaunchRegistry {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Running>> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn claim(&self, id: &str, cancel: Cancel) -> Result<Guard> {
        let mut map = self.lock();
        if map.contains_key(id) {
            return Err(Error::AlreadyRunning(id.to_string()));
        }
        map.insert(id.to_string(), Running { cancel, stop: None });
        Ok(Guard {
            registry: self.clone(),
            id: id.to_string(),
        })
    }

    fn set_stop(&self, id: &str, stop: Arc<Notify>) {
        if let Some(r) = self.lock().get_mut(id) {
            r.stop = Some(stop);
        }
    }

    pub fn is_running(&self, id: &str) -> bool {
        self.lock().contains_key(id)
    }

    pub fn running_ids(&self) -> Vec<String> {
        self.lock().keys().cloned().collect()
    }

    /// Отменить подготовку или остановить игру.
    pub fn stop(&self, id: &str) -> bool {
        match self.lock().get(id) {
            Some(r) => {
                r.cancel.cancel();
                if let Some(s) = &r.stop {
                    s.notify_one();
                }
                true
            }
            None => false,
        }
    }
}

/// Найти аккаунт для профиля и получить данные входа для игры.
async fn resolve_auth(
    data: &DataDir,
    secrets: &dyn SecretStore,
    inst: &Instance,
    global: &GlobalSettings,
    emit: &LaunchEventFn,
) -> Result<(GameAuth, Option<AccountRef>)> {
    let store = AccountStore::new(data, secrets);
    let accounts = store.list()?;
    let wanted = inst.account.as_ref().or(global.default_account.as_ref());
    let provider = match inst.auth_mode {
        AuthMode::Microsoft => Provider::Microsoft,
        AuthMode::ElyBy => Provider::ElyBy,
    };
    let account = accounts
        .iter()
        .filter(|a| a.provider == provider)
        .find(|a| wanted.is_none_or(|w| &a.key() == w))
        .or_else(|| accounts.iter().find(|a| a.provider == provider))
        .cloned();

    match (provider, account) {
        (Provider::Microsoft, Some(acc)) => {
            let (acc, tokens) = account::session(&store, &acc).await?;
            if !acc.owns_java_edition {
                return Err(Error::Auth(format!(
                    "На аккаунте {} не куплена Minecraft: Java Edition — запуск невозможен.",
                    acc.name
                )));
            }
            emit(LaunchEvent::Account { provider: "Microsoft".into(), name: acc.name.clone() });
            Ok((
                GameAuth {
                    name: acc.name.clone(),
                    uuid: acc.uuid.clone(),
                    access_token: tokens.access_token,
                    user_type: "msa".into(),
                    xuid: tokens.xuid.unwrap_or_else(|| "0".into()),
                    client_id: account::microsoft::CLIENT_ID.unwrap_or_default().to_string(),
                },
                Some(acc),
            ))
        }
        (Provider::ElyBy, Some(acc)) => {
            let (acc, tokens) = account::session(&store, &acc).await?;
            emit(LaunchEvent::Account { provider: "Ely.by".into(), name: acc.name.clone() });
            Ok((
                GameAuth {
                    name: acc.name.clone(),
                    uuid: acc.uuid.clone(),
                    access_token: tokens.access_token,
                    user_type: "mojang".into(),
                    xuid: "0".into(),
                    client_id: String::new(),
                },
                Some(acc),
            ))
        }
        (Provider::Microsoft, None) if DEV_LAUNCH => {
            emit(LaunchEvent::Warning {
                message: "РЕЖИМ РАЗРАБОТЧИКА (dev-launch): запуск без входа, только для отладки на собственной копии игры. Онлайн-серверы недоступны.".into(),
            });
            Ok((
                GameAuth {
                    name: "QuarryDev".into(),
                    uuid: "00000000000000000000000000000000".into(),
                    access_token: "0".into(),
                    user_type: "legacy".into(),
                    xuid: "0".into(),
                    client_id: String::new(),
                },
                None,
            ))
        }
        (Provider::Microsoft, None) => Err(Error::Auth(
            "Нет аккаунта Microsoft. Войдите на экране «Аккаунты»: запуск возможен только с купленной Minecraft: Java Edition.".into(),
        )),
        (Provider::ElyBy, None) => Err(Error::Auth(
            "Профиль настроен на Ely.by, но аккаунта Ely.by нет. Войдите через Ely.by или переключите профиль на Microsoft.".into(),
        )),
    }
}

async fn choose_java(
    data: &DataDir,
    req: &Requirement,
    user_path: Option<&PathBuf>,
    cancel: &Cancel,
    on_event: &EventFn,
) -> Result<Choice> {
    if let Some(path) = user_path {
        let j = java::probe(path, JavaSource::User).await?;
        let mut reason = format!("указана в настройках: {}", j.label());
        if !req.accepts(&j) {
            reason.push_str(&format!(
                " — внимание: версии нужна Java {} ({}), игра может не запуститься",
                req.major, req.reason
            ));
        }
        return Ok(Choice { install: j, reason });
    }
    let installs = java::discover(Some(data)).await;
    if let Some(c) = java::choose(req, &installs) {
        return Ok(c);
    }
    on_event(InstallEvent {
        stage: Stage::Java,
        progress: None,
    });
    let cb = on_event.clone();
    let progress: crate::download::ProgressFn = Arc::new(move |p: &crate::download::Progress| {
        cb(InstallEvent {
            stage: Stage::Java,
            progress: Some(p.clone()),
        })
    });
    let j = match java::install_temurin(data, req.major, req.arch, Some(progress.clone()), cancel)
        .await
    {
        Ok(j) => j,
        // Нет сборки для этой архитектуры (Java 8 для Apple Silicon) — x64 через Rosetta.
        Err(Error::Java(_)) if req.fallback_arch.is_some() => {
            let fallback = req.fallback_arch.unwrap_or(req.arch);
            java::install_temurin(data, req.major, fallback, Some(progress), cancel).await?
        }
        Err(e) => return Err(e),
    };
    let mut reason = format!(
        "нужна Java {} ({}) — подходящей на компьютере нет, скачана {}",
        req.major,
        req.reason,
        j.label()
    );
    if let Some(a) = &req.arch_reason {
        reason.push_str("; ");
        reason.push_str(a);
    }
    if j.arch() != Some(req.arch) {
        reason.push_str("; сборки для Apple Silicon нет — используется x64-версия через Rosetta 2");
    }
    Ok(Choice { install: j, reason })
}

/// Подготовить и запустить профиль, дождаться завершения игры.
pub async fn launch(
    data: &DataDir,
    secrets: &dyn SecretStore,
    registry: &LaunchRegistry,
    instance_id: &str,
    options: LaunchOptions,
    emit: LaunchEventFn,
) -> Result<ExitInfo> {
    let cancel = Cancel::new();
    let _guard = registry.claim(instance_id, cancel.clone())?;
    let store = InstanceStore::new(data.clone());
    let mut inst = store.get(instance_id)?;
    let started_at = now();

    let result = run(
        data, secrets, registry, &store, &mut inst, &options, &cancel, &emit,
    )
    .await;

    // Итог запуска сохраняется в профиль: из него берётся статус на карточке.
    let last = match &result {
        Ok(info) => LastLaunch {
            at: started_at,
            outcome: if info.killed {
                LaunchOutcome::Killed
            } else if info.crashed {
                LaunchOutcome::Crashed
            } else {
                LaunchOutcome::Success
            },
            exit_code: info.code,
            duration_secs: info.duration_secs,
            message: info.crashed.then(|| match (&info.analysis, info.code) {
                (Some(a), _) => a.title.clone(),
                (None, Some(c)) => format!("игра завершилась с кодом {c}"),
                (None, None) => "игра завершилась аварийно".into(),
            }),
            log_file: Some(info.log_file.clone()),
        },
        Err(Error::Cancelled) => return result,
        Err(e) => LastLaunch {
            at: started_at,
            outcome: LaunchOutcome::FailedToStart,
            exit_code: None,
            duration_secs: 0,
            message: Some(e.to_string()),
            log_file: None,
        },
    };
    if let Ok(mut fresh) = store.get(instance_id) {
        fresh.last_launch = Some(last);
        if fresh.loader_version.is_none() {
            fresh.loader_version = inst.loader_version.clone();
        }
        let _ = store.save(&fresh);
    }
    result
}

#[allow(clippy::too_many_arguments)]
async fn run(
    data: &DataDir,
    secrets: &dyn SecretStore,
    registry: &LaunchRegistry,
    store: &InstanceStore,
    inst: &mut Instance,
    options: &LaunchOptions,
    cancel: &Cancel,
    emit: &LaunchEventFn,
) -> Result<ExitInfo> {
    let global = GlobalSettings::load(data)?;
    let stage = |s: Stage| {
        emit(LaunchEvent::Stage {
            stage: s,
            label: s.label(),
        })
    };

    stage(Stage::Metadata);
    let (auth, account) = resolve_auth(data, secrets, inst, &global, emit).await?;
    cancel.check()?;

    // Совместимость загрузчика проверяется до скачивания.
    let loader_version = crate::loader::resolve_version(
        inst.loader,
        &inst.game_version,
        inst.loader_version.as_deref(),
    )
    .await
    .or_else(|e| match &inst.loader_version {
        // Без сети: доверяем уже сохранённой версии загрузчика.
        Some(v) if e.is_retryable() || matches!(e, Error::Network { .. }) => Ok(Some(v.clone())),
        _ => Err(e),
    })?;
    inst.loader_version = loader_version.clone();
    let mut version = crate::loader::resolve_profile(
        data,
        inst.loader,
        &inst.game_version,
        loader_version.as_deref(),
    )
    .await?;
    for note in
        crate::install::apply_platform_fixes(data, &mut version, &Environment::current()).await?
    {
        emit(LaunchEvent::Warning { message: note });
    }
    cancel.check()?;

    let event_cb = emit.clone();
    let on_event: EventFn = Arc::new(move |e: InstallEvent| match e.progress {
        Some(p) => event_cb(LaunchEvent::Progress {
            stage: e.stage,
            progress: p,
        }),
        None => event_cb(LaunchEvent::Stage {
            stage: e.stage,
            label: e.stage.label(),
        }),
    });
    let env = Environment::current();
    let game_dir = store.game_dir(&inst.id)?;
    let installer = Installer {
        data,
        env: env.clone(),
        verify: if options.full_verify {
            Verify::Full
        } else {
            Verify::Quick
        },
        cancel: cancel.clone(),
        on_event: Some(on_event.clone()),
    };
    if let Some(note) = recover_safe_mode(&game_dir) {
        emit(LaunchEvent::Warning { message: note });
    }
    // Mod Doctor до запуска: объявленные конфликты и недостающие моды останавливают запуск
    // сразу, а не после минуты загрузки.
    if inst.loader != crate::loader::LoaderKind::Vanilla && !options.safe_mode {
        let files = crate::mods::inventory(&game_dir, &store.dir(&inst.id)?)?;
        let report = crate::mods::doctor::check(
            &files,
            inst.loader,
            &inst.game_version,
            inst.loader_version.as_deref(),
        );
        if !report.can_launch {
            let list: Vec<String> = report
                .findings
                .iter()
                .filter(|f| f.severity == crate::mods::doctor::Severity::Blocking)
                .map(|f| format!("• {}", f.title))
                .collect();
            return Err(Error::invalid(format!(
                "Mod Doctor нашёл проблемы, из-за которых игра не запустится:\n{}\nИсправьте их на вкладке «Моды» или запустите профиль без модов.",
                list.join("\n")
            )));
        }
        for f in report.findings.iter().take(3) {
            emit(LaunchEvent::Warning {
                message: format!("Mod Doctor: {}", f.title),
            });
        }
    }
    let installed = installer
        .install(version, &inst.game_version, &game_dir)
        .await?;
    if !installed.unverified.is_empty() {
        emit(LaunchEvent::Warning {
            message: format!(
                "{} файл(ов) скачано без проверки хэша: источник его не публикует",
                installed.unverified.len()
            ),
        });
    }

    let (major, reason) = installed.version.required_java();
    let req = Requirement::for_version(major, reason, installed.version.has_macos_arm64_natives());
    let resolved = settings::resolve(&inst.settings, &inst.pack_settings, &global.launch);
    let java = choose_java(
        data,
        &req,
        resolved.java_path.value.as_ref(),
        cancel,
        &on_event,
    )
    .await?;
    emit(LaunchEvent::Java {
        label: java.install.label(),
        reason: java.reason.clone(),
    });
    cancel.check()?;

    // NeoForge: процессоры установщика нужны Java игры и скачанный ванильный клиент.
    if inst.loader == crate::loader::LoaderKind::NeoForge {
        let lv = loader_version
            .as_deref()
            .ok_or_else(|| Error::invalid("Не указана версия NeoForge"))?;
        stage(Stage::Loader);
        crate::loader::neoforge::run_processors(
            data,
            &inst.game_version,
            lv,
            &java.install.path,
            &installed.client_jar,
            cancel,
        )
        .await?;
    }

    let authlib = if inst.auth_mode == AuthMode::ElyBy {
        let jar = account::ely::ensure_authlib_injector(data, cancel).await?;
        Some((jar, account::ely::AUTHLIB_API.to_string()))
    } else {
        None
    };

    let spec = LaunchSpec {
        installed: &installed,
        java: &java.install.path,
        game_dir: &game_dir,
        auth: &auth,
        env,
        min_memory_mb: resolved.min_memory_mb.value,
        max_memory_mb: resolved.max_memory_mb.value,
        extra_jvm_args: resolved.jvm_args.value.clone(),
        resolution: resolved.resolution.value,
        quick_play: options.quick_play.clone(),
        authlib_injector: authlib,
        launcher_name: LAUNCHER_NAME,
        launcher_version: LAUNCHER_VERSION,
    };
    let cmd = args::build(&spec);
    if !cmd.unresolved.is_empty() {
        emit(LaunchEvent::Warning {
            message: format!(
                "Неизвестные переменные в аргументах версии: {}",
                cmd.unresolved.join(", ")
            ),
        });
    }

    let masker = Masker::new()
        .with_secret(&auth.access_token)
        .with_nick(global.streamer_mode.then_some(auth.name.as_str()));
    let logs_dir = store.logs_dir(&inst.id)?;
    rotate_logs(&logs_dir, 20);
    let log_file = logs_dir.join(format!("game-{}.log", now()));
    let preview = masker.mask(&cmd.preview(&auth));
    let line_cb = emit.clone();
    let game_cb: process::GameEventFn = Arc::new(move |e| match e {
        GameEvent::Log { line, .. } => line_cb(LaunchEvent::Log { line }),
        GameEvent::Started { .. } => {}
    });
    let safe_guard = if options.safe_mode && game_dir.join("mods").is_dir() {
        let aside = game_dir.join(SAFE_MODE_DIR);
        std::fs::rename(game_dir.join("mods"), &aside).map_err(|e| Error::io(&aside, e))?;
        emit(LaunchEvent::Warning {
            message: "Безопасный запуск: моды отложены на время игры и вернутся после выхода"
                .into(),
        });
        Some(SafeModeGuard {
            game_dir: game_dir.clone(),
        })
    } else {
        None
    };
    let proc = process::spawn(&cmd, &log_file, masker, Some(game_cb)).await?;
    registry.set_stop(&inst.id, proc.stopper());
    emit(LaunchEvent::Started {
        pid: proc.pid,
        command: preview,
    });
    let _ = account; // аккаунт уже показан событием Account
    let mut info = proc.wait().await?;
    drop(safe_guard);
    if info.crashed && !options.safe_mode {
        let mut lines = info.tail.clone();
        if let Some(cr) = &info.crash_report {
            lines.extend(crate::mods::crash::read_crash_report(cr));
        }
        let files = crate::mods::inventory(&game_dir, &store.dir(&inst.id)?).unwrap_or_default();
        let packages = crate::mods::crash::package_index(&game_dir, &files).unwrap_or_default();
        info.analysis = Some(crate::mods::crash::analyze(&lines, &files, &packages));
    }
    emit(LaunchEvent::Exited { info: info.clone() });
    Ok(info)
}

/// Итоговая команда запуска без запуска и без скачивания (кроме метаданных): для вкладки
/// «Запуск и память». Токен не показывается; если подходящей Java нет, это сказано в команде.
pub async fn preview(
    data: &DataDir,
    secrets: &dyn SecretStore,
    instance_id: &str,
) -> Result<String> {
    let store = InstanceStore::new(data.clone());
    let inst = store.get(instance_id)?;
    let global = GlobalSettings::load(data)?;
    let mut version = crate::loader::resolve_profile(
        data,
        inst.loader,
        &inst.game_version,
        inst.loader_version.as_deref(),
    )
    .await?;
    let env = Environment::current();
    crate::install::apply_platform_fixes(data, &mut version, &env).await?;
    let installer = Installer {
        data,
        env: env.clone(),
        verify: Verify::Quick,
        cancel: Cancel::new(),
        on_event: None,
    };
    let plan = installer.plan(&version, &inst.game_version)?;
    let natives_dir = installer.natives_dir_for(&version.id, &plan.native_jars)?;
    let (major, reason) = version.required_java();
    let req = Requirement::for_version(major, reason, version.has_macos_arm64_natives());
    let resolved = settings::resolve(&inst.settings, &inst.pack_settings, &global.launch);
    let java_path = match resolved.java_path.value.clone() {
        Some(p) => p,
        None => match java::choose(&req, &java::discover(Some(data)).await) {
            Some(c) => c.install.path,
            None => PathBuf::from(format!("‹будет скачана Java {}›", req.major)),
        },
    };
    let accounts = AccountStore::new(data, secrets).list().unwrap_or_default();
    let name = accounts
        .iter()
        .find(|a| match inst.auth_mode {
            AuthMode::Microsoft => a.provider == Provider::Microsoft,
            AuthMode::ElyBy => a.provider == Provider::ElyBy,
        })
        .map(|a| a.name.clone())
        .unwrap_or_else(|| "‹ник›".into());
    let auth = GameAuth {
        name,
        uuid: "‹uuid›".into(),
        access_token: "‹token›".into(),
        user_type: "msa".into(),
        xuid: "0".into(),
        client_id: account::microsoft::CLIENT_ID
            .unwrap_or_default()
            .to_string(),
    };
    let installed = crate::install::Installed {
        assets_index_name: plan.index_ref.id.clone(),
        version,
        client_jar: plan.client_jar,
        libraries: plan.libraries,
        natives_dir,
        assets_root: data.assets(),
        game_assets: data.assets(),
        logging_argument: plan.logging_argument,
        unverified: Vec::new(),
    };
    let game_dir = store.game_dir(&inst.id)?;
    let authlib = (inst.auth_mode == AuthMode::ElyBy).then(|| {
        (
            data.libraries()
                .join("moe/yushi/authlibinjector")
                .join(account::ely::AUTHLIB_VERSION)
                .join(format!(
                    "authlib-injector-{}.jar",
                    account::ely::AUTHLIB_VERSION
                )),
            account::ely::AUTHLIB_API.to_string(),
        )
    });
    let cmd = args::build(&LaunchSpec {
        installed: &installed,
        java: &java_path,
        game_dir: &game_dir,
        auth: &auth,
        env,
        min_memory_mb: resolved.min_memory_mb.value,
        max_memory_mb: resolved.max_memory_mb.value,
        extra_jvm_args: resolved.jvm_args.value,
        resolution: resolved.resolution.value,
        quick_play: None,
        authlib_injector: authlib,
        launcher_name: LAUNCHER_NAME,
        launcher_version: LAUNCHER_VERSION,
    });
    Ok(Masker::new().mask(&cmd.preview(&auth)))
}

/// Хранить не больше `keep` последних логов игры профиля.
fn rotate_logs(dir: &std::path::Path, keep: usize) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut logs: Vec<PathBuf> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("game-"))
        })
        .collect();
    if logs.len() < keep {
        return;
    }
    logs.sort();
    for old in &logs[..logs.len() + 1 - keep] {
        let _ = std::fs::remove_file(old);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_flight() {
        let reg = LaunchRegistry::default();
        let g = reg.claim("p", Cancel::new()).unwrap();
        assert!(matches!(
            reg.claim("p", Cancel::new()),
            Err(Error::AlreadyRunning(_))
        ));
        assert!(
            reg.claim("other", Cancel::new()).is_ok(),
            "другой профиль запускается параллельно"
        );
        assert!(reg.is_running("p"));
        drop(g);
        assert!(!reg.is_running("p"));
        assert!(reg.claim("p", Cancel::new()).is_ok());
    }

    #[test]
    fn stop_cancels_preparation() {
        let reg = LaunchRegistry::default();
        let cancel = Cancel::new();
        let _g = reg.claim("p", cancel.clone()).unwrap();
        assert!(reg.stop("p"));
        assert!(cancel.is_cancelled());
        assert!(!reg.stop("missing"));
    }

    #[test]
    fn safe_mode_mods_come_back() {
        let d = std::env::temp_dir().join(format!("quarry-safe-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join(SAFE_MODE_DIR)).unwrap();
        std::fs::write(d.join(SAFE_MODE_DIR).join("a.jar"), b"x").unwrap();
        {
            let _g = SafeModeGuard {
                game_dir: d.clone(),
            };
        }
        assert!(d.join("mods/a.jar").exists());
        assert!(recover_safe_mode(&d).is_none());
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn log_rotation_keeps_newest() {
        let d = std::env::temp_dir().join(format!("quarry-rotate-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        for i in 0..5 {
            std::fs::write(d.join(format!("game-{i}.log")), b"").unwrap();
        }
        rotate_logs(&d, 3);
        let mut left: Vec<_> = std::fs::read_dir(&d)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().into_string().unwrap())
            .collect();
        left.sort();
        assert_eq!(left, ["game-3.log", "game-4.log"], "место под новый лог");
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[tokio::test]
    async fn no_account_means_no_launch_in_release_builds() {
        if DEV_LAUNCH {
            return;
        }
        let root = std::env::temp_dir().join(format!("quarry-noacc-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let data = DataDir::open(&root).unwrap();
        let store = InstanceStore::new(data.clone());
        let inst = store
            .create("Test", "26.3", crate::loader::LoaderKind::Vanilla, None)
            .unwrap();
        let secrets = account::MemoryStore::default();
        let reg = LaunchRegistry::default();
        let emit: LaunchEventFn = Arc::new(|_| {});
        let err = launch(
            &data,
            &secrets,
            &reg,
            &inst.id,
            LaunchOptions::default(),
            emit,
        )
        .await
        .unwrap_err();
        assert!(matches!(err, Error::Auth(_)), "{err}");
        let saved = store.get(&inst.id).unwrap();
        assert_eq!(
            saved.last_launch.unwrap().outcome,
            LaunchOutcome::FailedToStart
        );
        assert!(!reg.is_running(&inst.id));
        std::fs::remove_dir_all(&root).unwrap();
    }
}
