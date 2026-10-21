//! CLI лаунчера: установка, проверка и запуск профилей без интерфейса.

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use quarry_core::account::{AccountStore, KeyringStore, Provider, account_key};
use quarry_core::download::Cancel;
use quarry_core::instance::InstanceStore;
use quarry_core::launch::{self, LaunchEvent, LaunchOptions, LaunchRegistry, args::QuickPlay};
use quarry_core::loader::{self, LoaderKind};
use quarry_core::paths::DataDir;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Parser)]
#[command(
    name = "quarry",
    version,
    about = "Quarry — открытый лаунчер Minecraft Java Edition"
)]
struct Cli {
    /// Папка данных (по умолчанию ~/.quarry)
    #[arg(long, global = true)]
    root: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Список версий Minecraft из манифеста Mojang
    Versions {
        #[arg(long)]
        snapshots: bool,
        /// Показать также alpha/beta
        #[arg(long)]
        old: bool,
        #[arg(long, default_value = "30")]
        limit: usize,
    },
    /// Версии загрузчика для версии игры
    Loaders {
        loader: String,
        game_version: String,
    },
    /// Найденные на компьютере Java
    Java,
    /// Профили
    Instance {
        #[command(subcommand)]
        action: InstanceAction,
    },
    /// Подготовить и запустить профиль
    Launch {
        id: String,
        /// Сразу в мир (имя папки в saves/)
        #[arg(long, conflicts_with = "server")]
        world: Option<String>,
        /// Сразу на сервер (адрес[:порт])
        #[arg(long)]
        server: Option<String>,
        /// Проверить хэши всех файлов
        #[arg(long)]
        verify: bool,
        /// Безопасный запуск: без модов (моды вернутся после выхода)
        #[arg(long)]
        safe: bool,
    },
    /// Итоговая команда запуска профиля (без токена и без запуска)
    Preview { id: String },
    /// Импорт: папка Prism/MultiMC, .mrpack или zip CurseForge
    Import { path: PathBuf },
    /// Моды профиля и проверка Mod Doctor
    Mods {
        id: String,
        #[command(subcommand)]
        action: Option<ModsAction>,
    },
    /// Миры профиля и их бэкапы
    Worlds { id: String },
    /// Снимки профиля
    Snapshots {
        id: String,
        /// Откатить к снимку с этим ID
        #[arg(long)]
        restore: Option<String>,
    },
    /// Аккаунты
    Account {
        #[command(subcommand)]
        action: AccountAction,
    },
}

#[derive(Subcommand)]
enum ModsAction {
    /// Установить мод с Modrinth (slug или ID) вместе с зависимостями
    Install { project: String },
    /// Проверить и установить обновления
    Update {
        #[arg(long)]
        apply: bool,
    },
    /// Удалить мод (перед этим создаётся снимок)
    Remove { file: String },
}

#[derive(Subcommand)]
enum InstanceAction {
    Create {
        name: String,
        #[arg(short, long)]
        version: String,
        #[arg(long, default_value = "vanilla")]
        loader: String,
        #[arg(long)]
        loader_version: Option<String>,
    },
    List,
    /// Убрать профиль в корзину (миры сохраняются)
    Trash {
        id: String,
    },
}

#[derive(Subcommand)]
enum AccountAction {
    List,
    /// Вход Microsoft (код на microsoft.com/link)
    LoginMicrosoft,
    /// Вход Ely.by логином и паролем (только при Microsoft-аккаунте с купленной игрой).
    /// Пароль читается из stdin, а не из аргументов — чтобы не попасть в историю оболочки.
    LoginEly {
        login: String,
    },
    /// Вход Ely.by через браузер (OAuth)
    LoginElyBrowser,
    /// Выйти: provider = microsoft | elyby
    Logout {
        provider: String,
        uuid: String,
    },
}

fn data_dir(root: Option<PathBuf>) -> Result<DataDir> {
    let root = root
        .or_else(DataDir::default_root)
        .context("Не удалось определить домашнюю папку")?;
    Ok(DataDir::open(root)?)
}

