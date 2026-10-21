use anyhow::{Context, Result};
use image::{DynamicImage, GenericImageView, ImageFormat};
use std::path::PathBuf;

/// Скачивает скин Ely.by по нику и сохраняет в кеш.
/// Возвращает путь к скачанному файлу.
pub async fn download_ely_skin(username: &str) -> Result<PathBuf> {
    let url = format!("https://skinsystem.ely.by/skins/{}.png", username);
    let client = reqwest::Client::new();
    
    let resp = client.get(&url).send().await
        .context("Ошибка запроса к skinsystem.ely.by")?;
        
    if !resp.status().is_success() {
        anyhow::bail!("Скин для '{}' не найден (HTTP {})", username, resp.status());
    }
    
    let bytes = resp.bytes().await?;
    
    // Создаем папку для скинов в кеше
    let home = dirs::home_dir().context("Не удалось найти домашнюю папку")?;
    let cache_dir = home.join(".quarry").join("cache").join("skins");
    std::fs::create_dir_all(&cache_dir)?;
    
    let safe_username = username.chars().map(|c| if c.is_alphanumeric() { c } else { '_' }).collect::<String>();
    let path = cache_dir.join(format!("{}_skin.png", safe_username));
    
    tokio::fs::write(&path, &bytes).await?;
    Ok(path)
}

/// Вырезает голову (аватарку) из скина и сохраняет в кеш.
pub async fn download_ely_avatar(username: &str) -> Result<PathBuf> {
    let skin_path = download_ely_skin(username).await?;
    let skin_data = tokio::fs::read(&skin_path).await?;
    
    let skin = image::load_from_memory(&skin_data)
        .context("Не удалось распознать PNG скина")?;
        
    let (w, h) = skin.dimensions();
    if w < 64 || h < 32 {
        anyhow::bail!("Неверный размер скина: {}x{}", w, h);
    }

    // Вырезаем лицо 8x8 (позиция 8,8) и слой шляпы 8x8 (позиция 40,8)
    let face = skin.crop_imm(8, 8, 8, 8).to_rgba8();
    let hat = skin.crop_imm(40, 8, 8, 8).to_rgba8();
    
    // Накладываем шляпу на лицо
    let mut combined = face;
    image::imageops::overlay(&mut combined, &hat, 0, 0);
    
    // Увеличиваем до 64x64 (пиксельный стиль)
    let avatar = image::imageops::resize(&combined, 64, 64, image::imageops::FilterType::Nearest);
    
    let home = dirs::home_dir().context("Не удалось найти домашнюю папку")?;
    let cache_dir = home.join(".quarry").join("cache").join("skins");
    std::fs::create_dir_all(&cache_dir)?;
    
    let safe_username = username.chars().map(|c| if c.is_alphanumeric() { c } else { '_' }).collect::<String>();
    let path = cache_dir.join(format!("{}_avatar.png", safe_username));
    
    // Сохраняем
    let mut out = std::io::Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(avatar).write_to(&mut out, ImageFormat::Png)?;
    tokio::fs::write(&path, out.into_inner()).await?;
    
    Ok(path)
}
