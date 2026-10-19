pub mod e2e;

use std::path::PathBuf;

pub struct LaunchConfig {
    pub java_path: PathBuf,
    pub main_class: String,
    pub classpath: Vec<PathBuf>,
    pub game_dir: PathBuf,
    pub assets_dir: PathBuf,
    pub natives_dir: PathBuf,
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

pub fn build_command(cfg: &LaunchConfig) -> LaunchCommand {
    let mut args: Vec<String> = Vec::new();
    args.push(format!("-Xms{}M", cfg.min_memory_mb));
    args.push(format!("-Xmx{}M", cfg.max_memory_mb));
    args.push("--enable-native-access=ALL-UNNAMED".into());
    args.push(format!("-Djava.library.path={}", cfg.natives_dir.display()));

    #[cfg(target_os = "macos")]
    args.push("-XstartOnFirstThread".to_string());

    let cp = cfg
        .classpath
        .iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join(":");
    args.push("-cp".into());
    args.push(cp);
    args.push(cfg.main_class.clone());

    args.push("--username".into());
    args.push(cfg.username.clone());
    args.push("--version".into());
    args.push(cfg.version_id.clone());
    args.push("--gameDir".into());
    args.push(cfg.game_dir.to_string_lossy().to_string());
    args.push("--assetsDir".into());
    args.push(cfg.assets_dir.to_string_lossy().to_string());
    args.push("--assetIndex".into());
    args.push(cfg.asset_index.clone());
    args.push("--uuid".into());
    args.push(cfg.uuid.clone());
    args.push("--accessToken".into());
    args.push(cfg.access_token.clone());
    args.push("--userType".into());
    args.push(cfg.user_type.clone());
    args.push("--versionType".into());
    args.push(cfg.version_type.clone());

    LaunchCommand { program: cfg.java_path.clone(), args }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> LaunchConfig {
        LaunchConfig {
            java_path: PathBuf::from("/opt/java/bin/java"),
            main_class: "net.minecraft.client.main.Main".into(),
            classpath: vec![PathBuf::from("/game/client.jar"), PathBuf::from("/game/lib/a.jar")],
            game_dir: PathBuf::from("/game"),
            assets_dir: PathBuf::from("/game/assets"),
            natives_dir: PathBuf::from("/game/natives"),
            version_id: "26.3".into(),
            username: "Steve".into(),
            uuid: "00000000-0000-0000-0000-000000000000".into(),
            access_token: "secret-token".into(),
            user_type: "msa".into(),
            version_type: "release".into(),
            asset_index: "26".into(),
            min_memory_mb: 2048,
            max_memory_mb: 4096,
        }
    }

    #[test]
    fn memory_args() {
        let c = build_command(&sample());
        assert!(c.args.contains(&"-Xms2048M".to_string()));
        assert!(c.args.contains(&"-Xmx4096M".to_string()));
    }

    #[test]
    fn classpath_joined() {
        let c = build_command(&sample());
        let i = c.args.iter().position(|a| a == "-cp").unwrap();
        assert_eq!(c.args[i + 1], "/game/client.jar:/game/lib/a.jar");
    }

    #[test]
    fn auth_args_separate() {
        let c = build_command(&sample());
        let i = c.args.iter().position(|a| a == "--accessToken").unwrap();
        assert_eq!(c.args[i + 1], "secret-token");
    }

    #[test]
    fn natives_path_included() {
        let c = build_command(&sample());
        assert!(c.args.iter().any(|a| a.starts_with("-Djava.library.path=")));
    }
}
