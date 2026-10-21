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
    pub version_type: String,
    pub asset_index: String,
    pub min_memory_mb: u32,
    pub max_memory_mb: u32,
    pub authlib_injector_jar: Option<PathBuf>,
    pub required_java: u32,
}

pub struct LaunchCommand {
    pub program: PathBuf,
    pub args: Vec<String>,
}

pub fn build_command(cfg: &LaunchConfig) -> LaunchCommand {
    let mut args: Vec<String> = Vec::new();
    
    // Аргументы памяти
    args.push(format!("-Xms{}M", cfg.min_memory_mb));
    args.push(format!("-Xmx{}M", cfg.max_memory_mb));
    
    // Если есть authlib-injector, добавляем javaagent
    if let Some(jar) = &cfg.authlib_injector_jar {
        args.push(format!("-javaagent:{}={}", jar.display(), "ely.by"));
    }

    if cfg.required_java >= 17 {
        if cfg.required_java >= 17 {
        if cfg.required_java >= 17 {
        args.push("--enable-native-access=ALL-UNNAMED".into());
    }
    }
    }
    args.push(format!("-Djava.library.path={}", cfg.natives_dir.display()));

    #[cfg(target_os = "macos")]
    args.push("-XstartOnFirstThread".to_string());

    let separator = if cfg!(target_os = "windows") { ";" } else { ":" };
    let cp = cfg.classpath.iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join(separator);
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
    args.push("--versionType".into());
    args.push(cfg.version_type.clone());

    LaunchCommand { program: cfg.java_path.clone(), args }
}
