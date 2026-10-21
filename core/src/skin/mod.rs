//! Скины: получение текущего скина и аватар (лицо + слой шляпы). Обработка PNG локальная.
//! Редактор скинов (Skin Studio) отложен (разбор игрока: P3).

use crate::error::{Error, Result};
use crate::paths::{DataDir, write_atomic};
use image::{DynamicImage, GenericImageView, ImageFormat};
use std::path::PathBuf;

/// Проверить скин: PNG 64×64 или 64×32 (старый формат).
pub fn validate_skin(bytes: &[u8]) -> Result<DynamicImage> {
    let img = image::load_from_memory_with_format(bytes, ImageFormat::Png)
        .map_err(|_| Error::invalid("Файл скина — не PNG"))?;
    match img.dimensions() {
        (64, 64) | (64, 32) => Ok(img),
        (w, h) => Err(Error::invalid(format!(
            "Неверный размер скина {w}×{h}: нужен 64×64 или 64×32"
        ))),
    }
}

/// Аватар 64×64: лицо (8,8) с наложенным слоем шляпы (40,8), увеличение без сглаживания.
pub fn avatar_png(skin: &[u8]) -> Result<Vec<u8>> {
    let img = validate_skin(skin)?;
    let mut face = img.crop_imm(8, 8, 8, 8).to_rgba8();
    let hat = img.crop_imm(40, 8, 8, 8).to_rgba8();
    image::imageops::overlay(&mut face, &hat, 0, 0);
    let avatar = image::imageops::resize(&face, 64, 64, image::imageops::FilterType::Nearest);
    let mut out = std::io::Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(avatar)
        .write_to(&mut out, ImageFormat::Png)
        .map_err(|e| Error::invalid(format!("Не удалось сохранить аватар: {e}")))?;
    Ok(out.into_inner())
}

fn safe_name(nick: &str) -> String {
    nick.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .take(32)
        .collect()
}

/// Скачать скин по URL (из профиля Minecraft или системы скинов Ely.by) и сохранить
/// скин и аватар в `cache/skins/<провайдер>/`.
pub async fn cache_skin(
    data: &DataDir,
    provider: &str,
    nick: &str,
    url: &str,
) -> Result<(PathBuf, PathBuf)> {
    let bytes = crate::net::get_bytes(url).await?;
    validate_skin(&bytes)?;
    let dir = data.skins().join(safe_name(provider));
    let skin_path = dir.join(format!("{}.png", safe_name(nick)));
    let avatar_path = dir.join(format!("{}_avatar.png", safe_name(nick)));
    write_atomic(&skin_path, &bytes)?;
    write_atomic(&avatar_path, &avatar_png(&bytes)?)?;
    Ok((skin_path, avatar_path))
}

/// Скин Ely.by по нику. В документации адрес указан как http://skinsystem.ely.by; лаунчер
/// ходит только по HTTPS — работает ли HTTPS, не подтверждено первоисточником.
pub fn ely_skin_url(nick: &str) -> String {
    format!(
        "https://skinsystem.ely.by/skins/{}.png",
        urlencoding::encode(nick)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(w: u32, h: u32) -> Vec<u8> {
        let mut img = image::RgbaImage::new(w, h);
        img.put_pixel(8, 8, image::Rgba([255, 0, 0, 255]));
        img.put_pixel(40, 8, image::Rgba([0, 0, 255, 255]));
        let mut out = std::io::Cursor::new(Vec::new());
        DynamicImage::ImageRgba8(img)
            .write_to(&mut out, ImageFormat::Png)
            .unwrap();
        out.into_inner()
    }

    #[test]
    fn sizes_are_validated() {
        assert!(validate_skin(&png(64, 64)).is_ok());
        assert!(validate_skin(&png(64, 32)).is_ok());
        assert!(validate_skin(&png(128, 128)).is_err());
        assert!(validate_skin(b"not a png").is_err());
    }

    #[test]
    fn avatar_overlays_hat() {
        let a = image::load_from_memory(&avatar_png(&png(64, 64)).unwrap()).unwrap();
        assert_eq!(a.dimensions(), (64, 64));
        // Верхний левый пиксель лица перекрыт непрозрачным пикселем шляпы.
        assert_eq!(a.get_pixel(0, 0).0, [0, 0, 255, 255]);
    }

    #[test]
    fn nick_is_file_safe() {
        assert_eq!(safe_name("../evil"), "___evil");
        assert!(ely_skin_url("a b").ends_with("a%20b.png"));
    }
}
