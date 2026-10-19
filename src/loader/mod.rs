use crate::metadata::{self, VersionDetails};

type BoxErr = Box<dyn std::error::Error + Send + Sync>;

const FABRIC_META: &str = "https://meta.fabricmc.net/v2";
const FABRIC_MAVEN: &str = "https://maven.fabricmc.net/";

#[derive(Debug, serde::Deserialize)]
struct LoaderEntry {
    loader: LoaderVersion,
}

#[derive(Debug, serde::Deserialize)]
struct LoaderVersion {
    version: String,
    #[serde(default)]
    stable: bool,
}

#[derive(Debug, serde::Deserialize)]
struct FabricProfile {
    #[serde(rename = "mainClass")]
    main_class: String,
    #[serde(default)]
    libraries: Vec<FabricLibraryRaw>,
    #[serde(rename = "inheritsFrom")]
    inherits_from: String,
    #[serde(default)]
    arguments: Option<serde_json::Value>,
}

#[derive(Debug, serde::Deserialize)]
struct FabricLibraryRaw {
    name: String,
    #[serde(default)]
    url: Option<String>,
}

#[derive(Debug)]
pub struct MavenLib {
    pub path: String,
    pub url: String,
}

#[derive(Debug)]
pub struct FabricInstall {
    pub details: VersionDetails,
    pub maven_libs: Vec<MavenLib>,
    pub loader_version: String,
}

pub async fn latest_fabric_loader(game_version: &str) -> Result<String, BoxErr> {
    let url = format!("{}/versions/loader/{}", FABRIC_META, game_version);
    let client = reqwest::Client::builder().user_agent(crate::USER_AGENT).build()?;
    let resp = client.get(&url).send().await?;
    if !resp.status().is_success() {
        return Err(format!("Fabric meta вернул HTTP {}", resp.status()).into());
    }
    let list: Vec<LoaderEntry> = resp.json().await?;
    let stable = list.iter().find(|e| e.loader.stable).map(|e| e.loader.version.clone());
    let any = list.first().map(|e| e.loader.version.clone());
    stable.or(any).ok_or_else(|| format!("Нет загрузчика Fabric для {}", game_version).into())
}

fn maven_path(name: &str) -> Option<String> {
    let parts: Vec<&str> = name.split(':').collect();
    if parts.len() < 3 {
        return None;
    }
    let group = parts[0].replace('.', "/");
    let artifact = parts[1];
    let version = parts[2];
    let file = format!("{}-{}.jar", artifact, version);
    Some(format!("{}/{}/{}/{}", group, artifact, version, file))
}

pub async fn install_fabric(
    game_version: &str,
    loader_version: Option<&str>,
) -> Result<FabricInstall, BoxErr> {
    let loader = match loader_version {
        Some(v) => v.to_string(),
        None => latest_fabric_loader(game_version).await?,
    };

    let manifest = metadata::fetch_manifest().await?;
    let vanilla_entry = manifest
        .versions
        .iter()
        .find(|v| v.id == game_version)
        .ok_or_else(|| format!("Ванильная версия {} не найдена", game_version))?;
    let mut vanilla = metadata::fetch_version_details(&vanilla_entry.url).await?;

    let url = format!(
        "{}/versions/loader/{}/{}/profile/json",
        FABRIC_META, game_version, loader
    );
    let client = reqwest::Client::builder().user_agent(crate::USER_AGENT).build()?;
    let resp = client.get(&url).send().await?;
    if !resp.status().is_success() {
        return Err(format!("Fabric profile вернул HTTP {}", resp.status()).into());
    }
    let fabric: FabricProfile = resp.json().await?;

    if fabric.inherits_from != game_version {
        return Err(format!(
            "Fabric profile наследует {}, а не {}",
            fabric.inherits_from, game_version
        )
        .into());
    }

    let mut maven_libs = Vec::new();
    for lib in &fabric.libraries {
        if let Some(path) = maven_path(&lib.name) {
            let base = lib.url.as_deref().unwrap_or(FABRIC_MAVEN);
            let base = if base.ends_with('/') {
                base.to_string()
            } else {
                format!("{}/", base)
            };
            maven_libs.push(MavenLib {
                path: path.clone(),
                url: format!("{}{}", base, path),
            });
        }
    }

    vanilla.main_class = fabric.main_class;
    vanilla.arguments = fabric.arguments;
    vanilla.inherits_from = Some(game_version.to_string());

    Ok(FabricInstall {
        details: vanilla,
        maven_libs,
        loader_version: loader,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maven_path_basic() {
        assert_eq!(
            maven_path("net.fabricmc:fabric-loader:0.16.0"),
            Some("net/fabricmc/fabric-loader/0.16.0/fabric-loader-0.16.0.jar".to_string())
        );
    }

    #[test]
    fn maven_path_invalid() {
        assert_eq!(maven_path("bad"), None);
    }

    #[tokio::test]
    #[ignore]
    async fn fabric_profile_merges() {
        let install = install_fabric("26.3", None).await.unwrap();
        println!("main_class: {}", install.details.main_class);
        println!("загрузчик: {}", install.loader_version);
        println!("ванильных библиотек: {}", install.details.libraries.len());
        println!("maven-библиотек: {}", install.maven_libs.len());
        assert!(install.details.main_class.contains("fabricmc"));
        assert!(!install.maven_libs.is_empty(), "Должны быть maven-библиотеки");
        assert!(install.maven_libs.iter().any(|m| m.path.contains("fabric-loader")));
    }
}
