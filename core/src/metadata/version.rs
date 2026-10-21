//! JSON версии игры (и профили загрузчиков, которые наследуют его через `inheritsFrom`).
//! Одна модель на все форматы: современные `arguments` (1.13+) и старые `minecraftArguments`,
//! нативы через `classifiers` (≤1.18) и нативы-библиотеки (1.19+), Maven-библиотеки загрузчиков.

use super::rules::{Environment, Rule};
use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionJson {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inherits_from: Option<String>,
    #[serde(default, rename = "type", skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub main_class: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minecraft_arguments: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arguments: Option<Arguments>,
    #[serde(default)]
    pub libraries: Vec<Library>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub downloads: Option<Downloads>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asset_index: Option<AssetIndexRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assets: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub java_version: Option<JavaVersion>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logging: Option<Logging>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release_time: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Arguments {
    #[serde(default)]
    pub game: Vec<Argument>,
    #[serde(default)]
    pub jvm: Vec<Argument>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Argument {
    Plain(String),
    Conditional { rules: Vec<Rule>, value: ArgValue },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ArgValue {
    One(String),
    Many(Vec<String>),
}

impl Argument {
    /// Значения аргумента, разрешённые в данном окружении.
    pub fn resolve(&self, env: &Environment) -> Vec<String> {
        match self {
            Argument::Plain(s) => vec![s.clone()],
            Argument::Conditional { rules, value } if env.allows(rules) => match value {
                ArgValue::One(s) => vec![s.clone()],
                ArgValue::Many(v) => v.clone(),
            },
            Argument::Conditional { .. } => Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Downloads {
    #[serde(default)]
    pub client: Option<FileRef>,
}

/// Файл с публикованными хэшем и размером.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileRef {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub url: String,
    pub sha1: String,
    #[serde(default)]
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetIndexRef {
    pub id: String,
    pub url: String,
    pub sha1: String,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub total_size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaVersion {
    #[serde(default)]
    pub component: String,
    pub major_version: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Logging {
    #[serde(default)]
    pub client: Option<LoggingEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingEntry {
    /// Например `-Dlog4j.configurationFile=${path}`.
    pub argument: String,
    pub file: FileRef,
    #[serde(default, rename = "type")]
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Library {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub downloads: Option<LibraryDownloads>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub natives: Option<BTreeMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extract: Option<Extract>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rules: Option<Vec<Rule>>,
    /// База Maven-репозитория (библиотеки Fabric/Quilt без `downloads`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha1: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LibraryDownloads {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact: Option<Artifact>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub classifiers: Option<BTreeMap<String, Artifact>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Artifact {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha1: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    #[serde(default)]
    pub url: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Extract {
    #[serde(default)]
    pub exclude: Vec<String>,
}

/// Координаты Maven: `group:artifact:version[:classifier][@ext]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MavenCoord {
    pub group: String,
    pub artifact: String,
    pub version: String,
    pub classifier: Option<String>,
    pub ext: String,
}

impl MavenCoord {
    pub fn parse(name: &str) -> Option<Self> {
        let (coords, ext) = match name.split_once('@') {
            Some((c, e)) => (c, e.to_string()),
            None => (name, "jar".to_string()),
        };
        let parts: Vec<&str> = coords.split(':').collect();
        if parts.len() < 3 || parts.len() > 4 || parts.iter().any(|p| p.is_empty()) {
            return None;
        }
        Some(Self {
            group: parts[0].to_string(),
            artifact: parts[1].to_string(),
            version: parts[2].to_string(),
            classifier: parts.get(3).map(|s| s.to_string()),
            ext,
        })
    }

    /// Путь в Maven-репозитории.
    pub fn path(&self) -> String {
        let file = match &self.classifier {
            Some(c) => format!("{}-{}-{}.{}", self.artifact, self.version, c, self.ext),
            None => format!("{}-{}.{}", self.artifact, self.version, self.ext),
        };
        format!(
            "{}/{}/{}/{}",
            self.group.replace('.', "/"),
            self.artifact,
            self.version,
            file
        )
    }

    /// Ключ для поиска одинаковых библиотек разных версий: группа, артефакт, классификатор.
    pub fn identity(&self) -> String {
        format!(
            "{}:{}:{}",
            self.group,
            self.artifact,
            self.classifier.as_deref().unwrap_or("")
        )
    }
}

impl Library {
    pub fn coord(&self) -> Option<MavenCoord> {
        MavenCoord::parse(&self.name)
    }

    pub fn is_allowed(&self, env: &Environment) -> bool {
        self.rules.as_deref().map(|r| env.allows(r)).unwrap_or(true)
    }

    /// Основной JAR библиотеки (для classpath), если он есть.
    pub fn main_artifact(&self) -> Option<Artifact> {
        if let Some(art) = self.downloads.as_ref().and_then(|d| d.artifact.clone()) {
            let path = art
                .path
                .clone()
                .or_else(|| self.coord().map(|c| c.path()))?;
            return Some(Artifact {
                path: Some(path),
                ..art
            });
        }
        // Старый стиль (только нативы через classifiers) — основного JAR нет.
        if self.downloads.is_some() {
            return None;
        }
        // Maven-стиль загрузчиков: имя + база репозитория.
        let coord = self.coord()?;
        let base = self
            .url
            .clone()
            .unwrap_or_else(|| "https://libraries.minecraft.net/".to_string());
        let base = if base.ends_with('/') {
            base
        } else {
            format!("{base}/")
        };
        let path = coord.path();
        Some(Artifact {
            url: format!("{base}{path}"),
            path: Some(path),
            sha1: self.sha1.clone(),
            size: self.size,
        })
    }

    /// Архив нативов для этой ОС (старый формат `natives` + `classifiers`).
    pub fn native_artifact(&self, env: &Environment) -> Option<Artifact> {
        let classifier = self
            .natives
            .as_ref()?
            .get(env.os.mojang_name())?
            .replace("${arch}", env.arch.bits());
        let art = self
            .downloads
            .as_ref()?
            .classifiers
            .as_ref()?
            .get(&classifier)?
            .clone();
        let path = art.path.clone().or_else(|| {
            let mut c = self.coord()?;
            c.classifier = Some(classifier.clone());
            Some(c.path())
        })?;
        Some(Artifact {
            path: Some(path),
            ..art
        })
    }

    pub fn extract_excludes(&self) -> Vec<String> {
        self.extract
            .as_ref()
            .map(|e| e.exclude.clone())
            .unwrap_or_default()
    }
}

impl VersionJson {
    pub fn parse(json: &str) -> Result<Self> {
        serde_json::from_str(json).map_err(|e| Error::parse("JSON версии", e))
    }

    /// Наложить профиль загрузчика (`self`, у которого `inheritsFrom`) на родительскую версию.
    /// Библиотеки загрузчика идут первыми и заменяют одноимённые библиотеки родителя
    /// (одинаковые группа, артефакт и классификатор); аргументы складываются.
    pub fn merged_onto(self, parent: VersionJson) -> VersionJson {
        let child_ids: HashSet<String> = self
            .libraries
            .iter()
            .filter_map(|l| l.coord().map(|c| c.identity()))
            .collect();
        let mut libraries = self.libraries;
        libraries.extend(parent.libraries.into_iter().filter(|l| {
            l.coord()
                .map(|c| !child_ids.contains(&c.identity()))
                .unwrap_or(true)
        }));

        let arguments = match (parent.arguments, self.arguments) {
            (Some(p), Some(c)) => Some(Arguments {
                game: p.game.into_iter().chain(c.game).collect(),
                jvm: p.jvm.into_iter().chain(c.jvm).collect(),
            }),
            (p, c) => c.or(p),
        };

        VersionJson {
            id: self.id,
            inherits_from: None,
            kind: self.kind.or(parent.kind),
            main_class: self.main_class.or(parent.main_class),
            minecraft_arguments: self.minecraft_arguments.or(parent.minecraft_arguments),
            arguments,
            libraries,
            downloads: self.downloads.or(parent.downloads),
            asset_index: self.asset_index.or(parent.asset_index),
            assets: self.assets.or(parent.assets),
            java_version: self.java_version.or(parent.java_version),
            logging: self.logging.or(parent.logging),
            release_time: self.release_time.or(parent.release_time),
        }
    }

    /// Требуемая Java и откуда это известно.
    pub fn required_java(&self) -> (u32, String) {
        match &self.java_version {
            Some(j) => (
                j.major_version,
                format!(
                    "javaVersion.majorVersion = {} в JSON версии",
                    j.major_version
                ),
            ),
            // Поля нет только у самых старых версий (1.6.4 и раньше, проверено по манифесту
            // 21.10.2026); все они рассчитаны на Java 8 и ниже.
            None => (
                8,
                "в JSON версии нет javaVersion — для таких старых версий нужна Java 8".to_string(),
            ),
        }
    }

    pub fn main_class(&self) -> Result<&str> {
        self.main_class
            .as_deref()
            .ok_or_else(|| Error::invalid(format!("В версии {} не указан mainClass", self.id)))
    }

    /// Есть ли у версии нативы под arm64 для macOS (иначе на Apple Silicon нужна x64-Java
    /// под Rosetta). Определяется по данным, а не по номеру версии.
    pub fn has_macos_arm64_natives(&self) -> bool {
        self.libraries.iter().any(|l| {
            l.name.contains("natives-macos-arm64")
                || l.natives
                    .as_ref()
                    .and_then(|n| n.get("osx"))
                    .is_some_and(|c| c.contains("arm64"))
                || l.downloads
                    .as_ref()
                    .and_then(|d| d.classifiers.as_ref())
                    .is_some_and(|c| c.keys().any(|k| k.contains("macos-arm64")))
        })
    }

    /// Версия log4j-core из библиотек, если есть (для защиты от Log4Shell).
    pub fn log4j_core_version(&self) -> Option<String> {
        self.libraries.iter().find_map(|l| {
            let c = l.coord()?;
            (c.group == "org.apache.logging.log4j" && c.artifact == "log4j-core")
                .then_some(c.version)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadata::rules::{Arch, Os};

    fn fixture(name: &str) -> VersionJson {
        let path = format!(
            "{}/tests/fixtures/versions/{name}.json",
            env!("CARGO_MANIFEST_DIR")
        );
        VersionJson::parse(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn all_fixtures_parse() {
        for v in ["26.3", "1.21.11", "1.20.1", "1.16.5", "1.12.2", "1.6.4"] {
            let j = fixture(v);
            assert_eq!(j.id, v);
            assert!(j.main_class.is_some());
            assert!(
                j.downloads
                    .as_ref()
                    .and_then(|d| d.client.as_ref())
                    .is_some()
            );
        }
    }

    #[test]
    fn java_requirement_comes_from_data() {
        assert_eq!(fixture("26.3").required_java().0, 25);
        assert_eq!(fixture("1.21.11").required_java().0, 21);
        assert_eq!(fixture("1.20.1").required_java().0, 17);
        assert_eq!(fixture("1.16.5").required_java().0, 8);
        let (major, reason) = fixture("1.6.4").required_java();
        assert_eq!(major, 8);
        assert!(reason.contains("нет javaVersion"));
    }

    #[test]
    fn maven_coordinates() {
        let c = MavenCoord::parse("net.fabricmc:fabric-loader:0.16.0").unwrap();
        assert_eq!(
            c.path(),
            "net/fabricmc/fabric-loader/0.16.0/fabric-loader-0.16.0.jar"
        );
        let c = MavenCoord::parse("org.lwjgl:lwjgl:3.4.3:natives-macos-arm64").unwrap();
        assert_eq!(
            c.path(),
            "org/lwjgl/lwjgl/3.4.3/lwjgl-3.4.3-natives-macos-arm64.jar"
        );
        let c = MavenCoord::parse("de.oceanlabs.mcp:mcp_config:1.20.1@zip").unwrap();
        assert_eq!(
            c.path(),
            "de/oceanlabs/mcp/mcp_config/1.20.1/mcp_config-1.20.1.zip"
        );
        assert_eq!(MavenCoord::parse("bad"), None);
        assert_eq!(MavenCoord::parse("a::b"), None);
    }

    #[test]
    fn natives_old_format_with_arch_placeholder() {
        let lib: Library = serde_json::from_str(
            r#"{"name":"tv.twitch:twitch-platform:5.16",
                "natives":{"windows":"natives-windows-${arch}"},
                "downloads":{"classifiers":{"natives-windows-64":
                  {"path":"tv/x-64.jar","sha1":"aa","size":1,"url":"https://x/64"}}}}"#,
        )
        .unwrap();
        let env = Environment::new(Os::Windows, Arch::X86_64, "10.0");
        assert_eq!(lib.native_artifact(&env).unwrap().url, "https://x/64");
        assert!(
            lib.main_artifact().is_none(),
            "у такой библиотеки нет основного JAR"
        );
        assert!(
            lib.native_artifact(&Environment::new(Os::Osx, Arch::Arm64, ""))
                .is_none()
        );
    }

    #[test]
    fn legacy_natives_in_1_16_5_on_macos() {
        let j = fixture("1.16.5");
        let env = Environment::new(Os::Osx, Arch::Arm64, "15.1");
        let natives: Vec<_> = j
            .libraries
            .iter()
            .filter(|l| l.is_allowed(&env))
            .filter_map(|l| l.native_artifact(&env))
            .collect();
        assert!(!natives.is_empty());
        assert!(
            natives
                .iter()
                .all(|a| a.path.as_ref().unwrap().contains("natives-macos")
                    || a.path.as_ref().unwrap().contains("natives-osx"))
        );
        assert!(
            !j.has_macos_arm64_natives(),
            "1.16.5 не содержит arm64-нативов macOS"
        );
        assert!(fixture("26.3").has_macos_arm64_natives());
    }

    #[test]
    fn log4j_version_is_read() {
        assert_eq!(
            fixture("1.16.5").log4j_core_version().as_deref(),
            Some("2.8.1")
        );
        assert_eq!(fixture("1.6.4").log4j_core_version(), None);
    }

    #[test]
    fn fabric_profile_merges_onto_vanilla() {
        let merged = fixture("fabric-loader-26.3").merged_onto(fixture("26.3"));
        assert_eq!(
            merged.main_class.as_deref(),
            Some("net.fabricmc.loader.impl.launch.knot.KnotClient")
        );
        assert!(merged.inherits_from.is_none());
        assert_eq!(merged.required_java().0, 25, "Java берётся у ванили");
        assert!(merged.logging.is_some());
        let args = merged.arguments.as_ref().unwrap();
        let jvm: Vec<String> = args
            .jvm
            .iter()
            .flat_map(|a| a.resolve(&Environment::new(Os::Osx, Arch::Arm64, "15.1")))
            .collect();
        assert!(
            jvm.iter().any(|a| a.starts_with("-DFabricMcEmu=")),
            "JVM-аргумент Fabric не теряется"
        );
        assert!(
            jvm.iter().any(|a| a == "-XstartOnFirstThread"),
            "и ванильные тоже"
        );
        // Библиотеки Fabric идут первыми и имеют хэши.
        let first = merged.libraries[0].main_artifact().unwrap();
        assert!(first.sha1.is_some());
        assert!(first.url.starts_with("https://maven.fabricmc.net/"));
    }

    #[test]
    fn child_library_replaces_parent_with_same_identity() {
        let parent: VersionJson = serde_json::from_str(
            r#"{"id":"p","mainClass":"M","libraries":[
                {"name":"org.ow2.asm:asm:9.0","downloads":{"artifact":{"url":"https://a","sha1":"1","size":1}}},
                {"name":"com.x:y:1"}]}"#,
        )
        .unwrap();
        let child: VersionJson = serde_json::from_str(
            r#"{"id":"c","inheritsFrom":"p","libraries":[{"name":"org.ow2.asm:asm:9.10","url":"https://maven.fabricmc.net/"}]}"#,
        )
        .unwrap();
        let merged = child.merged_onto(parent);
        let names: Vec<_> = merged.libraries.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, ["org.ow2.asm:asm:9.10", "com.x:y:1"]);
        assert_eq!(merged.main_class.as_deref(), Some("M"));
    }
}
