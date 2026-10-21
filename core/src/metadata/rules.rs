//! Условные правила из JSON версий: `os.name`, `os.arch`, `os.version` (регулярное выражение)
//! и `features`. Семантика как у официального лаунчера: без правил — разрешено; иначе
//! начальное состояние «запрещено», каждое совпавшее правило задаёт своё действие.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RuleAction {
    Allow,
    Disallow,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rule {
    pub action: RuleAction,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os: Option<OsRule>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub features: Option<BTreeMap<String, bool>>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OsRule {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Os {
    Osx,
    Windows,
    Linux,
}

impl Os {
    pub fn current() -> Self {
        if cfg!(target_os = "macos") {
            Os::Osx
        } else if cfg!(target_os = "windows") {
            Os::Windows
        } else {
            Os::Linux
        }
    }
    /// Имя ОС в терминах JSON версий Mojang.
    pub fn mojang_name(self) -> &'static str {
        match self {
            Os::Osx => "osx",
            Os::Windows => "windows",
            Os::Linux => "linux",
        }
    }
    pub fn classpath_separator(self) -> &'static str {
        match self {
            Os::Windows => ";",
            _ => ":",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Arch {
    X86,
    X86_64,
    Arm64,
    Arm32,
}

impl Arch {
    pub fn current() -> Self {
        if cfg!(target_arch = "aarch64") {
            Arch::Arm64
        } else if cfg!(target_arch = "x86_64") {
            Arch::X86_64
        } else if cfg!(target_arch = "arm") {
            Arch::Arm32
        } else {
            Arch::X86
        }
    }
    /// Значение `os.arch` в правилах Mojang. В реальных JSON встречается только `x86`.
    pub fn mojang_name(self) -> &'static str {
        match self {
            Arch::X86 => "x86",
            Arch::X86_64 => "x86_64",
            Arch::Arm64 => "arm64",
            Arch::Arm32 => "arm32",
        }
    }
    /// Подстановка `${arch}` в старых классификаторах нативов (`natives-windows-${arch}`).
    pub fn bits(self) -> &'static str {
        match self {
            Arch::X86 | Arch::Arm32 => "32",
            Arch::X86_64 | Arch::Arm64 => "64",
        }
    }
}

/// Для чего проверяются правила: ОС, архитектура, версия ОС, включённые возможности.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Environment {
    pub os: Os,
    pub arch: Arch,
    pub os_version: String,
    pub features: BTreeSet<String>,
}

impl Environment {
    pub fn new(os: Os, arch: Arch, os_version: impl Into<String>) -> Self {
        Self {
            os,
            arch,
            os_version: os_version.into(),
            features: BTreeSet::new(),
        }
    }

    pub fn current() -> Self {
        Self::new(Os::current(), Arch::current(), detect_os_version())
    }

    pub fn with_feature(mut self, name: &str) -> Self {
        self.features.insert(name.to_string());
        self
    }

    pub fn allows(&self, rules: &[Rule]) -> bool {
        if rules.is_empty() {
            return true;
        }
        let mut allowed = false;
        for rule in rules {
            if self.matches(rule) {
                allowed = rule.action == RuleAction::Allow;
            }
        }
        allowed
    }

    fn matches(&self, rule: &Rule) -> bool {
        if let Some(os) = &rule.os {
            if let Some(name) = &os.name
                && name != self.os.mojang_name()
            {
                return false;
            }
            if let Some(arch) = &os.arch
                && arch != self.arch.mojang_name()
            {
                return false;
            }
            if let Some(pattern) = &os.version {
                // Битое выражение в чужом JSON — правило не совпадает, а не паника.
                match regex::Regex::new(pattern) {
                    Ok(re) if re.is_match(&self.os_version) => {}
                    _ => return false,
                }
            }
        }
        if let Some(features) = &rule.features {
            for (name, wanted) in features {
                if self.features.contains(name) != *wanted {
                    return false;
                }
            }
        }
        true
    }
}

