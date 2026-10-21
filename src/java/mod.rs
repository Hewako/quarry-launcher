use flate2::read::GzDecoder;
use std::path::{Path, PathBuf};
use tar::Archive;
use tokio::process::Command;

type BoxErr = Box<dyn std::error::Error + Send + Sync>;

pub async fn find_java() -> Result<Option<(PathBuf, u32)>, BoxErr> {
    let output = Command::new("java").arg("-version").output().await;
    match output {
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            Ok(parse_java_version(&stderr).map(|v| (PathBuf::from("java"), v)))
        }
        Err(_) => Ok(None),
    }
}

pub fn parse_java_version(output: &str) -> Option<u32> {
    for line in output.lines() {
        if line.contains("version") {
            if let Some(start) = line.find('"') {
                if let Some(end) = line[start + 1..].find('"') {
                    let ver = &line[start + 1..start + 1 + end];
                    if ver.starts_with("1.") {
                        return ver.split('.').nth(1)?.parse().ok();
                    }
                    return ver.split('.').next()?.parse().ok();
                }
            }
        }
    }
    None
}

pub fn detect_platform() -> Result<(&'static str, &'static str), BoxErr> {
    let os = if cfg!(target_os = "macos") { "mac" }
        else if cfg!(target_os = "windows") { "windows" }
        else if cfg!(target_os = "linux") { "linux" }
        else { return Err("неподдерживаемая ОС".into()); };
    let arch = if cfg!(target_arch = "aarch64") { "aarch64" }
        else if cfg!(target_arch = "x86_64") { "x64" }
        else { return Err("неподдерживаемая архитектура".into()); };
    Ok((os, arch))
}

pub async fn download_jre(feature_version: u32, dest_dir: &Path) -> Result<PathBuf, BoxErr> {
    let version_dir = dest_dir.join(feature_version.to_string());

    // Если уже скачано и работает — возвращаем
    if version_dir.exists() {
        if let Ok(path) = find_java_binary(&version_dir) {
            if verify_java_binary(&path, feature_version).await.unwrap_or(false) {
                return Ok(path);
            }
        }
    }

    let (os, arch) = detect_platform()?;
    let url = format!(
        "https://api.adoptium.net/v3/binary/latest/{}/ga/{}/{}/jdk/hotspot/normal/eclipse",
        feature_version, os, arch
    );
    tokio::fs::create_dir_all(&version_dir).await?;
    let archive_name = if cfg!(target_os = "windows") { "jdk.zip" } else { "jdk.tar.gz" };
    let archive_path = version_dir.join(archive_name);

    crate::downloader::download_raw(&url, &archive_path).await?;

    if cfg!(target_os = "windows") {
        let archive_clone = archive_path.clone();
        let dest_clone = version_dir.clone();
        tokio::task::spawn_blocking(move || -> std::io::Result<()> {
            let file = std::fs::File::open(&archive_clone)?;
            let mut zip = zip::ZipArchive::new(file)?;
            zip.extract(&dest_clone)?;
            Ok(())
        }).await??;
    } else {
        let archive_clone = archive_path.clone();
        let dest_clone = version_dir.clone();
        tokio::task::spawn_blocking(move || -> std::io::Result<()> {
            let file = std::fs::File::open(&archive_clone)?;
            let mut archive = Archive::new(GzDecoder::new(file));
            archive.unpack(&dest_clone)?;
            Ok(())
        }).await??;
    }

    let _ = tokio::fs::remove_file(&archive_path).await;
    find_java_binary(&version_dir)
}

pub fn find_java_binary(root: &Path) -> Result<PathBuf, BoxErr> {
    for entry in walkdir::WalkDir::new(root) {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() && path.file_name().map_or(false, |n| n == "java" || n == "java.exe") {
            return Ok(path.to_path_buf());
        }
    }
    Err("исполняемый файл java не найден".into())
}

pub async fn verify_java_binary(java_path: &Path, expected_major: u32) -> Result<bool, BoxErr> {
    let output = Command::new(java_path).arg("-version").output().await?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    Ok(parse_java_version(&stderr) == Some(expected_major))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn t_modern() { assert_eq!(parse_java_version("openjdk version \"21.0.8\""), Some(21)); }
    #[test] fn t_legacy() { assert_eq!(parse_java_version("java version \"1.8.0_381\""), Some(8)); }
    #[test] fn t_25() { assert_eq!(parse_java_version("openjdk version \"25.0.1\""), Some(25)); }
}
