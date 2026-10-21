//! Полуавтоматический поиск виновника краша: выключаем половину подозреваемых модов
//! (вместе с модами, которым они нужны), игрок запускает игру и отвечает, повторился ли краш.

use super::ModFile;
use super::meta::DepKind;
use crate::error::{Error, IoContext, Result};
use crate::instance::InstanceStore;
use crate::paths::write_atomic;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Bisect {
    /// Файлы, среди которых ещё может быть виновник.
    pub suspects: Vec<String>,
    /// Что выключено в этом раунде (вернётся при следующем ответе).
    pub disabled: Vec<String>,
    /// Проверяемая половина подозреваемых.
    pub half: Vec<String>,
    pub round: u32,
    pub snapshot: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Step {
    /// Следующий раунд: запустить игру и ответить.
    Next { state: Bisect },
    /// Виновник найден; все моды снова включены.
    Found { file: String },
}

const FILE: &str = "bisect.toml";

fn state_path(store: &InstanceStore, id: &str) -> Result<PathBuf> {
    Ok(store.dir(id)?.join(FILE))
}

pub fn load(store: &InstanceStore, id: &str) -> Result<Option<Bisect>> {
    let p = state_path(store, id)?;
    match std::fs::read_to_string(&p) {
        Ok(t) => toml::from_str(&t)
            .map(Some)
            .map_err(|e| Error::parse(FILE, e)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(Error::io(&p, e)),
    }
}

fn save(store: &InstanceStore, id: &str, b: &Bisect) -> Result<()> {
    let text = toml::to_string_pretty(b).map_err(|e| Error::parse(FILE, e))?;
    write_atomic(&state_path(store, id)?, text.as_bytes())
}

/// Набор файлов вместе со всеми включёнными модами, которым они (через цепочку) нужны.
fn with_dependents(files: &[ModFile], set: &[String]) -> Vec<String> {
    let provides = |f: &ModFile| -> Vec<String> {
        f.mods
            .iter()
            .flat_map(|m| std::iter::once(m.id.clone()).chain(m.provides.clone()))
            .collect()
    };
    let mut out: Vec<String> = set.to_vec();
    loop {
        let ids: Vec<String> = files
            .iter()
            .filter(|f| out.contains(&f.file_name))
            .flat_map(provides)
            .collect();
        let more: Vec<String> = files
            .iter()
            .filter(|f| f.enabled && !out.contains(&f.file_name))
            .filter(|f| {
                f.mods
                    .iter()
                    .flat_map(|m| &m.dependencies)
                    .any(|d| d.kind == DepKind::Required && ids.contains(&d.id))
            })
            .map(|f| f.file_name.clone())
            .collect();
        if more.is_empty() {
            return out;
        }
        out.extend(more);
    }
}

fn set_all(store: &InstanceStore, id: &str, files: &[String], enabled: bool) -> Result<()> {
    let game = store.game_dir(id)?;
    for f in files {
        // Игрок мог сам переключить мод между раундами — это не ошибка.
        let _ = super::set_enabled(&game, f, enabled);
    }
    Ok(())
}

fn next_round(store: &InstanceStore, id: &str, mut b: Bisect) -> Result<Bisect> {
    let files = super::inventory(&store.game_dir(id)?, &store.dir(id)?)?;
    b.half = b.suspects[..b.suspects.len().div_ceil(2)].to_vec();
    b.disabled = with_dependents(&files, &b.half)
        .into_iter()
        .filter(|f| files.iter().any(|m| &m.file_name == f && m.enabled))
        .collect();
    b.round += 1;
    set_all(store, id, &b.disabled, false)?;
    save(store, id, &b)?;
    Ok(b)
}

/// Начать поиск: снимок, затем первый раунд с выключенной половиной модов.
pub fn start(store: &InstanceStore, id: &str) -> Result<Bisect> {
    if load(store, id)?.is_some() {
        return Err(Error::invalid(
            "Поиск виновника уже идёт — ответьте на вопрос или прекратите его",
        ));
    }
    let game = store.game_dir(id)?;
    let dir = store.dir(id)?;
    let mut suspects: Vec<String> = super::inventory(&game, &dir)?
        .into_iter()
        .filter(|f| f.enabled)
        .map(|f| f.file_name)
        .collect();
    suspects.sort();
    if suspects.len() < 2 {
        return Err(Error::invalid(
            "Для поиска нужно хотя бы два включённых мода",
        ));
    }
    let snap = crate::snapshot::create(&dir, &game, "Перед поиском виновника краша", true)?;
    next_round(
        store,
        id,
        Bisect {
            suspects,
            disabled: Vec::new(),
            half: Vec::new(),
            round: 0,
            snapshot: snap.id,
        },
    )
}

/// Ответ игрока после запуска: `crashed` — краш повторился при выключенной половине.
pub fn answer(store: &InstanceStore, id: &str, crashed: bool) -> Result<Step> {
    let mut b = load(store, id)?.ok_or_else(|| Error::invalid("Поиск виновника не идёт"))?;
    set_all(store, id, &b.disabled, true)?;
    b.suspects = if crashed {
        // Виновник среди того, что оставалось включённым.
        b.suspects
            .iter()
            .filter(|f| !b.disabled.contains(f))
            .cloned()
            .collect()
    } else {
        b.suspects
            .iter()
            .filter(|f| b.disabled.contains(f))
            .cloned()
            .collect()
    };
    if b.suspects.len() > 1 {
        return Ok(Step::Next {
            state: next_round(store, id, b)?,
        });
    }
    let p = state_path(store, id)?;
    std::fs::remove_file(&p).at(&p)?;
    match b.suspects.pop() {
        Some(file) => Ok(Step::Found { file }),
        None => Err(Error::invalid(
            "Ответы противоречат друг другу: похоже, краш не связан с одним модом. Все моды снова включены",
        )),
    }
}

/// Прекратить поиск: включить всё, что выключил поиск.
pub fn cancel(store: &InstanceStore, id: &str) -> Result<()> {
    if let Some(b) = load(store, id)? {
        set_all(store, id, &b.disabled, true)?;
        let p = state_path(store, id)?;
        std::fs::remove_file(&p).at(&p)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loader::LoaderKind;
    use crate::mods::meta::tests::make_jar;
    use crate::paths::DataDir;

    fn fabric(id: &str, depends: &[&str]) -> String {
        let deps: Vec<String> = depends.iter().map(|d| format!("\"{d}\":\"*\"")).collect();
        format!(
            r#"{{"schemaVersion":1,"id":"{id}","version":"1.0","depends":{{{}}}}}"#,
            deps.join(",")
        )
    }

    fn profile(
        name: &str,
        mods: &[(&str, &[&str])],
    ) -> (InstanceStore, String, std::path::PathBuf) {
        let root =
            std::env::temp_dir().join(format!("quarry-bisect-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let store = InstanceStore::new(DataDir::open(&root).unwrap());
        let inst = store
            .create("B", "26.3", LoaderKind::Fabric, Some("0.19".into()))
            .unwrap();
        let dir = store.game_dir(&inst.id).unwrap().join("mods");
        std::fs::create_dir_all(&dir).unwrap();
        for (id, deps) in mods {
            make_jar(
                &dir.join(format!("{id}.jar")),
                &[("fabric.mod.json", fabric(id, deps).as_bytes())],
            );
        }
        (store, inst.id, root)
    }

    fn enabled(store: &InstanceStore, id: &str) -> Vec<String> {
        let dir = store.game_dir(id).unwrap().join("mods");
        let mut v: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".jar"))
            .collect();
        v.sort();
        v
    }

    #[test]
    fn finds_the_culprit_and_restores_everything() {
        let (store, id, root) = profile("find", &[("a", &[]), ("b", &[]), ("c", &[]), ("d", &[])]);
        let culprit = "c.jar";
        let mut state = start(&store, &id).unwrap();
        assert_eq!(state.suspects.len(), 4);
        let found = loop {
            // Игра падает, пока виновник включён.
            let crashed = enabled(&store, &id).iter().any(|f| f == culprit);
            match answer(&store, &id, crashed).unwrap() {
                Step::Next { state: s } => {
                    assert!(s.round > state.round);
                    state = s;
                }
                Step::Found { file } => break file,
            }
        };
        assert_eq!(found, culprit);
        assert_eq!(enabled(&store, &id).len(), 4, "все моды снова включены");
        assert!(load(&store, &id).unwrap().is_none());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn dependents_are_disabled_together() {
        // lib нужен b: если выключается lib, выключается и b — иначе игра упадёт от нехватки зависимости.
        let (store, id, root) = profile(
            "deps",
            &[("lib", &[]), ("b", &["lib"]), ("x", &[]), ("y", &[])],
        );
        let s = start(&store, &id).unwrap();
        if s.half.iter().any(|f| f == "lib.jar") {
            assert!(s.disabled.contains(&"b.jar".to_string()), "{s:?}");
        }
        cancel(&store, &id).unwrap();
        assert_eq!(enabled(&store, &id).len(), 4);
        assert!(
            start(&store, &id).is_ok() && start(&store, &id).is_err(),
            "второй поиск поверх первого нельзя"
        );
        std::fs::remove_dir_all(&root).unwrap();
    }
}
