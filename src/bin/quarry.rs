use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use quarry_core::account::{load_active_account, AccountKind};
use quarry_core::profile::ProfileStore;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "quarry", version, about = "Minecraft Java launcher")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Profile {
        #[command(subcommand)]
        action: ProfileAction,
    },
    Versions {
        #[arg(long)]
        all: bool,
        #[arg(long, default_value = "30")]
        limit: usize,
    },
    Launch {
        profile_id: String,
        #[arg(short, long, default_value = "Player")]
        username: String,
        #[arg(long)]
        root: Option<PathBuf>,
        /// Использовать аккаунт Ely.by (включает authlib-injector)
        #[arg(long)]
        ely: bool,
    },
    /// Войти в аккаунт Ely.by через браузер
    LoginEly,
    /// Скачать скин и аватар Ely.by по нику
    SkinGet {
        username: String,
    },
}

#[derive(Subcommand)]
enum ProfileAction {
    Create {
        name: String,
        #[arg(short, long)]
        version: String,
        #[arg(long)]
        loader: Option<String>,
        #[arg(long)]
        root: Option<PathBuf>,
    },
    List {
        #[arg(long)]
        root: Option<PathBuf>,
    },
    Delete {
        id: String,
        #[arg(long)]
        root: Option<PathBuf>,
    },
}

fn data_root(root: Option<PathBuf>) -> Result<PathBuf> {
    let r = root
        .or_else(|| dirs::home_dir().map(|h| h.join(".quarry")))
        .context("Не удалось определить домашнюю папку")?;
    std::fs::create_dir_all(&r)?;
    Ok(r)
}

fn to_anyhow(e: Box<dyn std::error::Error + Send + Sync>) -> anyhow::Error {
    anyhow::anyhow!("{e}")
}

