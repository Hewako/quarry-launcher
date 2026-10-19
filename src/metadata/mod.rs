use serde::{Deserialize, Serialize};
use crate::USER_AGENT;

#[derive(Debug, Serialize, Deserialize)]
pub struct VersionManifest {
    pub latest: LatestVersions,
    pub versions: Vec<VersionEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LatestVersions {
    pub release: String,
    pub snapshot: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct VersionEntry {
    pub id: String,
    #[serde(rename = "type")]
    pub version_type: String,
    pub url: String,
    #[serde(rename = "time")]
    pub time: String,
    #[serde(rename = "releaseTime")]
    pub release_time: String,
}

// Получаем список всех версий с серверов Mojang
pub async fn fetch_manifest() -> Result<VersionManifest, Box<dyn std::error::Error>> {
    let client = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .build()?;

    let url = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
    let resp = client.get(url).send().await?;

    if !resp.status().is_success() {
        return Err(format!("Ошибка HTTP: {}", resp.status()).into());
    }

    let manifest = resp.json::<VersionManifest>().await?;
    Ok(manifest)
}

// Структуры для разбора конкретной версии
#[derive(Debug, Serialize, Deserialize)]
pub struct VersionDetails {
    pub id: String,
    #[serde(rename = "mainClass")]
    pub main_class: String,
    pub downloads: Downloads,
    #[serde(rename = "assetIndex")]
    pub asset_index: AssetIndex,
    #[serde(rename = "javaVersion")]
    pub java_version: JavaVersion,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Downloads {
    pub client: DownloadInfo,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DownloadInfo {
    pub url: String,
    pub size: u64,
    pub sha1: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AssetIndex {
    pub id: String,
    pub url: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct JavaVersion {
    pub component: String,
    #[serde(rename = "majorVersion")]
    pub major_version: u32,
}

// Скачиваем JSON конкретной версии, чтобы узнать, откуда брать файлы игры и какую Java просить
pub async fn fetch_version_details(url: &str) -> Result<VersionDetails, Box<dyn std::error::Error>> {
    let client = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .build()?;

    let resp = client.get(url).send().await?;

    if !resp.status().is_success() {
        return Err(format!("Ошибка HTTP: {}", resp.status()).into());
    }

    let details = resp.json::<VersionDetails>().await?;
    Ok(details)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_fetch_manifest() {
        let manifest = fetch_manifest().await.expect("Не удалось получить список версий");
        assert!(!manifest.versions.is_empty(), "Список версий не должен быть пустым");
        println!("Последняя версия: {}", manifest.latest.release);
    }

    #[tokio::test]
    async fn test_fetch_version_details() {
        let manifest = fetch_manifest().await.expect("Не удалось получить список версий");
        
        // Берём последнюю версию и скачиваем её JSON
        let latest_id = &manifest.latest.release;
        let version_entry = manifest.versions.iter()
            .find(|v| &v.id == latest_id)
            .expect("Версия не найдена в списке");
            
        let details = fetch_version_details(&version_entry.url).await
            .expect("Не удалось получить детали версии");
            
        assert!(!details.main_class.is_empty(), "Главный класс не может быть пустым");
        assert!(details.java_version.major_version > 0, "Версия Java должна быть больше 0");
        
        println!("Версия: {}", details.id);
        println!("Главный класс: {}", details.main_class);
        println!("Требуемая Java: {}", details.java_version.major_version);
        println!("Ссылка на клиент: {}", details.downloads.client.url);
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Library {
    pub name: String,
    #[serde(default)]
    pub rules: Vec<Rule>,
    pub downloads: Option<LibraryDownloads>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LibraryDownloads {
    pub artifact: Option<Artifact>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Artifact {
    pub path: String,
    pub sha1: String,
    pub size: u64,
    pub url: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Rule {
    pub action: String,
    #[serde(default)]
    pub os: Option<OsRule>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OsRule {
    pub name: Option<String>,
    pub arch: Option<String>,
}

// Проверяем, подходит ли библиотека под текущую ОС и архитектуру.
// Логика Mojang: по умолчанию include=false. Идём по правилам по порядку.
// allow + совпало = include=true. disallow + совпало = include=false.
// Нет правил вовсе — include=true.
pub fn is_library_allowed(rules: &[Rule]) -> bool {
    if rules.is_empty() {
        return true;
    }
    let mut allowed = false;
    for rule in rules {
        let matched = match &rule.os {
            None => true,
            Some(os) => {
                let name_match = match &os.name {
                    None => true,
                    Some(n) => current_os() == n,
                };
                let arch_match = match &os.arch {
                    None => true,
                    Some(a) => current_arch() == a,
                };
                name_match && arch_match
            }
        };
        if matched {
            allowed = rule.action == "allow";
        }
    }
    allowed
}

pub fn current_os() -> &'static str {
    if cfg!(target_os = "macos") {
        "osx"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else {
        "linux"
    }
}

pub fn current_arch() -> &'static str {
    if cfg!(target_arch = "aarch64") {
        "arm64"
    } else if cfg!(target_arch = "x86_64") {
        "x86_64"
    } else {
        "x86"
    }
}

// Возвращаем библиотеки, которые нужны на текущей платформе и имеют artifact
pub fn usable_libraries(libs: &[Library]) -> Vec<&Artifact> {
    libs.iter()
        .filter(|lib| is_library_allowed(&lib.rules))
        .filter_map(|lib| lib.downloads.as_ref())
        .filter_map(|d| d.artifact.as_ref())
        .collect()
}

#[cfg(test)]
mod library_tests {
    use super::*;

    #[test]
    fn test_library_no_rules() {
        let lib = Library {
            name: "test:lib:1.0".to_string(),
            rules: vec![],
            downloads: None,
        };
        assert!(is_library_allowed(&lib.rules));
    }

    #[test]
    fn test_library_allow_only() {
        let rules = vec![Rule { action: "allow".to_string(), os: None }];
        assert!(is_library_allowed(&rules));
    }

    #[test]
    fn test_library_disallow_current_os() {
        let rules = vec![
            Rule { action: "allow".to_string(), os: None },
            Rule {
                action: "disallow".to_string(),
                os: Some(OsRule {
                    name: Some(current_os().to_string()),
                    arch: None,
                }),
            },
        ];
        assert!(!is_library_allowed(&rules));
    }

    #[test]
    fn test_library_disallow_other_os() {
        let other = if current_os() == "osx" { "windows" } else { "osx" };
        let rules = vec![
            Rule { action: "allow".to_string(), os: None },
            Rule {
                action: "disallow".to_string(),
                os: Some(OsRule {
                    name: Some(other.to_string()),
                    arch: None,
                }),
            },
        ];
        assert!(is_library_allowed(&rules));
    }
}
