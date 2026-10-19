use std::path::Path;
use sha1::{Sha1, Digest};
use tokio::io::AsyncWriteExt;

// Проверяем, что файл существует и его SHA-1 совпадает с ожидаемым
pub async fn verify_sha1(path: &Path, expected: &str) -> Result<bool, Box<dyn std::error::Error>> {
    if !path.exists() {
        return Ok(false);
    }
    let data = tokio::fs::read(path).await?;
    let mut hasher = Sha1::new();
    hasher.update(&data);
    let hex = hex::encode(hasher.finalize());
    Ok(hex.eq_ignore_ascii_case(expected))
}

// Скачиваем файл во временный, проверяем SHA-1, затем атомарно заменяем целевой
pub async fn download_file(
    url: &str,
    dest: &Path,
    expected_sha1: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    if verify_sha1(dest, expected_sha1).await? {
        return Ok(());
    }

    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    let client = reqwest::Client::builder()
        .user_agent(crate::USER_AGENT)
        .build()?;

    let resp = client.get(url).send().await?;
    if !resp.status().is_success() {
        return Err(format!("Ошибка HTTP: {}", resp.status()).into());
    }

    let bytes = resp.bytes().await?;

    let mut hasher = Sha1::new();
    hasher.update(&bytes);
    let hex = hex::encode(hasher.finalize());
    if !hex.eq_ignore_ascii_case(expected_sha1) {
        return Err(format!(
            "Несовпадение SHA-1: ожидался {}, получен {}",
            expected_sha1, hex
        )
        .into());
    }

    let tmp = dest.with_extension("tmp");
    let mut file = tokio::fs::File::create(&tmp).await?;
    file.write_all(&bytes).await?;
    file.flush().await?;
    tokio::fs::rename(&tmp, dest).await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_verify_sha1() {
        let dir = std::env::temp_dir().join("quarry_test_verify_sha1");
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let path = dir.join("test.txt");
        tokio::fs::write(&path, b"hello").await.unwrap();

        let ok = verify_sha1(&path, "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
            .await
            .unwrap();
        assert!(ok, "Хэш должен совпасть");

        let bad = verify_sha1(&path, "0000000000000000000000000000000000000000")
            .await
            .unwrap();
        assert!(!bad, "Неверный хэш не должен совпадать");

        tokio::fs::remove_dir_all(&dir).await.unwrap();
    }

    #[tokio::test]
    #[ignore]
    async fn test_download_client_jar() {
        let manifest = crate::metadata::fetch_manifest().await.unwrap();
        let entry = manifest
            .versions
            .iter()
            .find(|v| v.id == manifest.latest.release)
            .unwrap();
        let details = crate::metadata::fetch_version_details(&entry.url)
            .await
            .unwrap();

        let dir = std::env::temp_dir().join("quarry_test_download_client");
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let dest = dir.join("client.jar");
        let _ = tokio::fs::remove_file(&dest).await;

        download_file(
            &details.downloads.client.url,
            &dest,
            &details.downloads.client.sha1,
        )
        .await
        .unwrap();

        assert!(dest.exists(), "Файл должен существовать после скачивания");
        let ok = verify_sha1(&dest, &details.downloads.client.sha1)
            .await
            .unwrap();
        assert!(ok, "SHA-1 скачанного файла должен совпасть");

        tokio::fs::remove_dir_all(&dir).await.unwrap();
    }
}

// Скачиваем файл без проверки хэша. Нужно для источников, которые не публикуют SHA-1
// (например, Adoptium отдаёт только SHA-256). Целостность проверяется отдельно —
// запуском скачанного бинарника и разбором его версии.
pub async fn download_raw(url: &str, dest: &Path) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    let client = reqwest::Client::builder()
        .user_agent(crate::USER_AGENT)
        .build()?;

    let resp = client.get(url).send().await?;
    if !resp.status().is_success() {
        return Err(format!("Ошибка HTTP: {}", resp.status()).into());
    }

    let bytes = resp.bytes().await?;

    let tmp = dest.with_extension("tmp");
    let mut file = tokio::fs::File::create(&tmp).await?;
    file.write_all(&bytes).await?;
    file.flush().await?;
    tokio::fs::rename(&tmp, dest).await?;

    Ok(())
}
