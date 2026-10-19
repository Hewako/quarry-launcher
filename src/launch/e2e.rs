use std::path::{Path, PathBuf};
use tokio::process::Command;

type BoxErr = Box<dyn std::error::Error + Send + Sync>;

async fn prepare_version(version_id: &str, root: &Path) -> Result<PathBuf, BoxErr> {
    let manifest = crate::metadata::fetch_manifest().await?;
    let entry = manifest
        .versions
        .iter()
        .find(|v| v.id == version_id)
        .ok_or_else(|| format!("Версия {} не найдена", version_id))?;
    let details = crate::metadata::fetch_version_details(&entry.url).await?;

    let lib_dir = root.join("libraries");
    let version_dir = root.join("versions").join(version_id);
    let assets_dir = root.join("assets");
    let natives_dir = root.join("natives");
    let game_dir = root.join("game");
    tokio::fs::create_dir_all(&lib_dir).await?;
    tokio::fs::create_dir_all(&version_dir).await?;
    tokio::fs::create_dir_all(&assets_dir).await?;
    tokio::fs::create_dir_all(&natives_dir).await?;
    tokio::fs::create_dir_all(&game_dir).await?;

    let client_path = version_dir.join("client.jar");
    crate::downloader::download_file(
        &details.downloads.client.url,
        &client_path,
        &details.downloads.client.sha1,
    )
    .await?;
    println!("client.jar готов");

    let mut libs_to_download: Vec<(String, PathBuf, String)> = Vec::new();
    let mut natives_to_extract: Vec<PathBuf> = Vec::new();
    for lib in &details.libraries {
        if !lib.is_allowed() {
            continue;
        }
        if let Some(dl) = &lib.downloads {
            if let Some(art) = &dl.artifact {
                libs_to_download.push((art.url.clone(), lib_dir.join(&art.path), art.sha1.clone()));
            }
            if let Some(classifier) = lib.native_classifier() {
                if let Some(nat) = dl.classifiers.get(classifier) {
                    let nat_path = lib_dir.join(&nat.path);
                    libs_to_download.push((nat.url.clone(), nat_path.clone(), nat.sha1.clone()));
                    natives_to_extract.push(nat_path);
                }
            }
        }
    }
    println!("Скачиваем {} файлов библиотек и нативов...", libs_to_download.len());
    crate::downloader::download_many(libs_to_download, 8).await?;
    println!("Библиотеки готовы");

    for nat_jar in &natives_to_extract {
        extract_natives(nat_jar, &natives_dir)?;
    }
    println!("Нативы распакованы");

    let asset_index_path = assets_dir
        .join("indexes")
        .join(format!("{}.json", details.asset_index.id));
    if let Some(parent) = asset_index_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    crate::downloader::download_file(
        &details.asset_index.url,
        &asset_index_path,
        &details.asset_index.sha1,
    )
    .await?;
    println!("asset index готов");

    // Скачиваем сами объекты ассетов
    let index_json = tokio::fs::read_to_string(&asset_index_path).await?;
    let index_data: crate::metadata::AssetIndexData = serde_json::from_str(&index_json)?;
    let mut asset_downloads: Vec<(String, PathBuf, String)> = Vec::new();
    for (_name, obj) in &index_data.objects {
        let hash = &obj.hash;
        let sub = &hash[0..2];
        let url = format!("https://resources.download.minecraft.net/{}/{}", sub, hash);
        let dest = assets_dir.join("objects").join(sub).join(hash);
        asset_downloads.push((url, dest, hash.clone()));
    }
    println!("Скачиваем {} ассетов...", asset_downloads.len());
    crate::downloader::download_many(asset_downloads, 16).await?;
    println!("Ассеты готовы");

    let mut classpath: Vec<PathBuf> = vec![client_path.clone()];
    for lib in &details.libraries {
        if !lib.is_allowed() {
            continue;
        }
        if let Some(dl) = &lib.downloads {
            if let Some(art) = &dl.artifact {
                classpath.push(lib_dir.join(&art.path));
            }
        }
    }
    let cp = std::env::join_paths(&classpath)?;

    let mut args: Vec<String> = vec![
        "-Xms1G".into(),
        "-Xmx2G".into(),
        "--enable-native-access=ALL-UNNAMED".into(),
        format!("-Djava.library.path={}", natives_dir.display()),
        "-cp".into(),
        cp.to_string_lossy().to_string(),
        details.main_class.clone(),
        "--username".into(),
        "QuarryTest".into(),
        "--version".into(),
        details.id.clone(),
        "--gameDir".into(),
        game_dir.to_string_lossy().to_string(),
        "--assetsDir".into(),
        assets_dir.to_string_lossy().to_string(),
        "--assetIndex".into(),
        details.asset_index.id.clone(),
        "--uuid".into(),
        "00000000000000000000000000000001".into(),
        "--accessToken".into(),
        "0".into(),
        "--userType".into(),
        "legacy".into(),
        "--versionType".into(),
        "release".into(),
    ];
    #[cfg(target_os = "macos")]
    args.insert(2, "-XstartOnFirstThread".into());

    let java_root = root.join("java");
    let java_bin = match crate::java::find_java().await {
        Ok(Some((path, ver))) if ver >= 25 && path != PathBuf::from("java") => path,
        _ => crate::java::download_jre(25, &java_root).await?,
    };
    println!("Java: {:?}", java_bin);

    println!("Запускаем игру...");
    let mut child = Command::new(&java_bin)
        .args(&args)
        .current_dir(&game_dir)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;

    tokio::time::sleep(std::time::Duration::from_secs(30)).await;

    match child.try_wait()? {
        Some(status) => {
            let mut stderr = String::new();
            if let Some(mut s) = child.stderr.take() {
                use tokio::io::AsyncReadExt;
                let _ = s.read_to_string(&mut stderr).await;
            }
            let log_path = game_dir.join("logs").join("latest.log");
            let log_content = tokio::fs::read_to_string(&log_path).await.unwrap_or_default();
            let errors: Vec<&str> = log_content
                .lines()
                .filter(|l| {
                    l.contains("ERROR")
                        || l.contains("FATAL")
                        || l.contains("Exception")
                        || l.contains("Caused by")
                })
                .take(20)
                .collect();
            Err(format!(
                "Игра завершилась с кодом {:?}.\n\nПоследние ошибки из latest.log:\n{}\n\nstderr:\n{}",
                status,
                errors.join("\n"),
                stderr
            )
            .into())
        }
        None => {
            let _ = child.kill().await;
            println!("Игра прожила 30 секунд и не упала — запуск успешен");
            Ok(game_dir)
        }
    }
}

fn extract_natives(jar: &Path, dest: &Path) -> Result<(), BoxErr> {
    use std::io::Read;
    let file = std::fs::File::open(jar)?;
    let mut archive = zip::ZipArchive::new(file)?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let name = entry.name()?.to_string();
        if name.ends_with(".dylib") || name.ends_with(".so") || name.ends_with(".dll") {
            let file_name = Path::new(&name).file_name().unwrap();
            let out = dest.join(file_name);
            let mut buf = Vec::new();
            entry.read_to_end(&mut buf)?;
            std::fs::write(out, buf)?;
        }
    }
    Ok(())
}

#[tokio::test]
#[ignore]
async fn e2e_launch() {
    let root = std::env::temp_dir().join("quarry_e2e");
    let _ = tokio::fs::remove_dir_all(&root).await;
    if let Err(e) = prepare_version("26.3", &root).await {
        panic!("{}", e);
    }
}