async fn ensure_authlib_injector(root: &PathBuf) -> Result<PathBuf> {
    let cache_dir = root.join("libraries");
    std::fs::create_dir_all(&cache_dir)?;
    let jar_path = cache_dir.join("authlib-injector.jar");

    if jar_path.exists() {
        return Ok(jar_path);
    }

    println!("Скачиваем authlib-injector.jar...");
    let client = reqwest::Client::new();
    
    // Получаем URL последней версии с официального API
    let meta_resp = client.get("https://authlib-injector.yushi.moe/artifact/latest.json")
        .send().await?
        .json::<serde_json::Value>().await?;
    
    let download_url = meta_resp["download_url"].as_str()
        .context("Не удалось найти download_url в ответе API")?;

    let bytes = client.get(download_url).send().await?.bytes().await?;
    
    if bytes.len() < 2 || &bytes[..2] != b"PK" {
        anyhow::bail!("Скачанный файл не является JAR-архивом");
    }

    std::fs::write(&jar_path, &bytes)?;
    println!("authlib-injector скачан: {}", jar_path.display());
    Ok(jar_path)
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Versions { all, limit } => {
            let manifest = quarry_core::metadata::fetch_manifest().await.map_err(to_anyhow)?;
            let filtered = quarry_core::metadata::filter_versions(&manifest.versions, all);
            println!("{:<25} {:<15} {}", "ID", "Тип", "Дата релиза");
            for v in filtered.iter().take(limit) {
                let date = v.release_time.get(0..10).unwrap_or(&v.release_time);
                println!("{:<25} {:<15} {}", v.id, v.version_type, date);
            }
            println!("\nВсего: {}", filtered.len());
        }
        Commands::SkinGet { username } => {
            println!("Скачиваем скин для '{}'...", username);
            match quarry_core::skin::download_ely_skin(&username).await {
                Ok(path) => println!("Скин сохранен: {}", path.display()),
                Err(e) => eprintln!("Ошибка скачивания скина: {:#}", e),
            }
            match quarry_core::skin::download_ely_avatar(&username).await {
                Ok(path) => println!("Аватарка сохранена: {}", path.display()),
                Err(e) => eprintln!("Ошибка создания аватарки: {:#}", e),
            }
        }
        Commands::LoginEly => {
            println!("Запуск авторизации Ely.by...");
            match quarry_core::account::ely::login().await {
                Ok(acc) => {
                    println!("Успешный вход!");
                    println!("Ник: {}", acc.username);
                    println!("UUID: {}", acc.uuid);
                    println!("Токен сохранён.");
                }
                Err(e) => eprintln!("Ошибка входа: {:#}", e),
            }
        }
        Commands::Launch { profile_id, username, root, ely } => {
            let root = data_root(root)?;
            let store = ProfileStore::new(root.clone());
            let profile = store.get(&profile_id).map_err(to_anyhow)?;
            let game_dir = store.game_dir(&profile_id);
            println!("Запускаем '{}' ({})", profile.name, profile.mc_version);

            let (auth_name, auth_uuid, auth_token, authlib_jar) = if ely {
                let acc = load_active_account()?.context("Аккаунт Ely.by не найден. Сначала выполните login-ely.")?;
                if acc.kind != AccountKind::ElyBy {
                    anyhow::bail!("Активный аккаунт не является Ely.by");
                }
                let jar = ensure_authlib_injector(&root).await?;
                let token = quarry_core::account::get_token_from_keyring(&format!("ely_{}", acc.uuid))?;
                (acc.username, acc.uuid, token, Some(jar))
            } else {
                (username, "00000000000000000000000000000001".into(), "0".into(), None)
            };

            let inst = quarry_core::instance::prepare(
                &profile.mc_version,
                &root,
                &game_dir,
                profile.loader.as_deref(),
            )
            .await
            .map_err(to_anyhow)?;

            let cfg = quarry_core::launch::LaunchConfig {
                java_path: inst.java_path.clone(),
                main_class: inst.main_class.clone(),
                classpath: inst.classpath.clone(),
                game_dir: inst.game_dir.clone(),
                assets_dir: inst.assets_dir.clone(),
                natives_dir: inst.natives_dir.clone(),
                version_id: inst.version_id.clone(),
                username: auth_name,
                uuid: auth_uuid,
                access_token: auth_token,
                version_type: "release".into(),
                asset_index: inst.asset_index.clone(),
                min_memory_mb: 2048,
                max_memory_mb: 4096,
                authlib_injector_jar: authlib_jar,
                required_java: inst.required_java,
            };
            let cmd = quarry_core::launch::build_command(&cfg);

            let status = std::process::Command::new(&cmd.program)
                .args(&cmd.args)
                .current_dir(&inst.game_dir)
                .status()
                .context("Не удалось запустить процесс игры")?;
            println!("Игра завершилась с кодом {:?}", status.code());
        }
        Commands::Profile { action } => match action {
            ProfileAction::Create { name, version, loader, root } => {
                let root = data_root(root)?;
                let store = ProfileStore::new(root);
                let p = store.create(&name, &version, loader.as_deref()).map_err(to_anyhow)?;
                match &p.loader {
                    Some(l) => println!("Создан профиль '{}' (ID: {}) с загрузчиком {}", p.name, p.id, l),
                    None => println!("Создан профиль '{}' (ID: {})", p.name, p.id),
                }
            }
            ProfileAction::List { root } => {
                let root = data_root(root)?;
                let store = ProfileStore::new(root);
                let list = store.list().map_err(to_anyhow)?;
                if list.is_empty() {
                    println!("Профилей нет.");
                } else {
                    println!("{:<20} {:<25} {}", "ID", "Имя", "Версия");
                    for p in list {
                        println!("{:<20} {:<25} {}", p.id, p.name, p.mc_version);
                    }
                }
            }
            ProfileAction::Delete { id, root } => {
                let root = data_root(root)?;
                let store = ProfileStore::new(root);
                store.delete(&id).map_err(to_anyhow)?;
                println!("Профиль '{}' удалён", id);
            }
        },
    }
    Ok(())
}
