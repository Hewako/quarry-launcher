//! Безопасная распаковка архивов (JDK, нативы, позже .mrpack): ни один файл не выходит
//! за целевую папку (zip-slip / path traversal).

use crate::error::{Error, IoContext, Result};
use crate::paths::safe_join;
use std::io::Read;
use std::path::Path;

/// Распаковать .tar.gz в `dest`. Символические ссылки допускаются только относительные
/// и только внутрь архива (в JDK для macOS такие есть).
pub fn extract_tar_gz(archive: &Path, dest: &Path) -> Result<()> {
    std::fs::create_dir_all(dest).at(dest)?;
    let file = std::fs::File::open(archive).at(archive)?;
    let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(file));
    tar.set_preserve_permissions(true);
    let entries = tar.entries().at(archive)?;
    for entry in entries {
        let mut entry = entry.at(archive)?;
        let path = entry.path().at(archive)?.to_string_lossy().into_owned();
        let target = safe_join(dest, &path)
            .ok_or_else(|| Error::invalid(format!("Архив содержит недопустимый путь: {path}")))?;
        if entry.header().entry_type().is_symlink() || entry.header().entry_type().is_hard_link() {
            let link = entry
                .link_name()
                .at(archive)?
                .map(|l| l.to_string_lossy().into_owned())
                .unwrap_or_default();
            let base = target.parent().unwrap_or(dest);
            if !link_stays_inside(dest, base, &link) {
                return Err(Error::invalid(format!(
                    "Архив содержит ссылку за пределы папки: {path} → {link}"
                )));
            }
        }
        // unpack_in дополнительно проверяет, что родительская папка (с учётом уже
        // распакованных ссылок) находится внутри dest.
        if !entry.unpack_in(dest).at(&target)? {
            return Err(Error::invalid(format!(
                "Архив содержит недопустимый путь: {path}"
            )));
        }
    }
    Ok(())
}

/// Распаковать zip в `dest`. Ссылки в zip не поддерживаются и пропускаются.
pub fn extract_zip(archive: &Path, dest: &Path) -> Result<()> {
    extract_zip_filtered(archive, dest, |_| true, false)
}

/// Распаковать из zip только подходящие файлы. `flatten` — класть файлы без подпапок
/// (так раскладываются нативы старых версий).
pub fn extract_zip_filtered(
    archive: &Path,
    dest: &Path,
    keep: impl Fn(&str) -> bool,
    flatten: bool,
) -> Result<()> {
    std::fs::create_dir_all(dest).at(dest)?;
    let file = std::fs::File::open(archive).at(archive)?;
    let mut zip =
        zip::ZipArchive::new(file).map_err(|e| Error::parse(archive.display().to_string(), e))?;
    for i in 0..zip.len() {
        let mut entry = zip
            .by_index(i)
            .map_err(|e| Error::parse(archive.display().to_string(), e))?;
        let name = entry.name().unwrap_or_default().to_string();
        if entry.is_dir() || entry.is_symlink() || !keep(&name) {
            continue;
        }
        let rel = if flatten {
            Path::new(&name)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default()
        } else {
            name.clone()
        };
        let target = safe_join(dest, &rel)
            .ok_or_else(|| Error::invalid(format!("Архив содержит недопустимый путь: {name}")))?;
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).at(parent)?;
        }
        let mut buf = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut buf).at(archive)?;
        std::fs::write(&target, buf).at(&target)?;
        #[cfg(unix)]
        if let Some(mode) = entry.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            let _ =
                std::fs::set_permissions(&target, std::fs::Permissions::from_mode(mode & 0o755));
        }
    }
    Ok(())
}

