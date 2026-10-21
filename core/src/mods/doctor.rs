//! Mod Doctor (базовый): проверка сборки до запуска. Только детерминированные правила по
//! метаданным модов; у каждого вывода есть основание. По разбору игрока: у мода три состояния
//! («конфликтов не найдено» / «есть предупреждение» / «блокирует запуск») плюс «выключен»
//! и «метаданные не прочитаны». Предупреждений по пересечению mixin-целей нет — они дают
//! ложные тревоги в больших сборках.

use super::ModFile;
use super::meta::{DepKind, ModLoader, ModMeta};
use super::range;
use crate::loader::LoaderKind;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModState {
    Ok,
    Warning,
    Blocked,
    Disabled,
    Unknown,
}

impl ModState {
    pub fn label(self) -> &'static str {
        match self {
            ModState::Ok => "конфликтов не найдено",
            ModState::Warning => "есть предупреждение",
            ModState::Blocked => "блокирует запуск",
            ModState::Disabled => "выключен",
            ModState::Unknown => "метаданные не прочитаны",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Blocking,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Fix {
    /// Поставить недостающий мод (через каталог).
    Install { mod_id: String },
    /// Выключить мод (файл остаётся в профиле).
    Disable { file_name: String },
    /// Выбрать, какой из дублей оставить.
    KeepOne { file_names: Vec<String> },
}

#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    pub severity: Severity,
    /// Короткий заголовок простыми словами.
    pub title: String,
    /// Что это значит для игрока.
    pub detail: String,
    /// Основание: откуда известно (поле метаданных, правило).
    pub evidence: String,
    pub files: Vec<String>,
    pub fixes: Vec<Fix>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModReport {
    pub file_name: String,
    pub state: ModState,
    pub state_label: &'static str,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub findings: Vec<Finding>,
    pub mods: Vec<ModReport>,
    pub total: usize,
    pub ok: usize,
    pub warnings: usize,
    pub blocked: usize,
    /// Если пусто — можно запускать; иначе запуск лучше не начинать.
    pub can_launch: bool,
}

fn builtin_ids(loader: LoaderKind) -> &'static [&'static str] {
    match loader {
        LoaderKind::Fabric => &[
            "minecraft",
            "java",
            "fabricloader",
            "fabric-loader",
            "mixinextras",
        ],
        LoaderKind::Quilt => &[
            "minecraft",
            "java",
            "quilt_loader",
            "fabricloader",
            "quilt_base",
            "mixinextras",
        ],
        LoaderKind::NeoForge => &[
            "minecraft",
            "java",
            "neoforge",
            "forge",
            "fml",
            "javafml",
            "mixinextras",
        ],
        LoaderKind::Forge => &["minecraft", "java", "forge", "fml", "javafml"],
        LoaderKind::Vanilla => &["minecraft", "java"],
    }
}

fn loader_accepts(profile: LoaderKind, m: ModLoader) -> bool {
    match profile {
        LoaderKind::Fabric => m == ModLoader::Fabric,
        LoaderKind::Quilt => matches!(m, ModLoader::Fabric | ModLoader::Quilt),
        // До 20.5 NeoForge читал mods.toml в формате Forge.
        LoaderKind::NeoForge => matches!(m, ModLoader::NeoForge | ModLoader::Forge),
        LoaderKind::Forge => m == ModLoader::Forge,
        LoaderKind::Vanilla => false,
    }
}

fn meta_file(m: &ModMeta) -> &'static str {
    match m.loader {
        ModLoader::Fabric => "fabric.mod.json",
        ModLoader::Quilt => "quilt.mod.json",
        ModLoader::NeoForge => "neoforge.mods.toml",
        ModLoader::Forge => "mods.toml",
    }
}

