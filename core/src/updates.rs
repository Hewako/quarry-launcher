//! Проверка новой версии Axel' на странице релизов GitHub. Ничего не скачивает и не
//! устанавливает сама: лаунчер только предлагает открыть страницу релиза.

use crate::error::Result;
use serde::Serialize;

pub const RELEASES_URL: &str = "https://github.com/Hewako/quarry-launcher/releases";

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Release {
    pub version: String,
    pub url: String,
    pub notes: String,
}

/// Новее ли `candidate` (например `v0.2.0`), чем `current` (`0.1.0`). Предрелизы
/// (`-beta.1`) считаются старше той же версии без суффикса.
pub fn is_newer(candidate: &str, current: &str) -> bool {
    fn parse(v: &str) -> Option<(Vec<u64>, bool)> {
        let v = v.trim().trim_start_matches('v');
        let (nums, pre) = v.split_once('-').map_or((v, false), |(n, _)| (n, true));
        let n: Vec<u64> = nums
            .split('.')
            .map(|p| p.parse().ok())
            .collect::<Option<_>>()?;
        (n.len() == 3).then_some((n, pre))
    }
    match (parse(candidate), parse(current)) {
        (Some((a, a_pre)), Some((b, b_pre))) => a > b || (a == b && b_pre && !a_pre),
        _ => false,
    }
}

/// Последний релиз на GitHub, если он новее этой версии Axel'.
pub async fn latest() -> Result<Option<Release>> {
    #[derive(serde::Deserialize)]
    struct Gh {
        tag_name: String,
        html_url: String,
        #[serde(default)]
        body: Option<String>,
    }
    let text =
        crate::net::get_text("https://api.github.com/repos/Hewako/quarry-launcher/releases/latest")
            .await?;
    let r: Gh =
        serde_json::from_str(&text).map_err(|e| crate::error::Error::parse("ответ GitHub", e))?;
    // Ссылку открываем только на страницу релизов этого проекта.
    if !is_newer(&r.tag_name, crate::launch::LAUNCHER_VERSION)
        || !r.html_url.starts_with(RELEASES_URL)
    {
        return Ok(None);
    }
    Ok(Some(Release {
        version: r.tag_name.trim_start_matches('v').to_string(),
        url: r.html_url,
        notes: r.body.unwrap_or_default().chars().take(2000).collect(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_comparison() {
        assert!(is_newer("v0.2.0", "0.1.0"));
        assert!(is_newer("0.10.0", "0.9.9"), "числа, а не строки");
        assert!(is_newer("1.0.0", "1.0.0-beta.2"));
        assert!(!is_newer("v0.1.0", "0.1.0"));
        assert!(!is_newer("0.1.0-rc.1", "0.1.0"));
        assert!(!is_newer("мусор", "0.1.0"));
    }
}
