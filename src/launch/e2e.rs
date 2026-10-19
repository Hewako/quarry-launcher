#[tokio::test]
#[ignore]
async fn e2e_launch() {
    let root = std::env::temp_dir().join("quarry_e2e");
    let _ = tokio::fs::remove_dir_all(&root).await;
    let game_dir = root.join("game");

    let inst = crate::instance::prepare("26.3", &root, &game_dir, None).await.unwrap();
    let cfg = crate::launch::LaunchConfig {
        java_path: inst.java_path.clone(),
        main_class: inst.main_class.clone(),
        classpath: inst.classpath.clone(),
        game_dir: inst.game_dir.clone(),
        assets_dir: inst.assets_dir.clone(),
        natives_dir: inst.natives_dir.clone(),
        version_id: inst.version_id.clone(),
        username: "QuarryTest".into(),
        uuid: "00000000000000000000000000000001".into(),
        access_token: "0".into(),
        version_type: "release".into(),
        asset_index: inst.asset_index.clone(),
        min_memory_mb: 1024,
        max_memory_mb: 2048,
    };
    let cmd = crate::launch::build_command(&cfg);

    let mut child = tokio::process::Command::new(&cmd.program)
        .args(&cmd.args)
        .current_dir(&inst.game_dir)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();

    tokio::time::sleep(std::time::Duration::from_secs(30)).await;
    match child.try_wait().unwrap() {
        Some(status) => {
            let log = tokio::fs::read_to_string(inst.game_dir.join("logs").join("latest.log"))
                .await
                .unwrap_or_default();
            let errors: Vec<&str> = log
                .lines()
                .filter(|l| l.contains("ERROR") || l.contains("FATAL") || l.contains("Caused by"))
                .take(20)
                .collect();
            panic!("Игра упала с {:?}:\n{}", status, errors.join("\n"));
        }
        None => {
            let _ = child.kill().await;
            println!("Игра прожила 30 секунд — запуск успешен");
        }
    }
}
