use flate2::read::GzDecoder;
use std::path::{Path, PathBuf};
use tar::Archive;
use tokio::process::Command;

// Ищем java в PATH и возвращаем путь и мажорную версию
pub async fn find_java() -> Result<Option<(PathBuf, u32)>, Box<dyn std::error::Error>> {
    let output = Command::new("java").arg("-version").output().await;
    match output {
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            Ok(parse_java_version(&stderr).map(|v| (PathBuf::from("java"), v)))
        }
        Err(_) => Ok(None),
    }
}

// Разбираем вывод `java -version`. Примеры:
// openjdk version "21.0.8" 2025-07-15  -> 21
// java version "1.8.0_381"             -> 8
pub fn parse_java_version(output: &str) -> Option<u32> {
    for line in output.lines() {
        if line.contains("version") {
            if let Some(start) = line.find('"') {
                if let Some(end) = line[start + 1..].find('"') {
                    let ver = &line[start + 1..start + 1 + end];
                    if ver.starts_with("1.") {
                        let parts: Vec<&str> = ver.split('.').collect();
                        if parts.len() >= 2 {
                            return parts[1].parse().ok();
                        }
                    } else {
                        let major = ver.split('.').next()?;
                        return major.parse().ok();
                    }
                }
            }
        }
    }
    None
}

// Определяем платформу для URL Adoptium.
pub fn detect_platform() -> Result<(&'static str, &'static str), Box<dyn std::error::Error>> {
    let os = if cfg!(target_os = "macos") {
        "mac"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        return Err("неподдерживаемая ОС".into());
    };

    let arch = if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else if cfg!(target_arch = "x86_64") {
        "x64"
    } else {
        return Err("неподдерживаемая архитектура".into());
    };

    Ok((os, arch))
}

// Скачиваем JDK нужной версии с Adoptium и распаковываем. Возвращаем путь к java.
pub async fn download_jre(
    feature_version: u32,
    dest_dir: &Path,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let (os, arch) = detect_platform()?;
    let url = format!(
        "https://api.adoptium.net/v3/binary/latest/{}/ga/{}/{}/jdk/hotspot/normal/eclipse",
        feature_version, os, arch
    );

    tokio::fs::create_dir_all(dest_dir).await?;
    let archive_path = dest_dir.join("jdk.tar.gz");

    crate::downloader::download_raw(&url, &archive_path).await?;

    let archive_clone = archive_path.clone();
    let dest_clone = dest_dir.to_path_buf();
    tokio::task::spawn_blocking(move || -> std::io::Result<()> {
        let file = std::fs::File::open(&archive_clone)?;
        let mut archive = Archive::new(GzDecoder::new(file));
        archive.unpack(&dest_clone)?;
        Ok(())
    })
    .await??;

    let _ = tokio::fs::remove_file(&archive_path).await;

    find_java_binary(dest_dir)
}

// Ищем исполняемый файл java в распакованном архиве.
pub fn find_java_binary(root: &Path) -> Result<PathBuf, Box<dyn std::error::Error>> {
    for entry in walkdir::WalkDir::new(root) {
        let entry = entry?;
        let path = entry.path();
        if path.is_file()
            && path.file_name().map_or(false, |n| n == "java" || n == "java.exe")
        {
            return Ok(path.to_path_buf());
        }
    }
    Err("исполняемый файл java не найден в распакованном архиве".into())
}

// Проверяем, что скачанный java действительно работает и той версии, что нужна.
pub async fn verify_java_binary(
    java_path: &Path,
    expected_major: u32,
) -> Result<bool, Box<dyn std::error::Error>> {
    let output = Command::new(java_path).arg("-version").output().await?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    Ok(parse_java_version(&stderr) == Some(expected_major))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_java_version_modern() {
        let out = "openjdk version \"21.0.8\" 2025-07-15";
        assert_eq!(parse_java_version(out), Some(21));
    }

    #[test]
    fn test_parse_java_version_legacy() {
        let out = "java version \"1.8.0_381\"";
        assert_eq!(parse_java_version(out), Some(8));
    }

    #[test]
    fn test_parse_java_version_25() {
        let out = "openjdk version \"25.0.1\" 2026-01-15";
        assert_eq!(parse_java_version(out), Some(25));
    }

    #[test]
    fn test_parse_java_version_none() {
        assert_eq!(parse_java_version("no java here"), None);
    }

    #[test]
    fn test_detect_platform_returns_something() {
        let result = detect_platform();
        assert!(result.is_ok(), "Платформа должна определяться");
        let (os, arch) = result.unwrap();
        assert!(!os.is_empty());
        assert!(!arch.is_empty());
    }

    #[tokio::test]
    #[ignore]
    async fn test_find_java_system() {
        let result = find_java().await.unwrap();
        println!("Найдена Java: {:?}", result);
    }

    #[tokio::test]
    #[ignore]
    async fn test_download_java_25() {
        let dir = std::env::temp_dir().join("quarry_test_java_25");
        let _ = tokio::fs::remove_dir_all(&dir).await;

        let java_path = download_jre(25, &dir)
            .await
            .expect("Не удалось скачать Java 25");
        println!("Java скачана: {:?}", java_path);

        let ok = verify_java_binary(&java_path, 25)
            .await
            .expect("Не удалось проверить Java");
        assert!(ok, "Скачанная Java должна быть версии 25");

        tokio::fs::remove_dir_all(&dir).await.unwrap();
    }
}
