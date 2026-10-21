//! Каталог Modrinth (api.modrinth.com/v2). Сверено с ответами API 21.10.2026:
//! уникальный User-Agent, лимит 300 запросов в минуту (заголовки x-ratelimit-*),
//! поиск с фильтрами, версии проекта по загрузчику и версии игры, поиск версии по хэшу
//! файла, проверка обновлений по хэшам, зависимости (`dependency_type: required`).

use crate::error::{Error, Result};
use crate::net;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

const API: &str = "https://api.modrinth.com/v2";

/// Остаток лимита и время сброса из последнего ответа.
static REMAINING: AtomicU64 = AtomicU64::new(300);
static RESET_SECS: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchHit {
    pub project_id: String,
    pub slug: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub client_side: String,
    #[serde(default)]
    pub server_side: String,
    #[serde(default)]
    pub downloads: u64,
    #[serde(default)]
    pub icon_url: Option<String>,
    #[serde(default)]
    pub author: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub hits: Vec<SearchHit>,
    pub total_hits: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionFile {
    pub url: String,
    pub filename: String,
    #[serde(default)]
    pub primary: bool,
    pub size: u64,
    pub hashes: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dependency {
    #[serde(default)]
    pub version_id: Option<String>,
    #[serde(default)]
    pub project_id: Option<String>,
    pub dependency_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Version {
    pub id: String,
    pub project_id: String,
    pub name: String,
    pub version_number: String,
    #[serde(default)]
    pub version_type: String,
    #[serde(default)]
    pub game_versions: Vec<String>,
    #[serde(default)]
    pub loaders: Vec<String>,
    #[serde(default)]
    pub dependencies: Vec<Dependency>,
    pub files: Vec<VersionFile>,
}

impl Version {
    pub fn primary_file(&self) -> Option<&VersionFile> {
        self.files
            .iter()
            .find(|f| f.primary)
            .or_else(|| self.files.first())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub slug: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub client_side: String,
    #[serde(default)]
    pub server_side: String,
}

/// Подождать, если лимит запросов почти исчерпан.
async fn respect_limit() {
    if REMAINING.load(Ordering::Relaxed) < 5 {
        let wait = RESET_SECS.load(Ordering::Relaxed).clamp(1, 60);
        tokio::time::sleep(Duration::from_secs(wait)).await;
    }
}

fn remember_limit(resp: &reqwest::Response) {
    let num = |name: &str| {
        resp.headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok())
    };
    if let Some(r) = num("x-ratelimit-remaining") {
        REMAINING.store(r, Ordering::Relaxed);
    }
    if let Some(r) = num("x-ratelimit-reset") {
        RESET_SECS.store(r, Ordering::Relaxed);
    }
}

async fn send<T: for<'de> Deserialize<'de>>(req: reqwest::RequestBuilder, url: &str) -> Result<T> {
    respect_limit().await;
    let resp = req.send().await.map_err(|e| net::network_error(url, e))?;
    remember_limit(&resp);
    let resp = net::check_status(url, resp).await?;
    resp.json()
        .await
        .map_err(|e| Error::parse("ответ Modrinth", e))
}

async fn get<T: for<'de> Deserialize<'de>>(url: &str) -> Result<T> {
    net::with_retries(|| send(net::client().get(url), url)).await
}

async fn post<T: for<'de> Deserialize<'de>>(url: &str, body: &serde_json::Value) -> Result<T> {
    net::with_retries(|| send(net::client().post(url).json(body), url)).await
}

/// Названия загрузчиков в Modrinth для профиля. Quilt загружает и моды Fabric.
pub fn loaders_for(loader: crate::loader::LoaderKind) -> Vec<&'static str> {
    use crate::loader::LoaderKind::*;
    match loader {
        Fabric => vec!["fabric"],
        Quilt => vec!["quilt", "fabric"],
        NeoForge => vec!["neoforge"],
        Forge => vec!["forge"],
        Vanilla => vec![],
    }
}

fn json_list(items: &[&str]) -> String {
    serde_json::to_string(items).unwrap_or_else(|_| "[]".into())
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ProjectType {
    Mod,
    Resourcepack,
    Shader,
    Datapack,
}

impl ProjectType {
    fn as_str(self) -> &'static str {
        match self {
            ProjectType::Mod => "mod",
            ProjectType::Resourcepack => "resourcepack",
            ProjectType::Shader => "shader",
            ProjectType::Datapack => "datapack",
        }
    }
}

/// Адрес поиска Modrinth с фильтрами: тип, версия игры, загрузчик (для модов), категория.
pub fn search_url(
    query: &str,
    kind: ProjectType,
    loaders: &[&str],
    game_version: &str,
    offset: u32,
    category: Option<&str>,
) -> String {
    let mut facets: Vec<Vec<String>> = vec![
        vec![format!("project_type:{}", kind.as_str())],
        vec![format!("versions:{game_version}")],
    ];
    if kind == ProjectType::Mod && !loaders.is_empty() {
        facets.push(loaders.iter().map(|l| format!("categories:{l}")).collect());
    }
    if let Some(c) = category.filter(|c| !c.is_empty()) {
        facets.push(vec![format!("categories:{c}")]);
    }
    format!(
        "{API}/search?query={}&limit=20&offset={offset}&index=relevance&facets={}",
        urlencoding::encode(query),
        urlencoding::encode(&serde_json::to_string(&facets).unwrap_or_default())
    )
}

pub async fn search(
    query: &str,
    kind: ProjectType,
    loaders: &[&str],
    game_version: &str,
    offset: u32,
    category: Option<&str>,
) -> Result<SearchResult> {
    get(&search_url(
        query,
        kind,
        loaders,
        game_version,
        offset,
        category,
    ))
    .await
}

pub async fn project(id: &str) -> Result<Project> {
    get(&format!("{API}/project/{}", urlencoding::encode(id))).await
}

/// Версии проекта для загрузчика и версии игры (новые первыми, как отдаёт API).
pub async fn versions(project: &str, loaders: &[&str], game_version: &str) -> Result<Vec<Version>> {
    let mut url = format!(
        "{API}/project/{}/version?game_versions={}",
        urlencoding::encode(project),
        urlencoding::encode(&json_list(&[game_version]))
    );
    if !loaders.is_empty() {
        url.push_str(&format!(
            "&loaders={}",
            urlencoding::encode(&json_list(loaders))
        ));
    }
    get(&url).await
}

pub async fn version(id: &str) -> Result<Version> {
    get(&format!("{API}/version/{}", urlencoding::encode(id))).await
}

/// Найти версии Modrinth по SHA-1 файлов (опознать файлы, положенные вручную).
pub async fn versions_by_hash(sha1s: &[String]) -> Result<BTreeMap<String, Version>> {
    if sha1s.is_empty() {
        return Ok(BTreeMap::new());
    }
    post(
        &format!("{API}/version_files"),
        &serde_json::json!({ "hashes": sha1s, "algorithm": "sha1" }),
    )
    .await
}

/// Последние подходящие версии для файлов (по SHA-1): ключ — хэш текущего файла.
pub async fn updates_by_hash(
    sha1s: &[String],
    loaders: &[&str],
    game_version: &str,
) -> Result<BTreeMap<String, Version>> {
    if sha1s.is_empty() {
        return Ok(BTreeMap::new());
    }
    post(
        &format!("{API}/version_files/update"),
        &serde_json::json!({ "hashes": sha1s, "algorithm": "sha1", "loaders": loaders, "game_versions": [game_version] }),
    )
    .await
}

/// Канал обновлений: насколько нестабильные версии допускаются.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Channel {
    /// Стабильная, если есть; иначе бета, иначе альфа.
    #[default]
    Auto,
    /// Только стабильные.
    Release,
    /// Новейшая из стабильных и бета.
    Beta,
    /// Новейшая любая, включая альфа.
    Alpha,
}

/// Версия по каналу; список — в порядке API (новые первыми).
pub fn pick(list: &[Version], channel: Channel) -> Option<&Version> {
    let allowed: &[&str] = match channel {
        Channel::Auto => return pick_best(list),
        Channel::Release => &["release"],
        Channel::Beta => &["release", "beta"],
        Channel::Alpha => &["release", "beta", "alpha"],
    };
    list.iter()
        .find(|v| allowed.contains(&v.version_type.as_str()))
}

/// Лучшая версия: сначала release, потом beta, потом alpha; внутри — порядок API (новые первыми).
pub fn pick_best(list: &[Version]) -> Option<&Version> {
    ["release", "beta", "alpha"]
        .iter()
        .find_map(|t| list.iter().find(|v| v.version_type == *t))
        .or_else(|| list.first())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(id: &str, t: &str) -> Version {
        Version {
            id: id.into(),
            project_id: "p".into(),
            name: id.into(),
            version_number: id.into(),
            version_type: t.into(),
            game_versions: vec![],
            loaders: vec![],
            dependencies: vec![],
            files: vec![],
        }
    }

    #[test]
    fn best_version_prefers_release() {
        let list = vec![v("b2", "beta"), v("r1", "release"), v("a3", "alpha")];
        assert_eq!(pick_best(&list).unwrap().id, "r1");
        let betas = vec![v("b2", "beta"), v("b1", "beta")];
        assert_eq!(pick_best(&betas).unwrap().id, "b2");
        assert!(pick_best(&[]).is_none());
    }

    #[test]
    fn quilt_uses_fabric_mods_too() {
        assert_eq!(
            loaders_for(crate::loader::LoaderKind::Quilt),
            ["quilt", "fabric"]
        );
        assert!(loaders_for(crate::loader::LoaderKind::Vanilla).is_empty());
    }

    #[test]
    fn real_response_shape_parses() {
        // Сокращённый ответ /v2/project/iris/version (21.10.2026).
        let json = r#"[{"id":"x","project_id":"YL57xq9U","name":"Iris 1.9","version_number":"1.9.0+1.21.11-fabric","version_type":"release",
          "game_versions":["1.21.11"],"loaders":["fabric"],
          "dependencies":[{"version_id":"rkdTcxoT","project_id":"AANobbMI","file_name":null,"dependency_type":"required"}],
          "files":[{"url":"https://cdn.modrinth.com/data/x/iris.jar","filename":"iris.jar","primary":true,"size":10,"hashes":{"sha1":"a","sha512":"b"}}]}]"#;
        let list: Vec<Version> = serde_json::from_str(json).unwrap();
        assert_eq!(
            list[0].dependencies[0].project_id.as_deref(),
            Some("AANobbMI")
        );
        assert_eq!(list[0].primary_file().unwrap().hashes["sha512"], "b");
    }

    #[tokio::test]
    #[ignore = "сеть: api.modrinth.com"]
    async fn live_search_and_versions() {
        let r = search("sodium", ProjectType::Mod, &["fabric"], "1.21.11", 0, None)
            .await
            .unwrap();
        assert!(r.hits.iter().any(|h| h.slug == "sodium"));
        let vs = versions("sodium", &["fabric"], "1.21.11").await.unwrap();
        assert!(pick_best(&vs).is_some());
    }

    #[test]
    fn channel_limits_instability() {
        // Новые первыми, как отдаёт API.
        let list = vec![v("a3", "alpha"), v("b2", "beta"), v("r1", "release")];
        assert_eq!(pick(&list, Channel::Auto).unwrap().id, "r1");
        assert_eq!(pick(&list, Channel::Release).unwrap().id, "r1");
        assert_eq!(pick(&list, Channel::Beta).unwrap().id, "b2");
        assert_eq!(pick(&list, Channel::Alpha).unwrap().id, "a3");
        let betas = vec![v("b2", "beta")];
        assert!(pick(&betas, Channel::Release).is_none());
        assert_eq!(pick(&betas, Channel::Auto).unwrap().id, "b2");
    }

    #[test]
    fn search_url_has_category_facet() {
        let url = search_url(
            "sodium",
            ProjectType::Mod,
            &["fabric"],
            "26.3",
            20,
            Some("optimization"),
        );
        let facets = urlencoding::decode(url.split("facets=").nth(1).unwrap())
            .unwrap()
            .into_owned();
        assert!(facets.contains("\"categories:optimization\""), "{facets}");
        assert!(facets.contains("\"categories:fabric\""));
        assert!(url.contains("offset=20"));
        assert!(!search_url("", ProjectType::Shader, &[], "26.3", 0, None).contains("categories"));
    }
}
