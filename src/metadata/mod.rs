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
