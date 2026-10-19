use std::path::{Path, PathBuf};
use crate::downloader;
use crate::metadata;

type BoxErr = Box<dyn std::error::Error + Send + Sync>;

pub struct PreparedInstance {
    pub version_id: String,
    pub main_class: String,
    pub java_path: PathBuf,
    pub classpath: Vec<PathBuf>,
    pub game_dir: PathBuf,
    pub assets_dir: PathBuf,
    pub natives_dir: PathBuf,
    pub asset_index: String,
    pub required_java: u32,
}

pub async fn prepare(version_id: &str, root: &Path) -> Result<PreparedInstance, BoxErr> {
    let manifest = metadata::fetch_manifest().await?;
    let entry = manifest
        .versions
        .iter()
        .find(|v| v.id == version_id)
        .ok_or_else(|| format!("Версия {} не найдена", version_id))?;
    let details = metadata::fetch_version_details(&entry.url).await?;

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
    downloader::download_file(
        &details.downloads.client.url,
        &client_path,
        &details.downloads.client.sha1,
    )
    .await?;
    println!("client.jar готов");

    let mut libs: Vec<(String, PathBuf, String)> = Vec::new();
    let mut natives_to_extract: Vec<PathBuf> = Vec::new();
    for lib in &details.libraries {
        if !lib.is_allowed() {
            continue;
        }
        if let Some(dl) = &lib.downloads {
            if let Some(art) = &dl.artifact {
                libs.push((art.url.clone(), lib_dir.join(&art.path), art.sha1.clone()));
            }
            if let Some(classifier) = lib.native_classifier() {
                if let Some(nat) = dl.classifiers.get(classifier) {
                    let nat_path = lib_dir.join(&nat.path);
                    libs.push((nat.url.clone(), nat_path.clone(), nat.sha1.clone()));
                    natives_to_extract.push(nat_path);
                }
            }
        }
    }
    println!("Скачиваем {} файлов библиотек и нативов...", libs.len());
    downloader::download_many(libs, 8).await?;
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
    downloader::download_file(
        &details.asset_index.url,
        &asset_index_path,
        &details.asset_index.sha1,
    )
    .await?;
    println!("asset index готов");

    let index_json = tokio::fs::read_to_string(&asset_index_path).await?;
    let index_data: metadata::AssetIndexData = serde_json::from_str(&index_json)?;
    let mut asset_downloads: Vec<(String, PathBuf, String)> = Vec::new();
    for obj in index_data.objects.values() {
        let hash = &obj.hash;
        let sub = &hash[0..2];
        let url = format!("https://resources.download.minecraft.net/{}/{}", sub, hash);
        let dest = assets_dir.join("objects").join(sub).join(hash);
        asset_downloads.push((url, dest, hash.clone()));
    }
    println!("Скачиваем {} ассетов...", asset_downloads.len());
    downloader::download_many(asset_downloads, 16).await?;
    println!("Ассеты готовы");

    let mut classpath: Vec<PathBuf> = vec![client_path];
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

    let java_root = root.join("java");
    let java_path = match crate::java::find_java().await {
        Ok(Some((path, ver))) if ver >= details.java_version.major_version && path != PathBuf::from("java") => path,
        _ => crate::java::download_jre(details.java_version.major_version, &java_root).await?,
    };

    Ok(PreparedInstance {
        version_id: details.id.clone(),
        main_class: details.main_class.clone(),
        java_path,
        classpath,
        game_dir,
        assets_dir,
        natives_dir,
        asset_index: details.asset_index.id.clone(),
        required_java: details.java_version.major_version,
    })
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