/// Версия ОС для правил `os.version`: macOS — `sw_vers`, Linux — версия ядра,
/// Windows — `ver`. Нужна только для старых версий игры; при неудаче — пустая строка.
pub fn detect_os_version() -> String {
    let output = if cfg!(target_os = "macos") {
        std::process::Command::new("sw_vers")
            .arg("-productVersion")
            .output()
    } else if cfg!(target_os = "windows") {
        std::process::Command::new("cmd")
            .args(["/C", "ver"])
            .output()
    } else {
        std::process::Command::new("uname").arg("-r").output()
    };
    let text = match output {
        Ok(out) => String::from_utf8_lossy(&out.stdout).trim().to_string(),
        Err(_) => return String::new(),
    };
    // Windows: "Microsoft Windows [Version 10.0.19045.5371]"
    match text.find("Version ") {
        Some(pos) => text[pos + 8..].trim_end_matches(']').to_string(),
        None => text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(action: RuleAction, os: Option<OsRule>) -> Rule {
        Rule {
            action,
            os,
            features: None,
        }
    }
    fn os_name(name: &str) -> Option<OsRule> {
        Some(OsRule {
            name: Some(name.into()),
            ..Default::default()
        })
    }

    #[test]
    fn empty_rules_allow() {
        assert!(Environment::new(Os::Linux, Arch::X86_64, "").allows(&[]));
    }

    #[test]
    fn allow_all_except_osx() {
        let rules = [
            rule(RuleAction::Allow, None),
            rule(RuleAction::Disallow, os_name("osx")),
        ];
        assert!(Environment::new(Os::Windows, Arch::X86_64, "10.0").allows(&rules));
        assert!(!Environment::new(Os::Osx, Arch::Arm64, "15.1").allows(&rules));
    }

    #[test]
    fn only_on_osx() {
        let rules = [rule(RuleAction::Allow, os_name("osx"))];
        assert!(Environment::new(Os::Osx, Arch::Arm64, "15.1").allows(&rules));
        assert!(!Environment::new(Os::Linux, Arch::X86_64, "6.1").allows(&rules));
    }

    #[test]
    fn os_version_regex_from_1_6_4() {
        // lwjgl 2.9.0 в 1.6.4: разрешено везде, кроме macOS 10.5.x
        let rules = [
            rule(RuleAction::Allow, None),
            rule(
                RuleAction::Disallow,
                Some(OsRule {
                    name: Some("osx".into()),
                    version: Some(r"^10\.5\.\d$".into()),
                    ..Default::default()
                }),
            ),
        ];
        assert!(Environment::new(Os::Osx, Arch::Arm64, "15.1").allows(&rules));
        assert!(!Environment::new(Os::Osx, Arch::X86, "10.5.8").allows(&rules));
    }

    #[test]
    fn arch_rule() {
        let rules = [rule(
            RuleAction::Allow,
            Some(OsRule {
                arch: Some("x86".into()),
                ..Default::default()
            }),
        )];
        assert!(Environment::new(Os::Windows, Arch::X86, "").allows(&rules));
        assert!(!Environment::new(Os::Windows, Arch::X86_64, "").allows(&rules));
    }

    #[test]
    fn features_must_match() {
        let rules = [Rule {
            action: RuleAction::Allow,
            os: None,
            features: Some([("is_demo_user".to_string(), true)].into()),
        }];
        let env = Environment::new(Os::Linux, Arch::X86_64, "");
        assert!(!env.allows(&rules));
        assert!(env.with_feature("is_demo_user").allows(&rules));
    }

    #[test]
    fn broken_regex_does_not_match() {
        let rules = [rule(
            RuleAction::Allow,
            Some(OsRule {
                version: Some("([".into()),
                ..Default::default()
            }),
        )];
        assert!(!Environment::new(Os::Linux, Arch::X86_64, "6.1").allows(&rules));
    }
}
