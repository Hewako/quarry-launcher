//! Каталог CurseForge (API v1). Ключ API не хранится в коде: он подставляется при сборке
//! из переменной окружения `QUARRY_CURSEFORGE_KEY` (см. docs/CURSEFORGE_SETUP.md).
//! Без ключа каталог CurseForge в этой сборке недоступен, остальное работает.

use crate::catalog::{Channel, ProjectType, SearchHit};
use crate::error::{Error, Result};
use crate::instance::InstanceStore;
use crate::loader::LoaderKind;
use crate::mods::lock::{PackLock, Source};
use crate::mods::manage::{Change, ChangeKind, Plan};
use crate::net;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const API: &str = "https://api.curseforge.com/v1";
/// Minecraft в CurseForge.
const GAME_ID: u32 = 432;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CfHash {
    pub value: String,
    /// 1 — SHA-1, 2 — MD5.
    pub algo: u8,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CfDep {
    pub mod_id: u64,
    /// 3 — обязательная, 2 — необязательная, 5 — несовместима.
    pub relation_type: u8,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CfFile {
    pub id: u64,
    pub mod_id: u64,
    pub display_name: String,
    pub file_name: String,
    /// 1 — release, 2 — beta, 3 — alpha.
    pub release_type: u8,
    #[serde(default)]
    pub file_length: u64,
    /// `None` — автор запретил скачивание сторонними лаунчерами.
    #[serde(default)]
    pub download_url: Option<String>,
    #[serde(default)]
    pub hashes: Vec<CfHash>,
    #[serde(default)]
    pub dependencies: Vec<CfDep>,
}

/// Ключ, подставленный при сборке. Пустая строка считается отсутствием ключа.
pub fn api_key() -> Option<&'static str> {
    option_env!("QUARRY_CURSEFORGE_KEY").filter(|k| !k.trim().is_empty())
}

pub fn class_id(kind: ProjectType) -> u32 {
    match kind {
        ProjectType::Mod => 6,
        ProjectType::Resourcepack => 12,
        ProjectType::Shader => 6552,
        ProjectType::Datapack => 6945,
        ProjectType::Modpack => 4471,
    }
}

/// Тип загрузчика CurseForge. Для Quilt — Fabric: Quilt загружает моды Fabric, а модов
/// с отметкой Quilt на CurseForge мало.
pub fn loader_type(loader: LoaderKind) -> Option<u8> {
    match loader {
        LoaderKind::Forge => Some(1),
        LoaderKind::Fabric | LoaderKind::Quilt => Some(4),
        LoaderKind::NeoForge => Some(6),
        LoaderKind::Vanilla => None,
    }
}

pub fn search_url(
    query: &str,
    kind: ProjectType,
    loader: LoaderKind,
    game_version: &str,
    offset: u32,
) -> String {
    let mut u = format!(
        "{API}/mods/search?gameId={GAME_ID}&classId={}&gameVersion={}&searchFilter={}&sortField=2&sortOrder=desc&pageSize=20&index={offset}",
        class_id(kind),
        urlencoding::encode(game_version),
        urlencoding::encode(query.trim()),
    );
    if kind == ProjectType::Mod
        && let Some(t) = loader_type(loader)
    {
        u.push_str(&format!("&modLoaderType={t}"));
    }
    u
}

/// Новейший файл канала; файлы — новые первыми.
pub fn pick(files: &[CfFile], channel: Channel) -> Option<&CfFile> {
    let max_type = match channel {
        Channel::Auto => {
            return [1, 2, 3]
                .iter()
                .find_map(|t| files.iter().find(|f| f.release_type == *t));
        }
        Channel::Release => 1,
        Channel::Beta => 2,
        Channel::Alpha => 3,
    };
    files.iter().find(|f| f.release_type <= max_type)
}

impl CfFile {
    pub fn sha1(&self) -> Option<String> {
        self.hashes
            .iter()
            .find(|h| h.algo == 1)
            .map(|h| h.value.to_ascii_lowercase())
    }
    /// Ссылку на скачивание можно использовать: https и CDN CurseForge.
    pub fn trusted_url(&self) -> Option<&str> {
        let u = self.download_url.as_deref()?;
        let host = net::host_of(u);
        (u.starts_with("https://") && (host == "forgecdn.net" || host.ends_with(".forgecdn.net")))
            .then_some(u)
    }
}

#[derive(Deserialize)]
struct Data<T> {
    data: T,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CfMod {
    id: u64,
    #[serde(default)]
    slug: String,
    name: String,
    #[serde(default)]
    summary: String,
    #[serde(default)]
    download_count: f64,
    #[serde(default)]
    logo: Option<CfLogo>,
    #[serde(default)]
    authors: Vec<CfAuthor>,
    #[serde(default)]
    categories: Vec<CfCategory>,
    #[serde(default)]
    links: Option<CfLinks>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CfLogo {
    thumbnail_url: Option<String>,
}
#[derive(Deserialize)]
struct CfAuthor {
    name: String,
}
#[derive(Deserialize)]
struct CfCategory {
    slug: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CfLinks {
    website_url: Option<String>,
}

impl CfMod {
    fn hit(self) -> SearchHit {
        SearchHit {
            project_id: self.id.to_string(),
            slug: self.slug,
            title: self.name,
            description: self.summary,
            categories: self.categories.into_iter().map(|c| c.slug).collect(),
            client_side: String::new(),
            server_side: String::new(),
            downloads: self.download_count.max(0.0) as u64,
            icon_url: self.logo.and_then(|l| l.thumbnail_url),
            author: self
                .authors
                .into_iter()
                .next()
                .map(|a| a.name)
                .unwrap_or_default(),
        }
    }
}

pub fn parse_search(json: &str) -> Result<Vec<SearchHit>> {
    let d: Data<Vec<CfMod>> =
        serde_json::from_str(json).map_err(|e| Error::parse("ответ CurseForge", e))?;
    Ok(d.data.into_iter().map(CfMod::hit).collect())
}

fn no_key() -> Error {
    Error::invalid("Каталог CurseForge не подключён в этой сборке Quarry (нет ключа API)")
}

async fn get_json(url: &str) -> Result<String> {
    let key = api_key().ok_or_else(no_key)?;
    net::with_retries(|| async {
        let resp = net::client()
            .get(url)
            .header("x-api-key", key)
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|e| net::network_error(url, e))?;
        net::check_status(url, resp)
            .await?
            .text()
            .await
            .map_err(|e| net::network_error(url, e))
    })
    .await
}

pub async fn search(
    query: &str,
    kind: ProjectType,
    loader: LoaderKind,
    game_version: &str,
    offset: u32,
) -> Result<Vec<SearchHit>> {
    parse_search(&get_json(&search_url(query, kind, loader, game_version, offset)).await?)
}

async fn mod_info(mod_id: u64) -> Result<CfMod> {
    let d: Data<CfMod> = serde_json::from_str(&get_json(&format!("{API}/mods/{mod_id}")).await?)
        .map_err(|e| Error::parse("ответ CurseForge", e))?;
    Ok(d.data)
}

/// Более новый файл (стабильный, если есть), чем установленный `current_file_id`.
pub fn newer_file<'a>(files: &'a [CfFile], current_file_id: &str) -> Option<&'a CfFile> {
    pick(files, Channel::Auto).filter(|f| f.id.to_string() != current_file_id)
}

pub async fn files(mod_id: u64, loader: LoaderKind, game_version: &str) -> Result<Vec<CfFile>> {
    let mut url = format!(
        "{API}/mods/{mod_id}/files?gameVersion={}&pageSize=50",
        urlencoding::encode(game_version)
    );
    if let Some(t) = loader_type(loader) {
        url.push_str(&format!("&modLoaderType={t}"));
    }
    let mut d: Data<Vec<CfFile>> = serde_json::from_str(&get_json(&url).await?)
        .map_err(|e| Error::parse("ответ CurseForge", e))?;
    d.data.sort_by_key(|f| std::cmp::Reverse(f.id));
    Ok(d.data)
}

/// План установки мода с CurseForge вместе с обязательными зависимостями.
pub async fn plan_install(
    store: &InstanceStore,
    id: &str,
    mod_id: u64,
    channel: Channel,
) -> Result<Plan> {
    let inst = store.get(id)?;
    let lock = PackLock::load(&store.dir(id)?)?;
    let present: BTreeSet<String> = lock
        .files
        .iter()
        .filter(|f| f.source == Source::CurseForge)
        .filter_map(|f| f.project_id.clone())
        .collect();
    let mut plan = Plan {
        changes: Vec::new(),
        notes: Vec::new(),
    };
    let mut seen = BTreeSet::new();
    let mut queue = vec![(mod_id, "выбран вами".to_string())];
    while let Some((m, reason)) = queue.pop() {
        if !seen.insert(m) || seen.len() > 40 {
            continue;
        }
        let root = m == mod_id;
        if present.contains(&m.to_string()) {
            if root {
                return Err(Error::invalid("Этот мод уже установлен"));
            }
            continue;
        }
        let info = mod_info(m).await?;
        let list = files(m, inst.loader, &inst.game_version).await?;
        let Some(f) = pick(&list, channel).cloned() else {
            let msg = format!(
                "У «{}» нет подходящей версии для Minecraft {} и {}",
                info.name,
                inst.game_version,
                inst.loader.display()
            );
            if root {
                return Err(Error::invalid(msg));
            }
            plan.notes.push(msg);
            continue;
        };
        if f.trusted_url().is_none() {
            let page = info
                .links
                .as_ref()
                .and_then(|l| l.website_url.clone())
                .unwrap_or_default();
            let msg = format!(
                "Автор «{}» запретил скачивание в сторонних лаунчерах — скачайте файл вручную на странице {page} и перетащите его в окно",
                info.name
            );
            if root {
                return Err(Error::invalid(msg));
            }
            plan.notes.push(msg);
            continue;
        }
        for d in &f.dependencies {
            if d.relation_type == 3 {
                queue.push((d.mod_id, format!("нужен для {}", info.name)));
            }
        }
        plan.changes.push(Change {
            kind: ChangeKind::Add,
            title: info.name.clone(),
            file_name: Some(f.file_name.clone()),
            old_file: None,
            old_version: None,
            new_version: Some(f.display_name.clone()),
            reason,
            size: f.file_length,
            version: None,
            cf_file: Some(f),
        });
    }
    plan.changes.reverse();
    Ok(plan)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(id: u64, rt: u8, url: Option<&str>) -> CfFile {
        CfFile {
            id,
            mod_id: 1,
            display_name: format!("v{id}"),
            file_name: format!("m-{id}.jar"),
            release_type: rt,
            file_length: 10,
            download_url: url.map(str::to_string),
            hashes: vec![
                CfHash {
                    value: "ABC123".into(),
                    algo: 1,
                },
                CfHash {
                    value: "md5".into(),
                    algo: 2,
                },
            ],
            dependencies: vec![],
        }
    }

    #[test]
    fn search_url_uses_class_loader_and_version() {
        let u = search_url("jei", ProjectType::Mod, LoaderKind::NeoForge, "1.21.1", 20);
        assert!(u.starts_with("https://api.curseforge.com/v1/mods/search?gameId=432&classId=6"));
        assert!(
            u.contains("modLoaderType=6")
                && u.contains("gameVersion=1.21.1")
                && u.contains("index=20")
        );
        assert!(u.contains("searchFilter=jei"));
        // Quilt грузит моды Fabric; у ресурспаков загрузчика нет.
        assert!(
            search_url("", ProjectType::Mod, LoaderKind::Quilt, "1.21.1", 0)
                .contains("modLoaderType=4")
        );
        assert!(
            !search_url(
                "",
                ProjectType::Resourcepack,
                LoaderKind::Fabric,
                "1.21.1",
                0
            )
            .contains("modLoaderType")
        );
        assert_eq!(class_id(ProjectType::Shader), 6552);
    }

    #[test]
    fn newest_file_by_channel() {
        let files = vec![file(30, 3, None), file(20, 2, None), file(10, 1, None)];
        assert_eq!(pick(&files, Channel::Auto).unwrap().id, 10);
        assert_eq!(pick(&files, Channel::Beta).unwrap().id, 20);
        assert_eq!(pick(&files, Channel::Alpha).unwrap().id, 30);
        assert!(pick(&files[..2], Channel::Release).is_none());
    }

    #[test]
    fn hashes_and_download_hosts() {
        assert_eq!(file(1, 1, None).sha1().as_deref(), Some("abc123"));
        assert_eq!(file(1, 1, None).trusted_url(), None);
        let ok = file(1, 1, Some("https://edge.forgecdn.net/files/1/2/m.jar"));
        assert!(ok.trusted_url().is_some());
        assert!(
            file(
                1,
                1,
                Some("https://mediafilez.forgecdn.net/files/1/2/m.jar")
            )
            .trusted_url()
            .is_some()
        );
        assert!(
            file(1, 1, Some("http://edge.forgecdn.net/x.jar"))
                .trusted_url()
                .is_none()
        );
        assert!(
            file(1, 1, Some("https://evil.example/forgecdn.net/x.jar"))
                .trusted_url()
                .is_none()
        );
    }

    #[test]
    fn search_response_becomes_hits() {
        let json = r#"{"data":[{"id":238222,"slug":"jei","name":"Just Enough Items","summary":"Рецепты","downloadCount":3.5E8,
            "logo":{"thumbnailUrl":"https://media.forgecdn.net/x.png"},"authors":[{"name":"mezz"}],"categories":[{"slug":"utility"}]}],
            "pagination":{"totalCount":1}}"#;
        let hits = parse_search(json).unwrap();
        assert_eq!(hits[0].project_id, "238222");
        assert_eq!(
            (
                hits[0].title.as_str(),
                hits[0].author.as_str(),
                hits[0].downloads
            ),
            ("Just Enough Items", "mezz", 350_000_000)
        );
        assert_eq!(hits[0].categories, ["utility"]);
    }

    #[test]
    fn update_only_when_newer_file_in_channel() {
        let files = vec![file(30, 2, None), file(20, 1, None), file(10, 1, None)];
        // Стоит 10, новейшая стабильная — 20 (бета 30 не предлагается в канале «авто»).
        assert_eq!(newer_file(&files, "10").map(|f| f.id), Some(20));
        assert!(newer_file(&files, "20").is_none(), "уже последняя");
        assert!(newer_file(&[], "10").is_none());
    }
}
