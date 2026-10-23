//! Сравнение двух профилей: версия и загрузчик, включённые моды (по ID мода, а не по имени файла),
//! настройки запуска и options.txt.

use crate::error::Result;
use crate::instance::InstanceStore;
use crate::mods::{self, ModFile};
use serde::Serialize;
use std::collections::BTreeMap;

/// Строка различия: что, значение в первом профиле, значение во втором ("" — нет).
pub type Row = (String, String, String);

#[derive(Debug, Default, Serialize, PartialEq)]
pub struct ProfileDiff {
    pub basics: Vec<Row>,
    /// Моды только в первом / только во втором: «Название версия».
    pub only_a: Vec<String>,
    pub only_b: Vec<String>,
    /// Есть в обоих, но версии разные.
    pub versions: Vec<Row>,
    /// Настройки запуска профиля (ключи LaunchOverrides).
    pub settings: Vec<Row>,
    pub options: Vec<Row>,
}

/// Все ключи, где значения различаются; отсутствующее значение — пустая строка.
pub fn diff_maps(a: &BTreeMap<String, String>, b: &BTreeMap<String, String>) -> Vec<Row> {
    let mut keys: Vec<&String> = a.keys().chain(b.keys()).collect();
    keys.sort();
    keys.dedup();
    keys.into_iter()
        .filter_map(|k| {
            let (va, vb) = (
                a.get(k).cloned().unwrap_or_default(),
                b.get(k).cloned().unwrap_or_default(),
            );
            (va != vb).then(|| (k.clone(), va, vb))
        })
        .collect()
}

/// Включённые моды: ID → (название, версия). Файл без метаданных сравнивается по имени.
fn mod_map(files: &[ModFile]) -> BTreeMap<String, (String, String)> {
    files
        .iter()
        .filter(|f| f.enabled)
        .map(|f| match f.primary() {
            Some(m) => (m.id.clone(), (m.name.clone(), m.version.clone())),
            None => (f.file_name.clone(), (f.display_name(), String::new())),
        })
        .collect()
}

pub fn diff_mods(a: &[ModFile], b: &[ModFile]) -> (Vec<String>, Vec<String>, Vec<Row>) {
    let (ma, mb) = (mod_map(a), mod_map(b));
    let label = |(n, v): &(String, String)| {
        if v.is_empty() {
            n.clone()
        } else {
            format!("{n} {v}")
        }
    };
    let only = |x: &BTreeMap<String, (String, String)>, y: &BTreeMap<String, (String, String)>| {
        x.iter()
            .filter(|(k, _)| !y.contains_key(*k))
            .map(|(_, v)| label(v))
            .collect::<Vec<_>>()
    };
    let versions = ma
        .iter()
        .filter_map(|(k, (n, va))| {
            mb.get(k)
                .filter(|(_, vb)| vb != va)
                .map(|(_, vb)| (n.clone(), va.clone(), vb.clone()))
        })
        .collect();
    (only(&ma, &mb), only(&mb, &ma), versions)
}

/// Настройки запуска как «ключ → значение» (JSON значения, без пустых).
fn settings_map(s: &crate::settings::LaunchOverrides) -> BTreeMap<String, String> {
    match serde_json::to_value(s) {
        Ok(serde_json::Value::Object(o)) => {
            o.into_iter().map(|(k, v)| (k, v.to_string())).collect()
        }
        _ => BTreeMap::new(),
    }
}

pub fn compare(store: &InstanceStore, a: &str, b: &str) -> Result<ProfileDiff> {
    let (ia, ib) = (store.get(a)?, store.get(b)?);
    let (ga, gb) = (store.game_dir(a)?, store.game_dir(b)?);
    let loader = |i: &crate::instance::Instance| {
        format!(
            "{} {}",
            i.loader.display(),
            i.loader_version.clone().unwrap_or_default()
        )
        .trim()
        .to_string()
    };
    let basics = diff_maps(
        &BTreeMap::from([
            ("Minecraft".to_string(), ia.game_version.clone()),
            ("Загрузчик".to_string(), loader(&ia)),
        ]),
        &BTreeMap::from([
            ("Minecraft".to_string(), ib.game_version.clone()),
            ("Загрузчик".to_string(), loader(&ib)),
        ]),
    );
    let (only_a, only_b, versions) = diff_mods(
        &mods::inventory(&ga, &store.dir(a)?)?,
        &mods::inventory(&gb, &store.dir(b)?)?,
    );
    Ok(ProfileDiff {
        basics,
        only_a,
        only_b,
        versions,
        settings: diff_maps(&settings_map(&ia.settings), &settings_map(&ib.settings)),
        options: diff_maps(
            &crate::snapshot::options_map(&ga.join("options.txt")),
            &crate::snapshot::options_map(&gb.join("options.txt")),
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mods::meta::{ModLoader, ModMeta, Side};

    fn file(name: &str, id: &str, version: &str, enabled: bool) -> ModFile {
        ModFile {
            world_content: false,
            file_name: name.into(),
            rel_path: format!("mods/{name}"),
            enabled,
            size: 1,
            sha1: "x".into(),
            mods: vec![ModMeta {
                loader: ModLoader::Fabric,
                id: id.into(),
                name: id.into(),
                version: version.into(),
                side: Side::Both,
                dependencies: vec![],
                provides: vec![],
                mixins: vec![],
                nested: vec![],
            }],
            read_error: None,
            source: None,
        }
    }

    #[test]
    fn mods_are_compared_by_id_not_file_name() {
        let a = [
            file("sodium-0.6.jar", "sodium", "0.6", true),
            file("iris.jar", "iris", "1.8", true),
            file("off.jar", "off", "1", false),
        ];
        let b = [
            file("sodium-renamed.jar", "sodium", "0.7", true),
            file("lithium.jar", "lithium", "0.15", true),
        ];
        let (only_a, only_b, versions) = diff_mods(&a, &b);
        assert_eq!(only_a, vec!["iris 1.8"]);
        assert_eq!(only_b, vec!["lithium 0.15"]);
        assert_eq!(
            versions,
            vec![("sodium".into(), "0.6".into(), "0.7".into())]
        );
    }

    #[test]
    fn maps_report_both_sides_and_missing_keys() {
        let a = BTreeMap::from([
            ("fov".to_string(), "70".to_string()),
            ("gamma".to_string(), "1".to_string()),
        ]);
        let b = BTreeMap::from([
            ("fov".to_string(), "90".to_string()),
            ("lang".to_string(), "ru_ru".to_string()),
        ]);
        assert_eq!(
            diff_maps(&a, &b),
            vec![
                ("fov".into(), "70".into(), "90".into()),
                ("gamma".into(), "1".into(), "".into()),
                ("lang".into(), "".into(), "ru_ru".into()),
            ]
        );
    }
}
