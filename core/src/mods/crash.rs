//! Разбор краша: «вероятный виновник» по стектрейсу и известным признакам.
//! Догадка не выдаётся за факт: у вывода есть уровень уверенности и строки-основания.

use super::ModFile;
use crate::error::{IoContext, Result};
use regex::Regex;
use serde::Serialize;
use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    /// Причина прямо названа в логе.
    High,
    /// Сильный косвенный признак (мод в стектрейсе).
    Medium,
    /// Слабый признак.
    Low,
}

impl Confidence {
    pub fn label(self) -> &'static str {
        match self {
            Confidence::High => "высокая",
            Confidence::Medium => "средняя",
            Confidence::Low => "низкая",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Action {
    DisableMod { file_name: String },
    IncreaseMemory,
    UpdateDrivers,
    OpenModDoctor,
    SafeLaunch,
}

#[derive(Debug, Clone, Serialize)]
pub struct Analysis {
    pub title: String,
    pub explanation: String,
    pub confidence: Confidence,
    pub confidence_label: &'static str,
    /// Строки лога, на которых основан вывод.
    pub evidence: Vec<String>,
    /// Файлы модов-подозреваемых.
    pub suspects: Vec<String>,
    pub actions: Vec<Action>,
}

fn re(cell: &'static OnceLock<Regex>, pattern: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pattern).expect("корректное выражение"))
}

/// Пакеты Java → файл мода (по классам внутри JAR). Нужен для поиска мода в стектрейсе.
pub fn package_index(game_dir: &Path, files: &[ModFile]) -> Result<BTreeMap<String, String>> {
    let mut out = BTreeMap::new();
    for f in files.iter().filter(|f| f.enabled) {
        let path = game_dir.join(&f.rel_path);
        let file = std::fs::File::open(&path).at(&path)?;
        let Ok(mut zip) = zip::ZipArchive::new(file) else {
            continue;
        };
        for i in 0..zip.len() {
            let Ok(e) = zip.by_index_raw(i) else { continue };
            let name = e.name().unwrap_or_default();
            if let Some(cls) = name.strip_suffix(".class")
                && let Some((pkg, _)) = cls.rsplit_once('/')
                && !pkg.starts_with("META-INF")
            {
                out.entry(pkg.replace('/', "."))
                    .or_insert_with(|| f.file_name.clone());
            }
        }
    }
    Ok(out)
}

/// Пакеты игры, Java и загрузчиков — в стектрейсе их полно, но они не «виновники».
const SYSTEM_PREFIXES: &[&str] = &[
    "java.",
    "javax.",
    "jdk.",
    "sun.",
    "com.sun.",
    "net.minecraft.",
    "com.mojang.",
    "org.lwjgl.",
    "net.fabricmc.",
    "org.quiltmc.loader",
    "net.neoforged.",
    "net.minecraftforge.",
    "cpw.mods.",
    "org.spongepowered.",
    "com.llamalad7.mixinextras",
    "org.objectweb.asm",
    "io.netty.",
    "com.google.",
    "org.apache.",
    "it.unimi.",
    "knot.",
    "TRANSFORMER",
];

fn mod_for_package(pkg: &str, index: &BTreeMap<String, String>) -> Option<String> {
    let mut p = pkg;
    loop {
        if let Some(f) = index.get(p) {
            return Some(f.clone());
        }
        p = &p[..p.rfind('.')?];
    }
}

/// Разобрать краш по строкам лога (и тексту crash report, если есть).
pub fn analyze(
    lines: &[String],
    files: &[ModFile],
    packages: &BTreeMap<String, String>,
) -> Analysis {
    static MIXIN_FAIL: OnceLock<Regex> = OnceLock::new();
    static MIXIN_JSON: OnceLock<Regex> = OnceLock::new();
    static FRAME: OnceLock<Regex> = OnceLock::new();
    let mixin_fail = re(&MIXIN_FAIL, r"Mixin apply for mod (\S+) failed");
    let mixin_json = re(
        &MIXIN_JSON,
        r"([A-Za-z0-9_.\-]*mixins?[A-Za-z0-9_.\-]*\.json)",
    );
    let frame = re(
        &FRAME,
        r"^\s*at (?:[\w\-.]+//)?(?:[\w@.\-]+/)?([a-z][\w$]*(?:\.[a-z_][\w$]*)+)\.[A-Za-z_$][\w$]*\.",
    );
    let by_id = |id: &str| -> Option<String> {
        files
            .iter()
            .find(|f| f.mods.iter().any(|m| m.id == id))
            .map(|f| f.file_name.clone())
    };
    let text_has = |needle: &str| lines.iter().find(|l| l.contains(needle)).cloned();

    // 1. Нехватка памяти.
    if let Some(l) = text_has("java.lang.OutOfMemoryError") {
        return Analysis {
            title: "Игре не хватило памяти".into(),
            explanation: "Java сообщила, что выделенная память закончилась. Увеличьте память профиля на вкладке «Запуск и память» (обычно на 1–2 ГБ).".into(),
            confidence: Confidence::High,
            confidence_label: Confidence::High.label(),
            evidence: vec![l],
            suspects: Vec::new(),
            actions: vec![Action::IncreaseMemory],
        };
    }
    // 2. Известная проблема старых версий на macOS.
    if let Some(l) = text_has("Regular windows do not have icons on macOS") {
        return Analysis {
            title: "Старая версия игры не открывает окно на macOS".into(),
            explanation: "Версии 1.14–1.18 пытаются поставить иконку окна, а современная macOS этого не позволяет (ошибка игры MCL-15163, исправлена Mojang в 1.19). Для Fabric-профиля помогает мод-исправление; для обычной игры — версия 1.19 и новее.".into(),
            confidence: Confidence::High,
            confidence_label: Confidence::High.label(),
            evidence: vec![l],
            suspects: Vec::new(),
            actions: Vec::new(),
        };
    }
    // 3. Загрузчик сам перечислил несовместимые моды.
    if let Some(l) = text_has("Incompatible mods found!")
        .or_else(|| text_has("Incompatible mod set!"))
        .or_else(|| text_has("Missing or unsupported mandatory dependencies"))
    {
        let details: Vec<String> = lines
            .iter()
            .skip_while(|x| **x != l)
            .take(12)
            .cloned()
            .collect();
        return Analysis {
            title: "Загрузчик нашёл несовместимые моды".into(),
            explanation: "Загрузчик остановил запуск и перечислил, каких модов не хватает или какие мешают друг другу. Mod Doctor покажет то же самое до запуска.".into(),
            confidence: Confidence::High,
            confidence_label: Confidence::High.label(),
            evidence: details,
            suspects: Vec::new(),
            actions: vec![Action::OpenModDoctor],
        };
    }
    // 4. Mixin прямо называет мод.
    for l in lines {
        if let Some(c) = mixin_fail.captures(l) {
            let id = c[1].to_string();
            let file = by_id(&id);
            return Analysis {
                title: format!("Мод «{id}» не смог встроиться в игру"),
                explanation: "Мод пытался изменить код игры или другого мода, но не нашёл нужного места. Обычно это значит, что версия мода не подходит к версии игры или к другому моду, который он дополняет.".into(),
                confidence: Confidence::High,
                confidence_label: Confidence::High.label(),
                evidence: vec![l.clone()],
                suspects: file.iter().cloned().collect(),
                actions: file.map(|f| vec![Action::DisableMod { file_name: f }, Action::SafeLaunch]).unwrap_or_default(),
            };
        }
    }
    // 5. Mixin-конфиг мода в трассе.
    for l in lines {
        for c in mixin_json.captures_iter(l) {
            let cfg = &c[1];
            if let Some(f) = files
                .iter()
                .find(|f| f.mods.iter().any(|m| m.mixins.iter().any(|x| x == cfg)))
            {
                return Analysis {
                    title: format!("Вероятный виновник — {}", f.display_name()),
                    explanation: format!(
                        "В ошибке упомянут mixin-конфиг «{cfg}», он принадлежит этому моду."
                    ),
                    confidence: Confidence::High,
                    confidence_label: Confidence::High.label(),
                    evidence: vec![l.clone()],
                    suspects: vec![f.file_name.clone()],
                    actions: vec![
                        Action::DisableMod {
                            file_name: f.file_name.clone(),
                        },
                        Action::SafeLaunch,
                    ],
                };
            }
        }
    }
    // 6. Первый кадр стектрейса из мода.
    for l in lines {
        if let Some(c) = frame.captures(l) {
            let pkg = &c[1];
            if SYSTEM_PREFIXES.iter().any(|p| pkg.starts_with(p)) {
                continue;
            }
            if let Some(file) = mod_for_package(pkg, packages) {
                let name = files
                    .iter()
                    .find(|f| f.file_name == file)
                    .map(|f| f.display_name())
                    .unwrap_or_else(|| file.clone());
                return Analysis {
                    title: format!("Вероятный виновник — {name}"),
                    explanation: "Ошибка произошла внутри кода этого мода. Это сильный признак, но не доказательство: иногда мод падает из-за другого мода или неподходящей версии.".into(),
                    confidence: Confidence::Medium,
                    confidence_label: Confidence::Medium.label(),
                    evidence: vec![l.clone()],
                    suspects: vec![file.clone()],
                    actions: vec![Action::DisableMod { file_name: file }, Action::SafeLaunch],
                };
            }
        }
    }
    // 7. Драйвер видеокарты.
    for needle in [
        "EXCEPTION_ACCESS_VIOLATION",
        "atio6axx",
        "nvoglv",
        "ig9icd",
        "ig7icd",
        "libGL error",
    ] {
        if let Some(l) = text_has(needle) {
            return Analysis {
                title: "Похоже на проблему драйвера видеокарты".into(),
                explanation: "Игра упала в коде графического драйвера. Обновите драйвер видеокарты; если не поможет — попробуйте запуск без шейдеров и модов графики.".into(),
                confidence: Confidence::Low,
                confidence_label: Confidence::Low.label(),
                evidence: vec![l],
                suspects: Vec::new(),
                actions: vec![Action::UpdateDrivers, Action::SafeLaunch],
            };
        }
    }
    let evidence: Vec<String> = lines
        .iter()
        .filter(|l| l.contains("Exception") || l.contains("Error") || l.starts_with("Caused by"))
        .take(5)
        .cloned()
        .collect();
    Analysis {
        title: "Причину определить не удалось".into(),
        explanation: "В логе нет явных признаков. Посмотрите первую строку «Caused by» на вкладке «Ошибки» или запустите профиль без модов, чтобы понять, виноваты ли моды.".into(),
        confidence: Confidence::Low,
        confidence_label: Confidence::Low.label(),
        evidence,
        suspects: Vec::new(),
        actions: vec![Action::SafeLaunch],
    }
}

/// Текст crash-report, если игра его создала.
pub fn read_crash_report(path: &Path) -> Vec<String> {
    let mut s = String::new();
    if let Ok(mut f) = std::fs::File::open(path) {
        let _ = f.read_to_string(&mut s);
    }
    s.lines().take(4000).map(str::to_string).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mods::meta::{ModLoader, ModMeta, Side};

    fn file(name: &str, id: &str, mixins: &[&str]) -> ModFile {
        ModFile {
            world_content: false,
            file_name: name.into(),
            rel_path: format!("mods/{name}"),
            enabled: true,
            size: 1,
            sha1: "x".into(),
            mods: vec![ModMeta {
                loader: ModLoader::Fabric,
                id: id.into(),
                name: id.to_uppercase(),
                version: "1".into(),
                side: Side::Both,
                dependencies: vec![],
                provides: vec![],
                mixins: mixins.iter().map(|s| s.to_string()).collect(),
                nested: vec![],
            }],
            read_error: None,
            source: None,
        }
    }
    fn lines(s: &str) -> Vec<String> {
        s.lines().map(str::to_string).collect()
    }

    #[test]
    fn mixin_failure_names_the_mod() {
        let files = vec![file("indium.jar", "indium", &["indium.mixins.json"])];
        let a = analyze(
            &lines(
                "[13:47:33] [Render thread/ERROR]: Mixin apply for mod indium failed indium.mixins.json:MixinChunkBuilderMeshingTask\norg.spongepowered...InvalidMixinException",
            ),
            &files,
            &BTreeMap::new(),
        );
        assert_eq!(a.confidence, Confidence::High);
        assert_eq!(a.suspects, ["indium.jar"]);
        assert!(a.actions.contains(&Action::DisableMod {
            file_name: "indium.jar".into()
        }));
    }

    #[test]
    fn stack_frame_maps_to_mod_package() {
        let files = vec![file("dh.jar", "distanthorizons", &[])];
        let mut pk = BTreeMap::new();
        pk.insert(
            "com.seibel.distanthorizons.core".to_string(),
            "dh.jar".to_string(),
        );
        let a = analyze(
            &lines(
                "java.lang.NullPointerException: boom\n\tat TRANSFORMER/minecraft@1.21.1/net.minecraft.client.Minecraft.run(Minecraft.java:1)\n\tat knot//com.seibel.distanthorizons.core.render.Renderer.draw(Renderer.java:42)",
            ),
            &files,
            &pk,
        );
        assert_eq!(a.confidence, Confidence::Medium);
        assert_eq!(a.suspects, ["dh.jar"]);
        assert!(a.title.contains("DISTANTHORIZONS"));
    }

    #[test]
    fn memory_mac_and_unknown() {
        let a = analyze(
            &lines("java.lang.OutOfMemoryError: Java heap space"),
            &[],
            &BTreeMap::new(),
        );
        assert_eq!(a.actions, vec![Action::IncreaseMemory]);
        let a = analyze(
            &lines("GLFW error 65548: Cocoa: Regular windows do not have icons on macOS"),
            &[],
            &BTreeMap::new(),
        );
        assert!(a.title.contains("macOS"));
        let a = analyze(
            &lines("Caused by: java.lang.IllegalStateException: x"),
            &[],
            &BTreeMap::new(),
        );
        assert_eq!(a.confidence, Confidence::Low);
        assert_eq!(a.evidence.len(), 1);
    }

    #[test]
    fn system_frames_are_not_suspects() {
        let mut pk = BTreeMap::new();
        pk.insert("net.minecraft.client".to_string(), "fake.jar".to_string());
        let a = analyze(
            &lines("\tat net.minecraft.client.Minecraft.run(Minecraft.java:1)"),
            &[],
            &pk,
        );
        assert!(a.suspects.is_empty());
    }
}