pub fn check(
    files: &[ModFile],
    loader: LoaderKind,
    game_version: &str,
    loader_version: Option<&str>,
) -> Report {
    let mut findings = Vec::new();
    let enabled: Vec<&ModFile> = files.iter().filter(|f| f.enabled).collect();

    // Кто что предоставляет (с учётом вложенных модов и provides).
    let mut present: BTreeMap<String, (String, String)> = BTreeMap::new(); // id -> (version, file)
    let mut by_id: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for f in &enabled {
        for m in &f.mods {
            by_id
                .entry(m.id.clone())
                .or_default()
                .push(f.file_name.clone());
            present.insert(m.id.clone(), (m.version.clone(), f.file_name.clone()));
            for p in &m.provides {
                present
                    .entry(p.clone())
                    .or_insert((m.version.clone(), f.file_name.clone()));
            }
            for n in &m.nested {
                present
                    .entry(n.id.clone())
                    .or_insert((n.version.clone(), f.file_name.clone()));
                for p in &n.provides {
                    present
                        .entry(p.clone())
                        .or_insert((n.version.clone(), f.file_name.clone()));
                }
            }
        }
    }
    let mut builtin: BTreeMap<&str, Option<&str>> = BTreeMap::new();
    for id in builtin_ids(loader) {
        builtin.insert(id, None);
    }
    builtin.insert("minecraft", Some(game_version));
    for id in ["fabricloader", "quilt_loader", "neoforge"] {
        if builtin.contains_key(id) {
            builtin.insert(id, loader_version);
        }
    }

    if loader == LoaderKind::Vanilla && !enabled.is_empty() {
        findings.push(Finding {
            severity: Severity::Warning,
            title: "В профиле без загрузчика лежат моды".into(),
            detail: "Vanilla не загружает моды: они просто не будут работать.".into(),
            evidence: "загрузчик профиля — Vanilla".into(),
            files: enabled.iter().map(|f| f.file_name.clone()).collect(),
            fixes: Vec::new(),
        });
    }

    // Дубли одного мода.
    for (id, list) in &by_id {
        if list.len() > 1 {
            findings.push(Finding {
                severity: Severity::Blocking,
                title: format!("Мод «{id}» установлен дважды"),
                detail:
                    "Загрузчик откажется запускать игру с двумя копиями одного мода. Оставьте одну."
                        .into(),
                evidence: format!(
                    "одинаковый id «{id}» в метаданных файлов: {}",
                    list.join(", ")
                ),
                files: list.clone(),
                fixes: vec![Fix::KeepOne {
                    file_names: list.clone(),
                }],
            });
        }
    }

    for f in &enabled {
        for m in &f.mods {
            if loader != LoaderKind::Vanilla && !loader_accepts(loader, m.loader) {
                findings.push(Finding {
                    severity: Severity::Blocking,
                    title: format!("{} — мод для другого загрузчика", m.name),
                    detail: format!(
                        "Этот мод сделан для {:?}, а профиль использует {}. Найдите версию мода для {} или выключите его.",
                        m.loader,
                        loader.display(),
                        loader.display()
                    ),
                    evidence: format!("в {} найден {}", f.file_name, meta_file(m)),
                    files: vec![f.file_name.clone()],
                    fixes: vec![Fix::Disable { file_name: f.file_name.clone() }],
                });
                continue;
            }
            for d in &m.dependencies {
                let found = present
                    .get(&d.id)
                    .map(|(v, file)| (Some(v.as_str()), Some(file.as_str())))
                    .or_else(|| builtin.get(d.id.as_str()).map(|v| (*v, None)));
                match d.kind {
                    DepKind::Required => match found {
                        None => findings.push(Finding {
                            severity: Severity::Blocking,
                            title: format!("{} требует мод «{}», а его нет", m.name, d.id),
                            detail: format!("Без «{}» загрузчик не запустит игру.", d.id),
                            evidence: format!(
                                "{}: обязательная зависимость {} {}",
                                meta_file(m),
                                d.id,
                                d.range
                            ),
                            files: vec![f.file_name.clone()],
                            fixes: vec![Fix::Install {
                                mod_id: d.id.clone(),
                            }],
                        }),
                        Some((Some(v), _)) => match range::matches(v, &d.range) {
                            Some(false) => findings.push(Finding {
                                severity: Severity::Blocking,
                                title: format!(
                                    "{} требует {} {}, а стоит {}",
                                    m.name, d.id, d.range, v
                                ),
                                detail:
                                    "Версии не подходят друг к другу — загрузчик остановит запуск."
                                        .into(),
                                evidence: format!(
                                    "{}: зависимость {} {}",
                                    meta_file(m),
                                    d.id,
                                    d.range
                                ),
                                files: vec![f.file_name.clone()],
                                fixes: Vec::new(),
                            }),
                            None => findings.push(Finding {
                                severity: Severity::Warning,
                                title: format!(
                                    "Не удалось проверить версию {} для {}",
                                    d.id, m.name
                                ),
                                detail: format!(
                                    "Нужна «{}», стоит {} — формат версии непонятен лаунчеру.",
                                    d.range, v
                                ),
                                evidence: format!(
                                    "{}: зависимость {} {}",
                                    meta_file(m),
                                    d.id,
                                    d.range
                                ),
                                files: vec![f.file_name.clone()],
                                fixes: Vec::new(),
                            }),
                            Some(true) => {}
                        },
                        Some((None, _)) => {}
                    },
                    DepKind::Optional => {
                        if let Some((Some(v), _)) = found
                            && range::matches(v, &d.range) == Some(false)
                        {
                            findings.push(Finding {
                                severity: Severity::Warning,
                                title: format!(
                                    "{} рекомендует {} {}, а стоит {}",
                                    m.name, d.id, d.range, v
                                ),
                                detail: "Игра запустится, но вместе они могут работать хуже."
                                    .into(),
                                evidence: format!(
                                    "{}: рекомендуемая зависимость {} {}",
                                    meta_file(m),
                                    d.id,
                                    d.range
                                ),
                                files: vec![f.file_name.clone()],
                                fixes: Vec::new(),
                            });
                        }
                    }
                    DepKind::Breaks | DepKind::Conflicts => {
                        let Some((Some(v), Some(other))) = found else {
                            continue;
                        };
                        if range::matches(v, &d.range) == Some(false) {
                            continue;
                        }
                        let hard = d.kind == DepKind::Breaks;
                        findings.push(Finding {
                            severity: if hard { Severity::Blocking } else { Severity::Warning },
                            title: if hard {
                                format!("{} несовместим с «{}»", m.name, d.id)
                            } else {
                                format!("{} плохо работает вместе с «{}»", m.name, d.id)
                            },
                            detail: if hard {
                                "Автор мода сам указал, что вместе они не работают: загрузчик не запустит игру. Выключите один из них — файл останется в профиле.".into()
                            } else {
                                "Автор мода предупреждает о проблемах. Игра запустится.".into()
                            },
                            evidence: format!(
                                "{}: {} {} {}",
                                meta_file(m),
                                if hard { "поле breaks/incompatible —" } else { "поле conflicts/discouraged —" },
                                d.id,
                                d.range
                            ),
                            files: vec![f.file_name.clone(), other.to_string()],
                            fixes: vec![
                                Fix::Disable { file_name: f.file_name.clone() },
                                Fix::Disable { file_name: other.to_string() },
                            ],
                        });
                    }
                }
            }
        }
    }

    // Правила-данные (графические движки).
    // Пары «вместе не работают» — данные (mods::rules), обновляются без релиза.
    let rules = super::rules::current();
    for super::rules::Conflict { a, b, why } in &rules.conflicts {
        if let (Some((_, fa)), Some((_, fb))) = (present.get(a.as_str()), present.get(b.as_str())) {
            if fa == fb {
                continue;
            }
            findings.push(Finding {
                severity: Severity::Blocking,
                title: format!("«{a}» и «{b}» не работают вместе"),
                detail: format!("{why}. Оставьте один."),
                evidence: format!("правило базы Quarry (версия от {})", rules.updated),
                files: vec![fa.clone(), fb.clone()],
                fixes: vec![
                    Fix::Disable {
                        file_name: fa.clone(),
                    },
                    Fix::Disable {
                        file_name: fb.clone(),
                    },
                ],
            });
        }
    }

    // Состояние каждого файла.
    let mut worst: BTreeMap<&str, (ModState, Vec<String>)> = BTreeMap::new();
    for finding in &findings {
        let st = if finding.severity == Severity::Blocking {
            ModState::Blocked
        } else {
            ModState::Warning
        };
        for file in &finding.files {
            let e = worst
                .entry(file.as_str())
                .or_insert((ModState::Ok, Vec::new()));
            if st > e.0 {
                e.0 = st;
            }
            e.1.push(finding.title.clone());
        }
    }
    let mods: Vec<ModReport> = files
        .iter()
        .map(|f| {
            let (state, notes) = if !f.enabled {
                (ModState::Disabled, Vec::new())
            } else if f.mods.is_empty() {
                (
                    ModState::Unknown,
                    vec![
                        f.read_error
                            .clone()
                            .unwrap_or_else(|| "в файле нет метаданных мода".into()),
                    ],
                )
            } else {
                worst
                    .get(f.file_name.as_str())
                    .cloned()
                    .unwrap_or((ModState::Ok, Vec::new()))
            };
            ModReport {
                file_name: f.file_name.clone(),
                state,
                state_label: state.label(),
                notes,
            }
        })
        .collect();
    let count = |s: ModState| mods.iter().filter(|m| m.state == s).count();
    let blocked = count(ModState::Blocked);
    Report {
        total: files.len(),
        ok: count(ModState::Ok),
        warnings: count(ModState::Warning),
        blocked,
        can_launch: !findings.iter().any(|f| f.severity == Severity::Blocking),
        findings,
        mods,
    }
}

