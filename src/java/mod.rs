use std::path::PathBuf;
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

    #[tokio::test]
    #[ignore]
    async fn test_find_java_system() {
        let result = find_java().await.unwrap();
        println!("Найдена Java: {:?}", result);
    }
}
