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

pub async fn fetch_manifest() -> Result<VersionManifest, Box<dyn std::error::Error>> {
    let client = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .build()?;

    let url = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
    let resp = client.get(url).send().await?;

    if !resp.status().is_success() {
        return Err(format!("HTTP error: {}", resp.status()).into());
    }

    let manifest = resp.json::<VersionManifest>().await?;
    Ok(manifest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_fetch_manifest() {
        let manifest = fetch_manifest().await.expect("Failed to fetch manifest");
        assert!(!manifest.versions.is_empty(), "Manifest should not be empty");
        println!("Latest release: {}", manifest.latest.release);
        
        let has_recent_version = manifest.versions.iter().any(|v| v.id == "26.1");
        assert!(has_recent_version, "Version 26.1 should be present in the manifest");
    }
}
