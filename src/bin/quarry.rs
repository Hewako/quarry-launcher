use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
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
    /// Управление профилями
    Profile {
        #[command(subcommand)]
        action: ProfileAction,
    },
    /// Запустить профиль
    Launch {
        /// ID профиля (например, my-survival)
        profile_id: String,
        #[arg(short, long, default_value = "Player")]
        username: String,
        #[arg(long)]
        root: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum ProfileAction {
    Create {
        name: String,
        #[arg(short, long)]
        version: String,
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

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Profile { action } => match action {
            ProfileAction::Create { name, version, root } => {
                let root = data_root(root)?;
                let store = ProfileStore::new(root);
                let p = store.create(&name, &version).map_err(to_anyhow)?;
                println!("Создан профиль '{}' (ID: {})", p.name, p.id);
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
        Commands::Launch { profile_id, username, root } => {
            let root = data_root(root)?;
            let store = ProfileStore::new(root.clone());
            let profile = store.get(&profile_id).map_err(to_anyhow)?;
            let game_dir = store.game_dir(&profile_id);
            println!("Запускаем '{}' ({})", profile.name, profile.mc_version);

            let inst = quarry_core::instance::prepare(&profile.mc_version, &root, &game_dir)
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
                username,
                uuid: "00000000000000000000000000000001".into(),
                access_token: "0".into(),
                user_type: "legacy".into(),
                version_type: "release".into(),
                asset_index: inst.asset_index.clone(),
                min_memory_mb: 2048,
                max_memory_mb: 4096,
            };
            let cmd = quarry_core::launch::build_command(&cfg);

            let status = std::process::Command::new(&cmd.program)
                .args(&cmd.args)
                .current_dir(&inst.game_dir)
                .status()
                .context("Не удалось запустить процесс игры")?;
            println!("Игра завершилась с кодом {:?}", status.code());
        }
    }
    Ok(())
}