fn mb(bytes: u64) -> f64 {
    bytes as f64 / 1_048_576.0
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let data = data_dir(cli.root)?;
    match cli.command {
        Command::Versions {
            snapshots,
            old,
            limit,
        } => {
            let (m, fresh) = quarry_core::metadata::manifest(&data).await?;
            if fresh == quarry_core::metadata::Freshness::Cached {
                eprintln!("Нет сети — показан сохранённый список.");
            }
            println!("{:<24} {:<10} Дата", "Версия", "Тип");
            let list = m.filtered(snapshots, old);
            for v in list.iter().take(limit) {
                println!(
                    "{:<24} {:<10} {}",
                    v.id,
                    v.kind,
                    v.release_time.get(..10).unwrap_or("")
                );
            }
            println!("\nВсего: {}", list.len());
        }
        Command::Loaders {
            loader,
            game_version,
        } => {
            let kind = LoaderKind::parse(&loader)?;
            let list = loader::versions(kind, &game_version).await?;
            if list.is_empty() {
                println!(
                    "{} не поддерживает Minecraft {game_version}",
                    kind.display()
                );
            }
            for v in list.iter().take(20) {
                println!(
                    "{}{}",
                    v.version,
                    if v.stable {
                        ""
                    } else {
                        "  (нестабильная)"
                    }
                );
            }
        }
        Command::Java => {
            for j in quarry_core::java::discover(Some(&data)).await {
                println!(
                    "Java {:<3} {:<40} {:?}  {}",
                    j.major,
                    j.label(),
                    j.source,
                    j.path.display()
                );
            }
        }
        Command::Instance { action } => {
            let store = InstanceStore::new(data.clone());
            match action {
                InstanceAction::Create {
                    name,
                    version,
                    loader: l,
                    loader_version,
                } => {
                    let kind = LoaderKind::parse(&l)?;
                    let lv =
                        loader::resolve_version(kind, &version, loader_version.as_deref()).await?;
                    let i = store.create(&name, &version, kind, lv)?;
                    println!(
                        "Создан профиль «{}» (ID {}): Minecraft {} · {}{}",
                        i.name,
                        i.id,
                        i.game_version,
                        i.loader.display(),
                        i.loader_version
                            .map(|v| format!(" {v}"))
                            .unwrap_or_default()
                    );
                }
                InstanceAction::List => {
                    let (list, broken) = store.list()?;
                    if list.is_empty() && broken.is_empty() {
                        println!("Профилей нет.");
                    }
                    for i in list {
                        let last = i
                            .last_launch
                            .map(|l| format!("{:?}", l.outcome))
                            .unwrap_or_else(|| "ещё не запускался".into());
                        println!(
                            "{:<24} {:<28} {:<10} {:<8} {}",
                            i.id,
                            i.name,
                            i.game_version,
                            i.loader.display(),
                            last
                        );
                    }
                    for b in broken {
                        eprintln!("Не прочитан {}: {}", b.dir.display(), b.reason);
                    }
                }
                InstanceAction::Trash { id } => {
                    let to = store.trash(&id)?;
                    println!("Профиль убран в корзину: {}", to.display());
                }
            }
        }
        Command::Launch {
            id,
            world,
            server,
            verify,
            safe,
        } => {
            let quick_play = world
                .map(QuickPlay::Singleplayer)
                .or(server.map(QuickPlay::Multiplayer));
            let emit: launch::LaunchEventFn = Arc::new(|e| match e {
                LaunchEvent::Stage { label, .. } => eprintln!("→ {label}"),
                LaunchEvent::Progress { progress: p, .. } => {
                    eprint!(
                        "\r   {}/{} файлов, {:.1}/{:.1} МБ   ",
                        p.files_done,
                        p.files_total,
                        mb(p.bytes_done),
                        mb(p.bytes_total)
                    );
                    let _ = std::io::stderr().flush();
                    if p.files_done == p.files_total {
                        eprintln!();
                    }
                }
                LaunchEvent::Account { provider, name } => {
                    eprintln!("Аккаунт: {name} ({provider})")
                }
                LaunchEvent::Java { label, reason } => eprintln!("Java: {label}\n   {reason}"),
                LaunchEvent::Warning { message } => eprintln!("⚠ {message}"),
                LaunchEvent::Started { pid, command } => {
                    eprintln!("Игра запущена (PID {:?})\n   {command}", pid)
                }
                LaunchEvent::Log { line } => println!("{line}"),
                LaunchEvent::Exited { info } => eprintln!(
                    "Игра завершилась: код {:?}, {} с{}",
                    info.code,
                    info.duration_secs,
                    if info.crashed { " — краш" } else { "" }
                ),
            });
            let registry = LaunchRegistry::default();
            let info = launch::launch(
                &data,
                &KeyringStore,
                &registry,
                &id,
                LaunchOptions {
                    quick_play,
                    full_verify: verify,
                    safe_mode: safe,
                },
                emit,
            )
            .await?;
            eprintln!("Лог: {}", info.log_file.display());
            if let Some(cr) = info.crash_report {
                eprintln!("Crash report: {}", cr.display());
            }
        }
        Command::Preview { id } => {
            println!("{}", launch::preview(&data, &KeyringStore, &id).await?);
        }
        Command::Import { path } => {
            let store = InstanceStore::new(data.clone());
            let r = if path.is_dir() {
                quarry_core::import::import_mmc(&store, &path)?
            } else if path.extension().is_some_and(|e| e == "mrpack") {
                quarry_core::import::import_mrpack(&store, &path, &Cancel::new()).await?
            } else {
                quarry_core::import::import_curseforge(&store, &path)?
            };
            println!(
                "Импортирован профиль «{}» (ID {}): {} · {}",
                r.instance.name,
                r.instance.id,
                r.instance.game_version,
                r.instance.loader.display()
            );
            for n in r.notes {
                println!("  {n}");
            }
            for m in r.manual {
                println!(
                    "  скачать вручную: проект {} файл {} — {}",
                    m.project_id, m.file_id, m.url
                );
            }
        }
        Command::Mods { id, action } => {
            let store = InstanceStore::new(data.clone());
            match action {
                None => {
                    let inst = store.get(&id)?;
                    let files =
                        quarry_core::mods::inventory(&store.game_dir(&id)?, &store.dir(&id)?)?;
                    let report = quarry_core::mods::doctor::check(
                        &files,
                        inst.loader,
                        &inst.game_version,
                        inst.loader_version.as_deref(),
                    );
                    for (f, r) in files.iter().zip(&report.mods) {
                        println!(
                            "{:<28} {:<14} {:<10} {}",
                            f.display_name(),
                            f.primary().map(|m| m.version.as_str()).unwrap_or("?"),
                            f.source
                                .as_ref()
                                .map(|s| format!("{:?}", s.source))
                                .unwrap_or_else(|| "вручную".into()),
                            r.state_label
                        );
                    }
                    println!(
                        "
Всего {}: без замечаний {}, с предупреждением {}, блокируют запуск {}",
                        report.total, report.ok, report.warnings, report.blocked
                    );
                    for f in &report.findings {
                        println!(
                            "{} {}
   {}
   основание: {}",
                            if f.severity == quarry_core::mods::doctor::Severity::Blocking {
                                "✖"
                            } else {
                                "⚠"
                            },
                            f.title,
                            f.detail,
                            f.evidence
                        );
                    }
                }
                Some(ModsAction::Install { project }) => {
                    let plan = quarry_core::mods::manage::plan_install(
                        &store,
                        &id,
                        &project,
                        quarry_core::catalog::Channel::Auto,
                    )
                    .await?;
                    for c in &plan.changes {
                        println!(
                            "+ {} {} ({})",
                            c.title,
                            c.new_version.clone().unwrap_or_default(),
                            c.reason
                        );
                    }
                    for n in &plan.notes {
                        println!("  {n}");
                    }
                    let snap = quarry_core::mods::manage::apply(
                        &store,
                        &id,
                        plan,
                        &format!("Перед установкой {project}"),
                        &Cancel::new(),
                    )
                    .await?;
                    println!("Готово. Снимок до изменений: {}", snap.id);
                }
                Some(ModsAction::Update { apply }) => {
                    let plan = quarry_core::mods::manage::check_updates(&store, &id).await?;
                    for c in &plan.changes {
                        println!(
                            "~ {} {} → {}",
                            c.title,
                            c.old_version.clone().unwrap_or_default(),
                            c.new_version.clone().unwrap_or_default()
                        );
                    }
                    for n in &plan.notes {
                        println!("  {n}");
                    }
                    if plan.changes.is_empty() {
                        println!("Обновлений нет.");
                    } else if apply {
                        let snap = quarry_core::mods::manage::apply(
                            &store,
                            &id,
                            plan,
                            "Перед обновлением модов",
                            &Cancel::new(),
                        )
                        .await?;
                        println!("Обновлено. Снимок до изменений: {}", snap.id);
                    }
                }
                Some(ModsAction::Remove { file }) => {
                    let snap =
                        quarry_core::mods::manage::remove(&store, &id, &file, "удалено через CLI")?;
                    println!(
                        "Удалено. Вернуть: quarry snapshots {id} --restore {}",
                        snap.id
                    );
                }
            }
        }
        Command::Worlds { id } => {
            let store = InstanceStore::new(data.clone());
            let game = store.game_dir(&id)?;
            let backups = quarry_core::worlds::backups_dir(&data, &id);
            for w in quarry_core::worlds::list(&game, &backups) {
                println!(
                    "{:<24} {:<10} {:<12} DataVersion {:<6} {:.1} МБ, бэкапов {}",
                    w.name,
                    w.version_name.unwrap_or_default(),
                    w.game_mode,
                    w.data_version.map(|v| v.to_string()).unwrap_or_default(),
                    w.size as f64 / 1_048_576.0,
                    w.backups.len()
                );
            }
        }
        Command::Snapshots { id, restore } => {
            let store = InstanceStore::new(data.clone());
            let (dir, game) = (store.dir(&id)?, store.game_dir(&id)?);
            if let Some(sid) = restore {
                let before = quarry_core::snapshot::restore(&dir, &game, &sid)?;
                println!(
                    "Откат выполнен. Состояние до отката сохранено: {}",
                    before.id
                );
            } else {
                for s in quarry_core::snapshot::list(&dir)? {
                    println!(
                        "{:<44} {}{} (модов {}, конфигов {})",
                        s.id,
                        s.reason,
                        if s.pinned {
                            " · закреплён"
                        } else {
                            ""
                        },
                        s.mods,
                        s.configs
                    );
                }
            }
        }
        Command::Account { action } => {
            let secrets = KeyringStore;
            let store = AccountStore::new(&data, &secrets);
            match action {
                AccountAction::List => {
                    for a in store.list()? {
                        println!(
                            "{:<10} {:<20} {}  {}",
                            a.provider.display(),
                            a.name,
                            a.uuid,
                            match (a.provider, a.owns_java_edition) {
                                (Provider::Microsoft, true) => "Java Edition куплена",
                                (Provider::Microsoft, false) => "Java Edition НЕ куплена",
                                (Provider::ElyBy, _) => "только совместимые серверы",
                            }
                        );
                    }
                }
                AccountAction::LoginMicrosoft => {
                    let code = quarry_core::account::microsoft::start_device_code().await?;
                    println!(
                        "Откройте {} и введите код {}",
                        code.verification_uri, code.user_code
                    );
                    let _ = open::that(&code.verification_uri);
                    let acc = quarry_core::account::microsoft::complete_login(
                        &store,
                        &code,
                        &Cancel::new(),
                    )
                    .await?;
                    println!(
                        "Вход выполнен: {} — {}",
                        acc.name,
                        if acc.owns_java_edition {
                            "Java Edition куплена"
                        } else {
                            "Java Edition не куплена"
                        }
                    );
                }
                AccountAction::LoginEly { login } => {
                    use quarry_core::account::ely::{PasswordLogin, login_password};
                    eprint!("Пароль Ely.by (ввод виден; передаётся только на authserver.ely.by): ");
                    let mut password = String::new();
                    std::io::stdin().read_line(&mut password)?;
                    let password = password.trim_end_matches(['\r', '\n']).to_string();
                    let acc = match login_password(&store, &login, &password, None).await? {
                        PasswordLogin::Done(a) => a,
                        PasswordLogin::NeedTotp => {
                            eprint!("Код двухфакторной защиты: ");
                            let mut code = String::new();
                            std::io::stdin().read_line(&mut code)?;
                            match login_password(&store, &login, &password, Some(code.trim()))
                                .await?
                            {
                                PasswordLogin::Done(a) => a,
                                PasswordLogin::NeedTotp => anyhow::bail!("Код не принят"),
                            }
                        }
                    };
                    println!("Вход Ely.by: {} (только совместимые серверы)", acc.name);
                }
                AccountAction::LoginElyBrowser => {
                    let acc = quarry_core::account::ely::login(
                        &store,
                        |url| {
                            println!("Открываем браузер для входа Ely.by…");
                            open::that(url).map_err(|e| {
                                quarry_core::Error::Auth(format!("Не удалось открыть браузер: {e}"))
                            })
                        },
                        &Cancel::new(),
                    )
                    .await?;
                    println!("Вход Ely.by: {} (только совместимые серверы)", acc.name);
                }
                AccountAction::Logout { provider, uuid } => {
                    let p = match provider.as_str() {
                        "microsoft" => Provider::Microsoft,
                        "elyby" => Provider::ElyBy,
                        other => anyhow::bail!("Неизвестный провайдер: {other}"),
                    };
                    store.remove(&account_key(p, &uuid))?;
                    println!("Аккаунт удалён из лаунчера.");
                }
            }
        }
    }
    Ok(())
}