/// Куда ведёт относительная ссылка из папки `base` — остаётся ли внутри `root`.
fn link_stays_inside(root: &Path, base: &Path, link: &str) -> bool {
    use std::path::Component;
    if link.is_empty() || Path::new(link).is_absolute() || link.contains('\\') {
        return false;
    }
    let Ok(rel_base) = base.strip_prefix(root) else {
        return false;
    };
    let mut depth: i64 = rel_base.components().count() as i64;
    for comp in Path::new(link).components() {
        match comp {
            Component::Normal(_) => depth += 1,
            Component::ParentDir => {
                depth -= 1;
                if depth < 0 {
                    return false;
                }
            }
            Component::CurDir => {}
            _ => return false,
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn tmpdir(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("quarry-archive-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn links_inside_and_outside() {
        let root = Path::new("/r");
        assert!(link_stays_inside(
            root,
            Path::new("/r/jdk/Contents/MacOS"),
            "../Home/lib/libjli.dylib"
        ));
        assert!(!link_stays_inside(
            root,
            Path::new("/r/a"),
            "../../etc/passwd"
        ));
        assert!(!link_stays_inside(root, Path::new("/r/a"), "/etc/passwd"));
    }

    #[test]
    fn zip_slip_is_rejected() {
        let d = tmpdir("zipslip");
        let zpath = d.join("evil.zip");
        {
            let f = std::fs::File::create(&zpath).unwrap();
            let mut w = zip::ZipWriter::new(f);
            let opts: zip::write::SimpleFileOptions = Default::default();
            w.start_file("ok/file.txt", opts).unwrap();
            w.write_all(b"ok").unwrap();
            w.start_file("../escaped.txt", opts).unwrap();
            w.write_all(b"bad").unwrap();
            w.finish().unwrap();
        }
        let out = d.join("out");
        assert!(extract_zip(&zpath, &out).is_err());
        assert!(!d.join("escaped.txt").exists());
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn zip_filtered_and_flattened() {
        let d = tmpdir("flat");
        let zpath = d.join("natives.jar");
        {
            let f = std::fs::File::create(&zpath).unwrap();
            let mut w = zip::ZipWriter::new(f);
            let opts: zip::write::SimpleFileOptions = Default::default();
            w.start_file("META-INF/MANIFEST.MF", opts).unwrap();
            w.write_all(b"m").unwrap();
            w.start_file("macos/arm64/liblwjgl.dylib", opts).unwrap();
            w.write_all(b"lib").unwrap();
            w.finish().unwrap();
        }
        let out = d.join("out");
        extract_zip_filtered(&zpath, &out, |n| !n.starts_with("META-INF/"), true).unwrap();
        assert!(out.join("liblwjgl.dylib").exists());
        assert!(!out.join("MANIFEST.MF").exists());
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn tar_gz_roundtrip_and_traversal() {
        let d = tmpdir("tar");
        let good = d.join("good.tar.gz");
        {
            let f = std::fs::File::create(&good).unwrap();
            let gz = flate2::write::GzEncoder::new(f, flate2::Compression::fast());
            let mut b = tar::Builder::new(gz);
            let mut h = tar::Header::new_gnu();
            h.set_size(2);
            h.set_mode(0o755);
            h.set_cksum();
            b.append_data(&mut h, "jdk-21/bin/java", &b"#!"[..])
                .unwrap();
            let mut l = tar::Header::new_gnu();
            l.set_entry_type(tar::EntryType::Symlink);
            l.set_size(0);
            l.set_cksum();
            b.append_link(&mut l, "jdk-21/link", "bin/java").unwrap();
            b.into_inner().unwrap().finish().unwrap();
        }
        let out = d.join("out");
        extract_tar_gz(&good, &out).unwrap();
        assert!(out.join("jdk-21/bin/java").exists());

        let evil = d.join("evil.tar.gz");
        {
            let f = std::fs::File::create(&evil).unwrap();
            let gz = flate2::write::GzEncoder::new(f, flate2::Compression::fast());
            let mut b = tar::Builder::new(gz);
            let mut l = tar::Header::new_gnu();
            l.set_entry_type(tar::EntryType::Symlink);
            l.set_size(0);
            l.set_cksum();
            b.append_link(&mut l, "jdk/escape", "../../outside")
                .unwrap();
            b.into_inner().unwrap().finish().unwrap();
        }
        assert!(extract_tar_gz(&evil, &d.join("out2")).is_err());
        std::fs::remove_dir_all(&d).unwrap();
    }
}
