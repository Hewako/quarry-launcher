use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "quarry", version, about = "Minecraft Java launcher")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Запустить игру
    Launch {
        /// Версия Minecraft (например, 26.3)
        version: String,
        /// Ник игрока
        #[arg(short, long, default_value = "Player")]
        username: String,
        /// Папка данных (по умолчанию ~/.quarry)
        #[arg(long)]
        root: Option<PathBuf>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Launch { version, username, root } => {
            run(&version, &username, root).await
        }
    }
}

async fn run(version: &str, username: &str, root: Option<PathBuf>) -> Result<()> {
    let root = root
        .or_else(|| dirs::home_dir().map(|h| h.join(".quarry")))
        .context("Не удалось определить домашнюю папку")?;
    std::fs::create_dir_all(&root)?;
    println!("Папка данных: {}", root.display());

    let inst = quarry_core::instance::prepare(version, &root)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    let cfg = quarry_core::launch::LaunchConfig {
        java_path: inst.java_path.clone(),
        main_class: inst.main_class.clone(),
        classpath: inst.classpath.clone(),
        game_dir: inst.game_dir.clone(),
        assets_dir: inst.assets_dir.clone(),
        natives_dir: inst.natives_dir.clone(),
        version_id: inst.version_id.clone(),
        username: username.to_string(),
        uuid: "00000000000000000000000000000001".into(),
        access_token: "0".into(),
        user_type: "legacy".into(),
        version_type: "release".into(),
        asset_index: inst.asset_index.clone(),
        min_memory_mb: 2048,
        max_memory_mb: 4096,
    };
    let cmd = quarry_core::launch::build_command(&cfg);

    println!("Запускаем {} как {}...", inst.version_id, username);
    let status = std::process::Command::new(&cmd.program)
        .args(&cmd.args)
        .current_dir(&inst.game_dir)
        .status()
        .context("Не удалось запустить процесс игры")?;
    println!("Игра завершилась с кодом {:?}", status.code());
    Ok(())
}
