//! Замена LWJGL на macOS для версий 1.13–1.18.
//!
//! Эти версии используют LWJGL 3.1–3.2, чей GLFW не может создать окно на современных macOS:
//! «Cocoa: Failed to find service port for display» (воспроизведено 21.10.2026 на 1.16.5,
//! macOS на Apple M4). Так же, как Prism Launcher, подставляем LWJGL 3.3.1 — берём
//! его описание из официального JSON версии 1.20.1 Mojang (те же модули, хэши и нативы
//! для Intel и Apple Silicon).

use crate::metadata::version::{Library, VersionJson};
use crate::metadata::{Environment, Os};
use std::collections::BTreeSet;

/// Версия-донор: JSON Mojang с LWJGL 3.3.1 для всех ОС и архитектур.
pub const DONOR_VERSION: &str = "1.20.1";

fn lwjgl_module(lib: &Library) -> Option<(String, String)> {
    let c = lib.coord()?;
    (c.group == "org.lwjgl").then_some((c.artifact, c.version))
}

/// Нужна ли замена: macOS и активные библиотеки org.lwjgl версий 3.1/3.2.
pub fn needs_replacement(version: &VersionJson, env: &Environment) -> bool {
    env.os == Os::Osx
        && version
            .libraries
            .iter()
            .filter(|l| l.is_allowed(env))
            .filter_map(lwjgl_module)
            .any(|(_, v)| v.starts_with("3.1.") || v.starts_with("3.2."))
}

/// Заменить все библиотеки org.lwjgl библиотеками донора. Если у донора нет какого-то
/// модуля — ничего не меняем (лучше старая LWJGL, чем недостающий модуль).
/// Возвращает пояснение для пользователя.
pub fn replace(
    version: &mut VersionJson,
    donor: &VersionJson,
    env: &Environment,
) -> Option<String> {
    if !needs_replacement(version, env) {
        return None;
    }
    let old_modules: BTreeSet<String> = version
        .libraries
        .iter()
        .filter_map(lwjgl_module)
        .map(|(a, _)| a)
        .collect();
    let donor_libs: Vec<Library> = donor
        .libraries
        .iter()
        .filter(|l| lwjgl_module(l).is_some())
        .cloned()
        .collect();
    let donor_modules: BTreeSet<String> = donor_libs
        .iter()
        .filter_map(lwjgl_module)
        .map(|(a, _)| a)
        .collect();
    if !old_modules.is_subset(&donor_modules) {
        return None;
    }
    let old_version = version
        .libraries
        .iter()
        .filter(|l| l.is_allowed(env))
        .filter_map(lwjgl_module)
        .map(|(_, v)| v)
        .next()
        .unwrap_or_default();
    let new_version = donor_libs
        .iter()
        .filter_map(lwjgl_module)
        .map(|(_, v)| v)
        .next()
        .unwrap_or_default();
    // Порядок classpath сохраняется: новые библиотеки встают на место первой старой.
    let pos = version
        .libraries
        .iter()
        .position(|l| lwjgl_module(l).is_some())
        .unwrap_or(0);
    version.libraries.retain(|l| lwjgl_module(l).is_none());
    let keep: Vec<Library> = donor_libs
        .into_iter()
        .filter(|l| lwjgl_module(l).is_some_and(|(a, _)| old_modules.contains(&a)))
        .collect();
    let pos = pos.min(version.libraries.len());
    version.libraries.splice(pos..pos, keep);
    Some(format!(
        "LWJGL {old_version} заменена на {new_version}: старая версия не может открыть окно на современных macOS (так же делает Prism Launcher)"
    ))
}

/// Версия падает на macOS при установке иконки окна (ошибка игры MCL-15163): GLFW из LWJGL 3.3
/// сообщает, что иконок у окон macOS нет, а игра считает любую ошибку GLFW при старте фатальной.
/// Снапшоты не угадываем — для них сработает разбор краша.
pub fn icon_crash(game_version: &str) -> bool {
    let minor = game_version
        .strip_prefix("1.")
        .and_then(|r| r.split(['.', '-']).next())
        .and_then(|m| m.parse::<u32>().ok());
    matches!(minor, Some(13..=17))
}

