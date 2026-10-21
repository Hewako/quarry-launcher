//! Загрузчики модов. Каждый — отдельный адаптер со своей схемой установки, без «универсальной
//! магии». Fabric и Quilt описывают установку профилем JSON поверх ванили (без запуска
//! установщика). NeoForge — профиль из установщика плюс процессоры (см. `neoforge`).
//! Forge — отдельное решение после v1.0 (план, Приложение А).

pub mod neoforge;

use crate::error::{Error, Result};
use crate::metadata::{self, VersionJson};
use crate::paths::DataDir;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LoaderKind {
    Vanilla,
    Fabric,
    Quilt,
    NeoForge,
    Forge,
}

impl LoaderKind {
    pub fn parse(s: &str) -> Result<Self> {
        match s.to_ascii_lowercase().as_str() {
            "vanilla" | "" => Ok(Self::Vanilla),
            "fabric" => Ok(Self::Fabric),
            "quilt" => Ok(Self::Quilt),
            "neoforge" => Ok(Self::NeoForge),
            "forge" => Ok(Self::Forge),
            other => Err(Error::invalid(format!("Неизвестный загрузчик: {other}"))),
        }
    }
    pub fn display(self) -> &'static str {
        match self {
            Self::Vanilla => "Vanilla",
            Self::Fabric => "Fabric",
            Self::Quilt => "Quilt",
            Self::NeoForge => "NeoForge",
            Self::Forge => "Forge",
        }
    }
    pub fn is_supported(self) -> bool {
        matches!(
            self,
            Self::Vanilla | Self::Fabric | Self::Quilt | Self::NeoForge
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LoaderVersion {
    pub version: String,
    pub stable: bool,
}

/// Версии загрузчика для версии игры (новые первыми). Пустой список — загрузчик эту версию
/// игры не поддерживает.
pub async fn versions(kind: LoaderKind, game: &str) -> Result<Vec<LoaderVersion>> {
    metadata::file_safe(game)?;
    match kind {
        LoaderKind::Vanilla => Ok(Vec::new()),
        LoaderKind::Fabric => fabric::versions(game).await,
        LoaderKind::Quilt => quilt::versions(game).await,
        LoaderKind::NeoForge => neoforge::versions(game).await,
        LoaderKind::Forge => Err(unsupported(kind)),
    }
}

/// Версия загрузчика по умолчанию — последняя стабильная, иначе последняя.
pub fn default_version(list: &[LoaderVersion]) -> Option<&LoaderVersion> {
    list.iter().find(|v| v.stable).or_else(|| list.first())
}

fn unsupported(kind: LoaderKind) -> Error {
    Error::invalid(format!(
        "{} пока не поддерживается: его установщик требует выполнения процессоров, это отдельная работа (этап 3).",
        kind.display()
    ))
}

/// Проверить совместимость и выбрать версию загрузчика — до скачивания чего-либо.
pub async fn resolve_version(
    kind: LoaderKind,
    game: &str,
    requested: Option<&str>,
) -> Result<Option<String>> {
    if kind == LoaderKind::Vanilla {
        return Ok(None);
    }
    let list = versions(kind, game).await?;
    if list.is_empty() {
        return Err(Error::invalid(format!(
            "{} не поддерживает Minecraft {game}. Выберите другую версию игры или загрузчик.",
            kind.display()
        )));
    }
    match requested {
        Some(v) if list.iter().any(|l| l.version == v) => Ok(Some(v.to_string())),
        Some(v) => Err(Error::invalid(format!(
            "{} {v} нет для Minecraft {game}",
            kind.display()
        ))),
        None => Ok(default_version(&list).map(|v| v.version.clone())),
    }
}

/// Полный JSON версии с загрузчиком (профиль загрузчика наложен на ваниль).
pub async fn resolve_profile(
    data: &DataDir,
    kind: LoaderKind,
    game: &str,
    loader_version: Option<&str>,
) -> Result<VersionJson> {
    let vanilla = metadata::vanilla_version(data, game).await?;
    let (child, cache_name) = match kind {
        LoaderKind::Vanilla => return Ok(vanilla),
        LoaderKind::Fabric | LoaderKind::Quilt => {
            let lv =
                loader_version.ok_or_else(|| Error::invalid("Не указана версия загрузчика"))?;
            metadata::file_safe(lv)?;
            let name = format!("{}-{lv}-{game}", kind.display().to_ascii_lowercase());
            let url = match kind {
                LoaderKind::Fabric => fabric::profile_url(game, lv),
                _ => quilt::profile_url(game, lv),
            };
            let text = match crate::net::get_text(&url).await {
                Ok(t) => {
                    metadata::cache_json(data, "loaders", &name, &t)?;
                    t
                }
                Err(e) => metadata::read_cached_json(data, "loaders", &name).ok_or(e)?,
            };
            (VersionJson::parse(&text)?, name)
        }
        LoaderKind::NeoForge => {
            let lv = loader_version.ok_or_else(|| Error::invalid("Не указана версия NeoForge"))?;
            (
                neoforge::profile(data, game, lv).await?,
                format!("neoforge-{lv}"),
            )
        }
        LoaderKind::Forge => return Err(unsupported(kind)),
    };
    if child.inherits_from.as_deref() != Some(game) {
        return Err(Error::invalid(format!(
            "Профиль {cache_name} наследует {:?}, а не {game}",
            child.inherits_from
        )));
    }
    Ok(child.merged_onto(vanilla))
}

#[derive(Debug, Deserialize)]
struct LoaderEntry {
    loader: LoaderInfo,
}

#[derive(Debug, Deserialize)]
struct LoaderInfo {
    version: String,
    #[serde(default)]
    stable: Option<bool>,
}

mod fabric {
    use super::*;
    const META: &str = "https://meta.fabricmc.net/v2";

    pub fn profile_url(game: &str, loader: &str) -> String {
        format!(
            "{META}/versions/loader/{}/{}/profile/json",
            urlencoding::encode(game),
            urlencoding::encode(loader)
        )
    }

    pub async fn versions(game: &str) -> Result<Vec<LoaderVersion>> {
        let url = format!("{META}/versions/loader/{}", urlencoding::encode(game));
        let list: Vec<LoaderEntry> = crate::net::get_json(&url).await?;
        Ok(list
            .into_iter()
            .map(|e| LoaderVersion {
                stable: e.loader.stable.unwrap_or(false),
                version: e.loader.version,
            })
            .collect())
    }
}

mod quilt {
    use super::*;
    const META: &str = "https://meta.quiltmc.org/v3";

    pub fn profile_url(game: &str, loader: &str) -> String {
        format!(
            "{META}/versions/loader/{}/{}/profile/json",
            urlencoding::encode(game),
            urlencoding::encode(loader)
        )
    }

    /// У Quilt нет флага stable: предварительные версии видны по суффиксу (`-beta.9`).
    pub fn is_stable(version: &str) -> bool {
        !version.contains('-')
    }

    pub async fn versions(game: &str) -> Result<Vec<LoaderVersion>> {
        let url = format!("{META}/versions/loader/{}", urlencoding::encode(game));
        let list: Vec<LoaderEntry> = crate::net::get_json(&url).await?;
        Ok(list
            .into_iter()
            .map(|e| LoaderVersion {
                stable: e
                    .loader
                    .stable
                    .unwrap_or_else(|| is_stable(&e.loader.version)),
                version: e.loader.version,
            })
            .collect())
    }

    #[cfg(test)]
    #[test]
    fn beta_is_not_stable() {
        assert!(!is_stable("0.20.0-beta.9"));
        assert!(is_stable("0.29.2"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_parse() {
        assert_eq!(LoaderKind::parse("Fabric").unwrap(), LoaderKind::Fabric);
        assert_eq!(LoaderKind::parse("vanilla").unwrap(), LoaderKind::Vanilla);
        assert!(LoaderKind::parse("rift").is_err());
        assert!(LoaderKind::NeoForge.is_supported());
        assert!(!LoaderKind::Forge.is_supported());
    }

    #[test]
    fn default_prefers_stable() {
        let list = vec![
            LoaderVersion {
                version: "0.20.1".into(),
                stable: false,
            },
            LoaderVersion {
                version: "0.19.2".into(),
                stable: true,
            },
        ];
        assert_eq!(default_version(&list).unwrap().version, "0.19.2");
        let only_beta = vec![LoaderVersion {
            version: "0.1-beta".into(),
            stable: false,
        }];
        assert_eq!(default_version(&only_beta).unwrap().version, "0.1-beta");
        assert!(default_version(&[]).is_none());
    }

    #[test]
    fn quilt_profile_parses_and_merges() {
        let dir = format!("{}/tests/fixtures/versions", env!("CARGO_MANIFEST_DIR"));
        let child = VersionJson::parse(
            &std::fs::read_to_string(format!("{dir}/quilt-loader-1.21.11.json")).unwrap(),
        )
        .unwrap();
        let parent =
            VersionJson::parse(&std::fs::read_to_string(format!("{dir}/1.21.11.json")).unwrap())
                .unwrap();
        assert_eq!(child.inherits_from.as_deref(), Some("1.21.11"));
        let merged = child.merged_onto(parent);
        assert!(merged.main_class.unwrap().starts_with("org.quiltmc"));
        // У библиотек Quilt хэшей в профиле нет — установщик возьмёт их из Maven.
        assert!(merged.libraries[0].main_artifact().unwrap().sha1.is_none());
    }

    #[tokio::test]
    #[ignore = "сеть: meta.fabricmc.net и meta.quiltmc.org"]
    async fn real_loader_lists() {
        let f = versions(LoaderKind::Fabric, "26.3").await.unwrap();
        assert!(!f.is_empty());
        let q = versions(LoaderKind::Quilt, "1.21.11").await.unwrap();
        assert!(!q.is_empty());
        assert!(
            resolve_version(LoaderKind::Fabric, "rd-132211", None)
                .await
                .is_err()
        );
    }
}