/// ID всех модов профиля (для подсказок каталога).
pub fn present_ids(files: &[ModFile]) -> BTreeSet<String> {
    files
        .iter()
        .filter(|f| f.enabled)
        .flat_map(|f| f.mods.iter().map(|m| m.id.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mods::meta::{Dependency, Side};

    fn file(name: &str, meta: ModMeta) -> ModFile {
        ModFile {
            world_content: false,
            file_name: name.into(),
            rel_path: format!("mods/{name}"),
            enabled: true,
            size: 1,
            sha1: "x".into(),
            mods: vec![meta],
            read_error: None,
            source: None,
        }
    }
    fn fabric(id: &str, version: &str, deps: Vec<Dependency>) -> ModMeta {
        ModMeta {
            loader: ModLoader::Fabric,
            id: id.into(),
            name: id.into(),
            version: version.into(),
            side: Side::Both,
            dependencies: deps,
            provides: vec![],
            mixins: vec![],
            nested: vec![],
        }
    }
    fn dep(id: &str, range: &str, kind: DepKind) -> Dependency {
        Dependency {
            id: id.into(),
            range: range.into(),
            kind,
        }
    }

    #[test]
    fn clean_pack_is_ok() {
        let files = vec![
            file(
                "sodium.jar",
                fabric(
                    "sodium",
                    "0.8.15",
                    vec![
                        dep("minecraft", "~1.21.11", DepKind::Required),
                        dep("fabricloader", ">=0.16", DepKind::Required),
                    ],
                ),
            ),
            file(
                "iris.jar",
                fabric(
                    "iris",
                    "1.9.0",
                    vec![dep("sodium", "0.8.x", DepKind::Required)],
                ),
            ),
        ];
        let r = check(&files, LoaderKind::Fabric, "1.21.11", Some("0.17.2"));
        assert!(r.findings.is_empty(), "{:?}", r.findings);
        assert!(r.can_launch);
        assert_eq!(r.ok, 2);
    }

    #[test]
    fn missing_and_wrong_versions_block() {
        let files = vec![
            file(
                "iris.jar",
                fabric(
                    "iris",
                    "1.9.0",
                    vec![dep("sodium", ">=0.9", DepKind::Required)],
                ),
            ),
            file("sodium.jar", fabric("sodium", "0.8.15", vec![])),
            file(
                "dh.jar",
                fabric(
                    "distanthorizons",
                    "2.3",
                    vec![
                        dep("fabric-api", "*", DepKind::Required),
                        dep("sodium", ">=0.9", DepKind::Optional),
                    ],
                ),
            ),
        ];
        let r = check(&files, LoaderKind::Fabric, "1.21.11", None);
        assert!(!r.can_launch);
        assert!(
            r.findings
                .iter()
                .any(|f| f.title.contains("требует sodium >=0.9"))
        );
        assert!(r.findings.iter().any(|f| f.fixes.contains(&Fix::Install {
            mod_id: "fabric-api".into()
        })));
        assert!(
            r.findings
                .iter()
                .any(|f| f.severity == Severity::Warning && f.title.contains("рекомендует"))
        );
        assert_eq!(
            r.mods
                .iter()
                .find(|m| m.file_name == "iris.jar")
                .unwrap()
                .state,
            ModState::Blocked
        );
    }

    #[test]
    fn breaks_duplicates_wrong_loader_and_rules() {
        let neo = ModMeta {
            loader: ModLoader::NeoForge,
            ..fabric("jei", "19", vec![])
        };
        let files = vec![
            file("optifabric.jar", fabric("optifabric", "1.0", vec![])),
            file(
                "sodium.jar",
                fabric(
                    "sodium",
                    "0.8",
                    vec![dep("optifabric", "*", DepKind::Breaks)],
                ),
            ),
            file("sodium-copy.jar", fabric("sodium", "0.8", vec![])),
            file("jei.jar", neo),
        ];
        let r = check(&files, LoaderKind::Fabric, "1.21.11", None);
        let titles: Vec<&str> = r.findings.iter().map(|f| f.title.as_str()).collect();
        assert!(
            titles
                .iter()
                .any(|t| t.contains("несовместим с «optifabric»")),
            "{titles:?}"
        );
        assert!(titles.iter().any(|t| t.contains("установлен дважды")));
        assert!(titles.iter().any(|t| t.contains("для другого загрузчика")));
        assert!(
            titles
                .iter()
                .any(|t| t.contains("«optifabric» и «sodium» не работают вместе"))
        );
        assert!(
            r.findings.iter().all(|f| !f.evidence.is_empty()),
            "у каждого вывода есть основание"
        );
    }

    #[test]
    fn quilt_accepts_fabric_mods_and_disabled_is_ignored() {
        let mut off = file(
            "old.jar",
            fabric("old", "1", vec![dep("nothing", "*", DepKind::Required)]),
        );
        off.enabled = false;
        let files = vec![file("sodium.jar", fabric("sodium", "0.8", vec![])), off];
        let r = check(&files, LoaderKind::Quilt, "1.21.11", None);
        assert!(r.can_launch);
        assert_eq!(
            r.mods
                .iter()
                .find(|m| m.file_name == "old.jar")
                .unwrap()
                .state,
            ModState::Disabled
        );
    }
}