/// Почему vanilla-профиль этой версии не откроет окно на macOS и что делать; `None` — откроет.
pub fn vanilla_icon_crash(game_version: &str) -> Option<String> {
    if !icon_crash(game_version) {
        return None;
    }
    let way = if game_version.starts_with("1.13") {
        "Выберите версию 1.12.2 или старше либо 1.18 и новее."
    } else {
        "Создайте профиль Fabric той же версии (Mod Doctor предложит мод-исправление) или выберите версию 1.18 и новее."
    };
    Some(format!(
        "Minecraft {game_version} без модов не откроет окно на современной macOS: игра ставит иконку окна, а macOS это запрещает (ошибка игры MCL-15163). {way}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadata::Arch;

    #[test]
    fn icon_crash_only_for_1_13_to_1_17() {
        // Проверено запуском 22.10.2026 (macOS, Apple M4): 1.13.2 и 1.16.5 падают, 1.12.2 и 1.18.2 — нет.
        for v in ["1.13", "1.13.2", "1.14.4", "1.16.5", "1.16.5-rc1", "1.17.1"] {
            assert!(icon_crash(v), "{v}");
        }
        for v in [
            "1.12.2", "1.18", "1.18.2", "1.21.11", "26.3", "21w03a", "b1.7.3",
        ] {
            assert!(!icon_crash(v), "{v}");
        }
        assert!(vanilla_icon_crash("1.16.5").unwrap().contains("Fabric"));
        assert!(
            !vanilla_icon_crash("1.13.2").unwrap().contains("Fabric"),
            "для 1.13 Fabric нет"
        );
        assert!(vanilla_icon_crash("1.18.2").is_none());
    }

    fn fixture(name: &str) -> VersionJson {
        let path = format!(
            "{}/tests/fixtures/versions/{name}.json",
            env!("CARGO_MANIFEST_DIR")
        );
        VersionJson::parse(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn replaced_on_macos_for_1_16_5() {
        let env = Environment::new(Os::Osx, Arch::Arm64, "15.1");
        let mut v = fixture("1.16.5");
        assert!(!v.has_macos_arm64_natives());
        let note = replace(&mut v, &fixture(DONOR_VERSION), &env).unwrap();
        assert!(note.contains("3.2.1") && note.contains("3.3.1"), "{note}");
        let versions: BTreeSet<String> = v
            .libraries
            .iter()
            .filter_map(lwjgl_module)
            .map(|(_, v)| v)
            .collect();
        assert_eq!(versions, BTreeSet::from(["3.3.1".to_string()]));
        assert!(
            v.has_macos_arm64_natives(),
            "теперь есть нативы для Apple Silicon"
        );
        // Все 7 модулей на месте.
        let modules: BTreeSet<String> = v
            .libraries
            .iter()
            .filter_map(lwjgl_module)
            .map(|(a, _)| a)
            .collect();
        assert_eq!(modules.len(), 7);
        assert!(!needs_replacement(&v, &env), "повторно не меняется");
    }

    #[test]
    fn untouched_elsewhere() {
        let donor = fixture(DONOR_VERSION);
        for (name, env) in [
            (
                "1.16.5",
                Environment::new(Os::Windows, Arch::X86_64, "10.0"),
            ),
            ("1.16.5", Environment::new(Os::Linux, Arch::X86_64, "6.8")),
            ("26.3", Environment::new(Os::Osx, Arch::Arm64, "15.1")),
            ("1.12.2", Environment::new(Os::Osx, Arch::Arm64, "15.1")), // LWJGL 2 — другая история
        ] {
            let mut v = fixture(name);
            let before = v.libraries.len();
            assert!(replace(&mut v, &donor, &env).is_none(), "{name}");
            assert_eq!(v.libraries.len(), before);
        }
    }
}
