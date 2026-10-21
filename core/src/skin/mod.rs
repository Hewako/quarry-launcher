//! Скины: получение текущего скина и аватар (лицо + слой шляпы). Обработка PNG локальная.
//! Редактор скинов (Skin Studio) отложен (разбор игрока: P3); на старте — только просмотр.

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

/// Фигура скина спереди и сзади (2D, с внешним слоем), увеличенная в 8 раз.
/// У старых скинов 64×32 левые рука и нога — зеркальные правые (так рисует игра).
pub fn figure_png(skin: &[u8]) -> Result<Vec<u8>> {
    use image::imageops::{flip_horizontal, overlay};
    let img = validate_skin(skin)?.to_rgba8();
    let legacy = img.height() == 32;
    let part =
        |x: u32, y: u32, w: u32, h: u32| image::imageops::crop_imm(&img, x, y, w, h).to_image();
    let mut canvas = image::RgbaImage::new(36, 32);
    // (основной слой, внешний слой, левый-зеркальный для 64×32, куда положить)
    type Part = (
        (u32, u32, u32, u32),
        Option<(u32, u32)>,
        Option<(u32, u32)>,
        (i64, i64),
    );
    let front: [Part; 6] = [
        ((8, 8, 8, 8), Some((40, 8)), None, (4, 0)),
        ((20, 20, 8, 12), Some((20, 36)), None, (4, 8)),
        ((44, 20, 4, 12), Some((44, 36)), None, (0, 8)),
        ((36, 52, 4, 12), Some((52, 52)), Some((44, 20)), (12, 8)),
        ((4, 20, 4, 12), Some((4, 36)), None, (4, 20)),
        ((20, 52, 4, 12), Some((4, 52)), Some((4, 20)), (8, 20)),
    ];
    let back: [Part; 6] = [
        ((24, 8, 8, 8), Some((56, 8)), None, (4, 0)),
        ((32, 20, 8, 12), Some((32, 36)), None, (4, 8)),
        ((52, 20, 4, 12), Some((52, 36)), None, (12, 8)),
        ((44, 52, 4, 12), Some((60, 52)), Some((52, 20)), (0, 8)),
        ((12, 20, 4, 12), Some((12, 36)), None, (8, 20)),
        ((28, 52, 4, 12), Some((12, 52)), Some((12, 20)), (4, 20)),
    ];
    for (parts, dx) in [(&front, 0i64), (&back, 20)] {
        for &((x, y, w, h), over, mirror, (px, py)) in parts.iter() {
            let base = match (legacy, mirror) {
                (true, Some((mx, my))) => flip_horizontal(&part(mx, my, w, h)),
                _ => part(x, y, w, h),
            };
            overlay(&mut canvas, &base, px + dx, py);
            // У 64×32 внешний слой есть только у головы.
            if let Some((ox, oy)) = over
                && (!legacy || oy < 16)
            {
                overlay(&mut canvas, &part(ox, oy, w, h), px + dx, py);
            }
        }
    }
    let big = image::imageops::resize(
        &canvas,
        36 * 8,
        32 * 8,
        image::imageops::FilterType::Nearest,
    );
    let mut out = std::io::Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(big)
        .write_to(&mut out, ImageFormat::Png)
        .map_err(|e| Error::invalid(format!("Не удалось нарисовать скин: {e}")))?;
    Ok(out.into_inner())
}

/// Фигура текущего скина аккаунта как data-URL (скин берётся из кеша или скачивается).
pub async fn figure_data_url(
    data: &DataDir,
    account: &crate::account::AccountRef,
) -> Option<String> {
    use base64::Engine as _;
    let provider = account.provider.key();
    let path = data
        .skins()
        .join(safe_name(provider))
        .join(format!("{}.png", safe_name(&account.name)));
    let skin = match std::fs::read(&path) {
        Ok(b) => b,
        Err(_) => {
            let (p, _) = cache_skin(data, provider, &account.name, account.skin_url.as_deref()?)
                .await
                .ok()?;
            std::fs::read(p).ok()?
        }
    };
    let png = figure_png(&skin).ok()?;
    Some(format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(png)
    ))
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

/// Аватар аккаунта как data-URL для интерфейса: из кеша, иначе скачать текущий скин.
/// `None` — скина нет (стандартный Steve/Alex) или он недоступен без сети.
pub async fn avatar_data_url(
    data: &DataDir,
    account: &crate::account::AccountRef,
) -> Option<String> {
    use base64::Engine as _;
    let provider = account.provider.key();
    let path = data
        .skins()
        .join(safe_name(provider))
        .join(format!("{}_avatar.png", safe_name(&account.name)));
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(_) => {
            let url = account.skin_url.as_deref()?;
            let (_, avatar) = cache_skin(data, provider, &account.name, url).await.ok()?;
            std::fs::read(avatar).ok()?
        }
    };
    Some(format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    ))
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

    #[test]
    fn figure_front_and_back() {
        let mut img = image::RgbaImage::new(64, 64);
        img.put_pixel(8, 8, image::Rgba([255, 0, 0, 255])); // лицо, верхний левый пиксель
        img.put_pixel(24, 8, image::Rgba([0, 0, 255, 255])); // затылок
        let mut png = std::io::Cursor::new(Vec::new());
        DynamicImage::ImageRgba8(img)
            .write_to(&mut png, ImageFormat::Png)
            .unwrap();
        let fig = image::load_from_memory(&figure_png(&png.into_inner()).unwrap())
            .unwrap()
            .to_rgba8();
        assert_eq!(fig.dimensions(), (36 * 8, 32 * 8));
        assert_eq!(fig.get_pixel(4 * 8, 0).0, [255, 0, 0, 255]);
        assert_eq!(fig.get_pixel((20 + 4) * 8, 0).0, [0, 0, 255, 255]);
    }
}
