use crate::USER_AGENT;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

type BoxErr = Box<dyn std::error::Error + Send + Sync>;

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

pub async fn fetch_manifest() -> Result<VersionManifest, BoxErr> {
    let client = reqwest::Client::builder().user_agent(USER_AGENT).build()?;
    let url = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
    let resp = client.get(url).send().await?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()).into());
    }
    Ok(resp.json::<VersionManifest>().await?)
}

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
    #[serde(default)]
    pub libraries: Vec<LibraryFull>,
    #[serde(rename = "inheritsFrom", default)]
    pub inherits_from: Option<String>,
    #[serde(default)]
    pub arguments: Option<serde_json::Value>,
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
    pub sha1: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct JavaVersion {
    pub component: String,
    #[serde(rename = "majorVersion")]
    pub major_version: u32,
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

pub async fn fetch_version_details(url: &str) -> Result<VersionDetails, BoxErr> {
    let client = reqwest::Client::builder().user_agent(USER_AGENT).build()?;
    let resp = client.get(url).send().await?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()).into());
    }
    Ok(resp.json::<VersionDetails>().await?)
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

pub fn is_library_allowed(rules: &[Rule]) -> bool {
    if rules.is_empty() {
        return true;
    }
    let mut allowed = false;
    for rule in rules {
        let matched = match &rule.os {
            None => true,
            Some(os) => {
                let n = os.name.as_deref().map_or(true, |v| v == current_os());
                let a = os.arch.as_deref().map_or(true, |v| v == current_arch());
                n && a
            }
        };
        if matched {
            allowed = rule.action == "allow";
        }
    }
    allowed
}

pub fn usable_libraries(libs: &[Library]) -> Vec<&Artifact> {
    libs.iter()
        .filter(|lib| is_library_allowed(&lib.rules))
        .filter_map(|lib| lib.downloads.as_ref())
        .filter_map(|d| d.artifact.as_ref())
        .collect()
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AssetIndexData {
    pub objects: HashMap<String, AssetObject>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AssetObject {
    pub hash: String,
    pub size: u64,
}

pub async fn fetch_asset_index(url: &str) -> Result<AssetIndexData, BoxErr> {
    let client = reqwest::Client::builder().user_agent(USER_AGENT).build()?;
    let resp = client.get(url).send().await?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()).into());
    }
    Ok(resp.json::<AssetIndexData>().await?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore]
    async fn test_fetch_manifest() {
        let m = fetch_manifest().await.unwrap();
        assert!(!m.versions.is_empty());
    }

    #[tokio::test]
    #[ignore]
    async fn test_fetch_version_details() {
        let m = fetch_manifest().await.unwrap();
        let entry = m
            .versions
            .iter()
            .find(|v| v.id == m.latest.release)
            .unwrap();
        let d = fetch_version_details(&entry.url).await.unwrap();
        assert!(!d.main_class.is_empty());
        assert!(!d.libraries.is_empty());
        println!(
            "Версия {}, Java {}, всего библиотек {}, из них подходят {}",
            d.id,
            d.java_version.major_version,
            d.libraries.len(),
            usable_libraries_full(&d.libraries).len()
        );
    }

    #[test]
    fn test_library_rules() {
        assert!(is_library_allowed(&[]));
        assert!(is_library_allowed(&[Rule { action: "allow".into(), os: None }]));
        let disallow_current = vec![
            Rule { action: "allow".into(), os: None },
            Rule {
                action: "disallow".into(),
                os: Some(OsRule { name: Some(current_os().into()), arch: None }),
            },
        ];
        assert!(!is_library_allowed(&disallow_current));
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LibraryDownloadsFull {
    pub artifact: Option<Artifact>,
    #[serde(default)]
    pub classifiers: std::collections::HashMap<String, Artifact>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LibraryFull {
    pub name: String,
    #[serde(default)]
    pub rules: Vec<Rule>,
    pub downloads: Option<LibraryDownloadsFull>,
    #[serde(default)]
    pub natives: std::collections::HashMap<String, String>,
}

impl LibraryFull {
    pub fn is_allowed(&self) -> bool {
        is_library_allowed(&self.rules)
    }

    /// Имя классификатора нативов для текущей ОС (например, "natives-macos-arm64")
    pub fn native_classifier(&self) -> Option<&str> {
        self.natives.get(current_os()).map(|s| s.as_str())
    }
}

pub fn usable_libraries_full(libs: &[LibraryFull]) -> Vec<&Artifact> {
    libs.iter()
        .filter(|lib| lib.is_allowed())
        .filter_map(|lib| lib.downloads.as_ref())
        .filter_map(|d| d.artifact.as_ref())
        .collect()
}

/// Возвращает все версии, если include_all = true, иначе только релизы.
pub fn filter_versions<'a>(
    versions: &'a [VersionEntry],
    include_all: bool,
) -> Vec<&'a VersionEntry> {
    if include_all {
        versions.iter().collect()
    } else {
        versions.iter().filter(|v| v.version_type == "release").collect()
    }
}

#[cfg(test)]
mod filter_tests {
    use super::*;

    fn v(id: &str, t: &str) -> VersionEntry {
        VersionEntry {
            id: id.into(),
            version_type: t.into(),
            url: String::new(),
            time: String::new(),
            release_time: String::new(),
        }
    }

    #[test]
    fn only_release_by_default() {
        let list = vec![v("26.3", "release"), v("26.4-snap", "snapshot"), v("a1.2.6", "old_alpha")];
        let out = filter_versions(&list, false);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].id, "26.3");
    }

    #[test]
    fn all_types_when_requested() {
        let list = vec![v("26.3", "release"), v("26.4-snap", "snapshot"), v("a1.2.6", "old_alpha")];
        let out = filter_versions(&list, true);
        assert_eq!(out.len(), 3);
    }
}
