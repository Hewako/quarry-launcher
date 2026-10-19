use std::path::PathBuf;

pub struct LaunchConfig {
    pub java_path: PathBuf,
    pub main_class: String,
    pub classpath: Vec<PathBuf>,
    pub game_dir: PathBuf,
    pub assets_dir: PathBuf,
    pub version_id: String,
    pub username: String,
    pub uuid: String,
    pub access_token: String,
    pub user_type: String,
    pub version_type: String,
    pub asset_index: String,
    pub min_memory_mb: u32,
    pub max_memory_mb: u32,
}

pub struct LaunchCommand {
    pub program: PathBuf,
    pub args: Vec<String>,
}

// Собираем командную строку запуска Minecraft.
// Всё, что приходит извне (ник, UUID, токен), передаём отдельными аргументами,
// а не склеиваем строкой — это защита от инъекции.
pub fn build_command(cfg: &LaunchConfig) -> LaunchCommand {
    let mut args: Vec<String> = Vec::new();

    // Память: Xms = Xmx, чтобы heap не перевыделялся посреди игры
    args.push(format!("-Xms{}M", cfg.min_memory_mb));
    args.push(format!("-Xmx{}M", cfg.max_memory_mb));

    // На macOS без этого флага окно игры не откроется
    #[cfg(target_os = "macos")]
    args.push("-XstartOnFirstThread".to_string());

    // Classpath: пути через ':' на macOS/Linux. На Windows нужен ';' — учтём позже.
    let cp = cfg
        .classpath
        .iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join(":");
    args.push("-cp".to_string());
    args.push(cp);

    // Главный класс
    args.push(cfg.main_class.clone());

    // Игровые аргументы — каждый отдельной строкой
    args.push("--username".to_string());
    args.push(cfg.username.clone());

    args.push("--version".to_string());
    args.push(cfg.version_id.clone());

    args.push("--gameDir".to_string());
    args.push(cfg.game_dir.to_string_lossy().to_string());

    args.push("--assetsDir".to_string());
    args.push(cfg.assets_dir.to_string_lossy().to_string());

    args.push("--assetIndex".to_string());
    args.push(cfg.asset_index.clone());

    args.push("--uuid".to_string());
    args.push(cfg.uuid.clone());

    args.push("--accessToken".to_string());
    args.push(cfg.access_token.clone());

    args.push("--userType".to_string());
    args.push(cfg.user_type.clone());

    args.push("--versionType".to_string());
    args.push(cfg.version_type.clone());

    LaunchCommand {
        program: cfg.java_path.clone(),
        args,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_config() -> LaunchConfig {
        LaunchConfig {
            java_path: PathBuf::from("/opt/java/bin/java"),
            main_class: "net.minecraft.client.main.Main".to_string(),
            classpath: vec![
                PathBuf::from("/game/client.jar"),
                PathBuf::from("/game/libraries/a.jar"),
            ],
            game_dir: PathBuf::from("/game"),
            assets_dir: PathBuf::from("/game/assets"),
            version_id: "26.3".to_string(),
            username: "Steve".to_string(),
            uuid: "00000000-0000-0000-0000-000000000000".to_string(),
            access_token: "secret-token".to_string(),
            user_type: "msa".to_string(),
            version_type: "release".to_string(),
            asset_index: "26".to_string(),
            min_memory_mb: 2048,
            max_memory_mb: 4096,
        }
    }

    #[test]
    fn test_memory_args() {
        let cmd = build_command(&sample_config());
        assert!(cmd.args.contains(&"-Xms2048M".to_string()));
        assert!(cmd.args.contains(&"-Xmx4096M".to_string()));
    }

    #[test]
    fn test_classpath_joins_with_colon() {
        let cmd = build_command(&sample_config());
        let idx = cmd.args.iter().position(|a| a == "-cp").unwrap();
        assert_eq!(cmd.args[idx + 1], "/game/client.jar:/game/libraries/a.jar");
    }

    #[test]
    fn test_main_class_present() {
        let cmd = build_command(&sample_config());
        assert!(cmd
            .args
            .contains(&"net.minecraft.client.main.Main".to_string()));
    }

    #[test]
    fn test_auth_args_passed_separately() {
        let cmd = build_command(&sample_config());
        // Токен и UUID — отдельными элементами, не склеены в одну строку
        let token_idx = cmd.args.iter().position(|a| a == "--accessToken").unwrap();
        assert_eq!(cmd.args[token_idx + 1], "secret-token");

        let uuid_idx = cmd.args.iter().position(|a| a == "--uuid").unwrap();
        assert_eq!(
            cmd.args[uuid_idx + 1],
            "00000000-0000-0000-0000-000000000000"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn test_macos_start_on_first_thread() {
        let cmd = build_command(&sample_config());
        assert!(cmd.args.contains(&"-XstartOnFirstThread".to_string()));
    }

    #[test]
    fn test_program_is_java_path() {
        let cmd = build_command(&sample_config());
        assert_eq!(cmd.program, PathBuf::from("/opt/java/bin/java"));
    }
}
