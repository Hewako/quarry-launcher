//! Метаданные мода из самого JAR (а не из имени файла): fabric.mod.json, quilt.mod.json,
//! META-INF/neoforge.mods.toml (NeoForge 20.5+), META-INF/mods.toml (Forge и ранний NeoForge).
//! Если метаданные прочитать не удалось, это так и сообщается — данные не додумываются.

use crate::error::{Error, IoContext, Result};
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModLoader {
    Fabric,
    Quilt,
    NeoForge,
    Forge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Client,
    Server,
    Both,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DepKind {
    /// Обязательная зависимость.
    Required,
    /// Рекомендуемая/необязательная.
    Optional,
    /// Объявленная несовместимость: запуск невозможен (Fabric `breaks`, NeoForge `incompatible`).
    Breaks,
    /// Предупреждение о проблемах (Fabric `conflicts`, NeoForge `discouraged`).
    Conflicts,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dependency {
    pub id: String,
    /// Диапазон версий как в метаданных (`>=0.7.3`, `[1.0,2.0)`, `*`).
    pub range: String,
    pub kind: DepKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModMeta {
    pub loader: ModLoader,
    pub id: String,
    pub name: String,
    pub version: String,
    pub side: Side,
    pub dependencies: Vec<Dependency>,
    /// Другие ID, которые мод «предоставляет» (Fabric `provides`).
    pub provides: Vec<String>,
    /// Mixin-конфиги — по ним мод узнаётся в стектрейсе краша.
    pub mixins: Vec<String>,
    /// Вложенные моды (jar-in-jar), уже разобранные.
    pub nested: Vec<ModMeta>,
}

fn read_entry(zip: &mut zip::ZipArchive<std::fs::File>, name: &str) -> Option<String> {
    let mut e = zip.by_name(name).ok()?;
    let mut s = String::new();
    e.read_to_string(&mut s).ok()?;
    Some(s)
}

/// Прочитать метаданные JAR. `Ok(vec![])` — это не мод известного формата (или библиотека).
/// В одном JAR может быть несколько модов (mods.toml допускает несколько `[[mods]]`).
pub fn read_jar(path: &Path) -> Result<Vec<ModMeta>> {
    let file = std::fs::File::open(path).at(path)?;
    let mut zip = zip::ZipArchive::new(file).map_err(|_| {
        Error::invalid(format!(
            "{} — не JAR-архив или файл повреждён",
            path.display()
        ))
    })?;
    let mut out = Vec::new();
    if let Some(text) = read_entry(&mut zip, "quilt.mod.json")
        && let Some(m) = parse_quilt(&text)
    {
        out.push(m);
    }
    if out.is_empty()
        && let Some(text) = read_entry(&mut zip, "fabric.mod.json")
        && let Some(mut m) = parse_fabric(&text)
    {
        m.nested = nested_fabric(&mut zip, &text);
        out.push(m);
    }
    let manifest_version = read_entry(&mut zip, "META-INF/MANIFEST.MF").and_then(|m| {
        m.lines().find_map(|l| {
            l.strip_prefix("Implementation-Version:")
                .map(|v| v.trim().to_string())
        })
    });
    for (file, loader) in [
        ("META-INF/neoforge.mods.toml", ModLoader::NeoForge),
        ("META-INF/mods.toml", ModLoader::Forge),
    ] {
        if !out.is_empty() {
            break;
        }
        if let Some(text) = read_entry(&mut zip, file) {
            out.extend(parse_mods_toml(&text, loader, manifest_version.as_deref()));
        }
    }
    Ok(out)
}

/// Добавляет ли мод что-то в сам мир: генерацию, измерения или новые блоки.
/// Удаление такого мода может испортить сохранённые чанки — перед этим нужен бэкап.
pub fn adds_world_content(path: &Path) -> bool {
    let Ok(file) = std::fs::File::open(path) else {
        return false;
    };
    let Ok(zip) = zip::ZipArchive::new(file) else {
        return false;
    };
    zip.file_names().flatten().any(|n| {
        let parts: Vec<&str> = n.split('/').collect();
        parts.len() >= 4
            && parts[1] != "minecraft"
            && matches!(
                (parts[0], parts[2]),
                ("data", "worldgen" | "dimension" | "dimension_type") | ("assets", "blockstates")
            )
    })
}

/// Вложенные моды Fabric (`jars: [{file: "META-INF/jars/x.jar"}]`).
fn nested_fabric(zip: &mut zip::ZipArchive<std::fs::File>, text: &str) -> Vec<ModMeta> {
    #[derive(Deserialize)]
    struct Jars {
        #[serde(default)]
        jars: Vec<JarRef>,
    }
    #[derive(Deserialize)]
    struct JarRef {
        file: String,
    }
    let Ok(j) = serde_json::from_str::<Jars>(text) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for r in j.jars.iter().take(64) {
        let Ok(mut e) = zip.by_name(&r.file) else {
            continue;
        };
        let mut buf = Vec::new();
        if e.read_to_end(&mut buf).is_err() {
            continue;
        }
        let Ok(mut inner) = zip::ZipArchive::new(std::io::Cursor::new(buf)) else {
            continue;
        };
        let Ok(mut f) = inner.by_name("fabric.mod.json") else {
            continue;
        };
        let mut s = String::new();
        if f.read_to_string(&mut s).is_ok()
            && let Some(m) = parse_fabric(&s)
        {
            out.push(m);
        }
    }
    out
}

/// Значение зависимости Fabric: строка, массив строк (любой из диапазонов) или объект.
fn range_of(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(a) => a
            .iter()
            .filter_map(|x| x.as_str())
            .collect::<Vec<_>>()
            .join(" || "),
        _ => "*".into(),
    }
}

pub fn parse_fabric(text: &str) -> Option<ModMeta> {
    // Некоторые моды кладут в JSON управляющие символы и комментарии; строгий разбор — честнее.
    let v: serde_json::Value = serde_json::from_str(text).ok()?;
    let id = v.get("id")?.as_str()?.to_string();
    let mut deps = Vec::new();
    for (key, kind) in [
        ("depends", DepKind::Required),
        ("recommends", DepKind::Optional),
        ("suggests", DepKind::Optional),
        ("breaks", DepKind::Breaks),
        ("conflicts", DepKind::Conflicts),
    ] {
        if let Some(obj) = v.get(key).and_then(|d| d.as_object()) {
            for (dep, range) in obj {
                deps.push(Dependency {
                    id: dep.clone(),
                    range: range_of(range),
                    kind,
                });
            }
        }
    }
    let side = match v.get("environment").and_then(|e| e.as_str()).unwrap_or("*") {
        "client" => Side::Client,
        "server" => Side::Server,
        _ => Side::Both,
    };
    let mixins = v
        .get("mixins")
        .and_then(|m| m.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| {
                    x.as_str()
                        .or_else(|| x.get("config").and_then(|c| c.as_str()))
                })
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    Some(ModMeta {
        loader: ModLoader::Fabric,
        name: v
            .get("name")
            .and_then(|n| n.as_str())
            .unwrap_or(&id)
            .to_string(),
        version: v
            .get("version")
            .and_then(|n| n.as_str())
            .unwrap_or("?")
            .to_string(),
        provides: v
            .get("provides")
            .and_then(|p| p.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
        id,
        side,
        dependencies: deps,
        mixins,
        nested: Vec::new(),
    })
}

pub fn parse_quilt(text: &str) -> Option<ModMeta> {
    let v: serde_json::Value = serde_json::from_str(text).ok()?;
    let ql = v.get("quilt_loader")?;
    let id = ql.get("id")?.as_str()?.to_string();
    let mut deps = Vec::new();
    let dep_of = |d: &serde_json::Value, kind: DepKind| -> Option<Dependency> {
        match d {
            serde_json::Value::String(s) => Some(Dependency {
                id: s.clone(),
                range: "*".into(),
                kind,
            }),
            serde_json::Value::Object(o) => {
                let optional = o.get("optional").and_then(|x| x.as_bool()).unwrap_or(false);
                Some(Dependency {
                    id: o.get("id")?.as_str()?.to_string(),
                    range: o
                        .get("versions")
                        .map(range_of)
                        .unwrap_or_else(|| "*".into()),
                    kind: if optional && kind == DepKind::Required {
                        DepKind::Optional
                    } else {
                        kind
                    },
                })
            }
            _ => None,
        }
    };
    for (key, kind) in [("depends", DepKind::Required), ("breaks", DepKind::Breaks)] {
        if let Some(a) = ql.get(key).and_then(|d| d.as_array()) {
            deps.extend(a.iter().filter_map(|d| dep_of(d, kind)));
        }
    }
    let meta = ql.get("metadata");
    let side = match v
        .get("minecraft")
        .and_then(|m| m.get("environment"))
        .and_then(|e| e.as_str())
    {
        Some("client") => Side::Client,
        Some("dedicated_server") => Side::Server,
        _ => Side::Both,
    };
    Some(ModMeta {
        loader: ModLoader::Quilt,
        name: meta
            .and_then(|m| m.get("name"))
            .and_then(|n| n.as_str())
            .unwrap_or(&id)
            .to_string(),
        version: ql
            .get("version")
            .and_then(|n| n.as_str())
            .unwrap_or("?")
            .to_string(),
        id,
        side,
        dependencies: deps,
        provides: Vec::new(),
        mixins: v
            .get("mixin")
            .map(|m| match m {
                serde_json::Value::String(s) => vec![s.clone()],
                serde_json::Value::Array(a) => a
                    .iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .collect(),
                _ => Vec::new(),
            })
            .unwrap_or_default(),
        nested: Vec::new(),
    })
}

/// mods.toml / neoforge.mods.toml. Поля: `[[mods]] modId, version, displayName`,
/// `[[dependencies.<modId>]] modId, type (required/optional/incompatible/discouraged)
/// или mandatory (Forge), versionRange, side`.
pub fn parse_mods_toml(text: &str, loader: ModLoader, jar_version: Option<&str>) -> Vec<ModMeta> {
    let Ok(v) = toml::from_str::<toml::Value>(text) else {
        return Vec::new();
    };
    let mixins: Vec<String> = v
        .get("mixins")
        .and_then(|m| m.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.get("config").and_then(|c| c.as_str()).map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let Some(mods) = v.get("mods").and_then(|m| m.as_array()) else {
        return Vec::new();
    };
    let deps_table = v.get("dependencies").and_then(|d| d.as_table());
    mods.iter()
        .filter_map(|m| {
            let id = m.get("modId")?.as_str()?.to_string();
            let mut version = m
                .get("version")
                .and_then(|x| x.as_str())
                .unwrap_or("?")
                .to_string();
            if version.contains("${file.jarVersion}") {
                version = jar_version.unwrap_or("?").to_string();
            }
            let mut side = Side::Unknown;
            let mut dependencies = Vec::new();
            if let Some(list) = deps_table
                .and_then(|t| t.get(&id))
                .and_then(|d| d.as_array())
            {
                for d in list {
                    let Some(dep_id) = d.get("modId").and_then(|x| x.as_str()) else {
                        continue;
                    };
                    let kind = match d.get("type").and_then(|x| x.as_str()) {
                        Some("required") => DepKind::Required,
                        Some("optional") => DepKind::Optional,
                        Some("incompatible") => DepKind::Breaks,
                        Some("discouraged") => DepKind::Conflicts,
                        _ => match d.get("mandatory").and_then(|x| x.as_bool()) {
                            Some(true) => DepKind::Required,
                            Some(false) => DepKind::Optional,
                            None => DepKind::Required,
                        },
                    };
                    dependencies.push(Dependency {
                        id: dep_id.to_string(),
                        range: d
                            .get("versionRange")
                            .and_then(|x| x.as_str())
                            .unwrap_or("*")
                            .to_string(),
                        kind,
                    });
                    // Сторона мода видна по его зависимости от minecraft.
                    if dep_id == "minecraft" {
                        side = match d.get("side").and_then(|x| x.as_str()) {
                            Some("CLIENT") => Side::Client,
                            Some("SERVER") => Side::Server,
                            Some("BOTH") => Side::Both,
                            _ => Side::Unknown,
                        };
                    }
                }
            }
            Some(ModMeta {
                loader,
                name: m
                    .get("displayName")
                    .and_then(|x| x.as_str())
                    .unwrap_or(&id)
                    .to_string(),
                version,
                id,
                side,
                dependencies,
                provides: Vec::new(),
                mixins: mixins.clone(),
                nested: Vec::new(),
            })
        })
        .collect()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::Write;

    /// Собрать тестовый JAR с указанными файлами.
    pub fn make_jar(path: &Path, files: &[(&str, &[u8])]) {
        let mut w = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
        for (name, data) in files {
            w.start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            w.write_all(data).unwrap();
        }
        w.finish().unwrap();
    }

    #[test]
    fn fabric_metadata() {
        let m = parse_fabric(r#"{"schemaVersion":1,"id":"iris","version":"1.9.0","name":"Iris","environment":"client",
            "depends":{"minecraft":"~1.21.11","fabricloader":">=0.16","sodium":["0.8.x","0.9.x"]},
            "breaks":{"optifabric":"*"},"conflicts":{"oculus":"*"},
            "mixins":["mixins.iris.json",{"config":"iris.fabric.mixins.json","environment":"client"}]}"#).unwrap();
        assert_eq!(m.id, "iris");
        assert_eq!(m.side, Side::Client);
        assert!(m.dependencies.contains(&Dependency {
            id: "sodium".into(),
            range: "0.8.x || 0.9.x".into(),
            kind: DepKind::Required
        }));
        assert!(
            m.dependencies
                .iter()
                .any(|d| d.id == "optifabric" && d.kind == DepKind::Breaks)
        );
        assert!(
            m.dependencies
                .iter()
                .any(|d| d.id == "oculus" && d.kind == DepKind::Conflicts)
        );
        assert_eq!(m.mixins, ["mixins.iris.json", "iris.fabric.mixins.json"]);
    }

    #[test]
    fn quilt_metadata() {
        let m = parse_quilt(r#"{"schema_version":1,"quilt_loader":{"group":"x","id":"qmod","version":"2.0",
            "metadata":{"name":"Q Mod"},"depends":["quilt_loader",{"id":"qsl","versions":">=8.0"},{"id":"emi","optional":true}],
            "breaks":[{"id":"badmod"}]},"minecraft":{"environment":"client"},"mixin":"qmod.mixins.json"}"#).unwrap();
        assert_eq!(m.loader, ModLoader::Quilt);
        assert_eq!(m.name, "Q Mod");
        assert!(
            m.dependencies
                .iter()
                .any(|d| d.id == "emi" && d.kind == DepKind::Optional)
        );
        assert!(
            m.dependencies
                .iter()
                .any(|d| d.id == "badmod" && d.kind == DepKind::Breaks)
        );
        assert_eq!(m.side, Side::Client);
    }

    #[test]
    fn neoforge_and_forge_toml() {
        let toml = r#"
modLoader="javafml"
loaderVersion="[4,)"
[[mods]]
modId="jei"
version="${file.jarVersion}"
displayName="Just Enough Items"
[[mixins]]
config="jei.mixins.json"
[[dependencies.jei]]
modId="minecraft"
type="required"
versionRange="[1.21.1,1.22)"
side="BOTH"
[[dependencies.jei]]
modId="rei"
type="incompatible"
versionRange="*"
side="CLIENT"
"#;
        let mods = parse_mods_toml(toml, ModLoader::NeoForge, Some("19.21.0"));
        assert_eq!(mods.len(), 1);
        assert_eq!(
            mods[0].version, "19.21.0",
            "версия из MANIFEST вместо ${{file.jarVersion}}"
        );
        assert_eq!(mods[0].side, Side::Both);
        assert!(
            mods[0]
                .dependencies
                .iter()
                .any(|d| d.id == "rei" && d.kind == DepKind::Breaks)
        );
        assert_eq!(mods[0].mixins, ["jei.mixins.json"]);
        // Forge: mandatory вместо type.
        let forge = parse_mods_toml(
            "[[mods]]\nmodId=\"a\"\nversion=\"1\"\n[[dependencies.a]]\nmodId=\"b\"\nmandatory=false\nversionRange=\"*\"\n",
            ModLoader::Forge,
            None,
        );
        assert_eq!(forge[0].dependencies[0].kind, DepKind::Optional);
    }

    #[test]
    fn jar_reading_and_broken_jars() {
        let d = std::env::temp_dir().join(format!("quarry-meta-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let jar = d.join("sodium.jar");
        make_jar(
            &jar,
            &[(
                "fabric.mod.json",
                br#"{"id":"sodium","version":"0.8.15","environment":"client"}"#,
            )],
        );
        let mods = read_jar(&jar).unwrap();
        assert_eq!(mods[0].id, "sodium");
        let lib = d.join("lib.jar");
        make_jar(&lib, &[("a/B.class", b"x")]);
        assert!(
            read_jar(&lib).unwrap().is_empty(),
            "библиотека без метаданных — пустой список"
        );
        let broken = d.join("broken.jar");
        std::fs::write(&broken, b"not a zip").unwrap();
        assert!(read_jar(&broken).is_err());
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn world_content_detection() {
        let d = std::env::temp_dir().join(format!("quarry-worldmod-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let biomes = d.join("biomes.jar");
        make_jar(&biomes, &[("data/terralith/worldgen/biome/x.json", b"{}")]);
        let blocks = d.join("blocks.jar");
        make_jar(&blocks, &[("assets/create/blockstates/cog.json", b"{}")]);
        let ui = d.join("ui.jar");
        make_jar(
            &ui,
            &[
                ("assets/minecraft/blockstates/stone.json", b"{}"),
                ("assets/sodium/lang/en_us.json", b"{}"),
            ],
        );
        assert!(adds_world_content(&biomes));
        assert!(adds_world_content(&blocks));
        // Ресурсы, переопределяющие ванильные блоки, — не новый контент.
        assert!(!adds_world_content(&ui));
        std::fs::remove_dir_all(&d).unwrap();
    }
}
