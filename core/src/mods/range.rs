//! Проверка «версия подходит под диапазон» для зависимостей модов.
//! Fabric/Quilt: `*`, `1.2.3`, `>=1.2`, `<2`, `~1.21`, `^0.8`, `1.21.x`, пробел — «и», `||` — «или».
//! Maven (Forge/NeoForge): `[1.21.1,1.22)`, `(,2.0]`, `[1.0]`, несколько через запятую.
//! `None` — диапазон не понят: такой случай не выдаётся ни за «подходит», ни за «не подходит».

use std::cmp::Ordering;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    nums: Vec<u64>,
    pre: Option<String>,
}

impl Version {
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim().trim_start_matches('v');
        let s = s.split('+').next()?; // метаданные сборки не участвуют в сравнении
        let (main, pre) = match s.split_once('-') {
            Some((m, p)) => (m, Some(p.to_string())),
            None => (s, None),
        };
        let nums: Option<Vec<u64>> = main.split('.').map(|p| p.parse().ok()).collect();
        let nums = nums?;
        (!nums.is_empty()).then_some(Self { nums, pre })
    }

    fn get(&self, i: usize) -> u64 {
        self.nums.get(i).copied().unwrap_or(0)
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        let n = self.nums.len().max(other.nums.len());
        for i in 0..n {
            match self.get(i).cmp(&other.get(i)) {
                Ordering::Equal => {}
                o => return o,
            }
        }
        match (&self.pre, &other.pre) {
            (None, None) => Ordering::Equal,
            (None, Some(_)) => Ordering::Greater,
            (Some(_), None) => Ordering::Less,
            (Some(a), Some(b)) => a.cmp(b),
        }
    }
}

/// Подходит ли версия под диапазон.
pub fn matches(version: &str, range: &str) -> Option<bool> {
    let range = range.trim();
    if range.is_empty() || range == "*" {
        return Some(true);
    }
    let v = Version::parse(version)?;
    if range.starts_with('[') || range.starts_with('(') {
        return maven(&v, range);
    }
    // Любая из альтернатив.
    let mut any_unknown = false;
    for alt in range.split("||") {
        match all_of(&v, alt) {
            Some(true) => return Some(true),
            Some(false) => {}
            None => any_unknown = true,
        }
    }
    if any_unknown { None } else { Some(false) }
}

fn all_of(v: &Version, alt: &str) -> Option<bool> {
    let mut ok = true;
    for c in alt.split_whitespace() {
        ok &= single(v, c)?;
    }
    Some(ok)
}

fn single(v: &Version, c: &str) -> Option<bool> {
    if c == "*" || c == "x" {
        return Some(true);
    }
    for (op, f) in [
        (">=", Ordering::is_ge as fn(Ordering) -> bool),
        ("<=", Ordering::is_le),
        (">", Ordering::is_gt),
        ("<", Ordering::is_lt),
        ("=", Ordering::is_eq),
    ] {
        if let Some(rest) = c.strip_prefix(op) {
            let other = Version::parse(&rest.replace(".x", ""))?;
            return Some(f(v.cmp(&other)));
        }
    }
    if let Some(rest) = c.strip_prefix('~') {
        // ~1.21.4 — та же минорная: >=1.21.4 <1.22
        let base = Version::parse(rest)?;
        let upper = Version {
            nums: vec![base.get(0), base.get(1) + 1],
            pre: None,
        };
        return Some(v >= &base && v < &upper);
    }
    if let Some(rest) = c.strip_prefix('^') {
        let base = Version::parse(rest)?;
        let upper = if base.get(0) > 0 {
            Version {
                nums: vec![base.get(0) + 1],
                pre: None,
            }
        } else {
            Version {
                nums: vec![0, base.get(1) + 1],
                pre: None,
            }
        };
        return Some(v >= &base && v < &upper);
    }
    // 1.21.x / 1.21.* — совпадение по префиксу.
    if c.ends_with(".x") || c.ends_with(".*") || c.ends_with(".X") {
        let prefix: Option<Vec<u64>> = c[..c.len() - 2]
            .split('.')
            .map(|p| p.parse().ok())
            .collect();
        let prefix = prefix?;
        return Some(prefix.iter().enumerate().all(|(i, p)| v.get(i) == *p));
    }
    let exact = Version::parse(c)?;
    Some(v.cmp(&exact) == Ordering::Equal)
}

/// Диапазоны Maven: `[a,b)`, `(,b]`, `[a]`; несколько диапазонов через запятую — «или».
fn maven(v: &Version, range: &str) -> Option<bool> {
    let mut rest = range;
    let mut any = false;
    while !rest.is_empty() {
        let open = rest.chars().next()?;
        let close_pos = rest.find([']', ')'])?;
        let close = rest[close_pos..].chars().next()?;
        let body = &rest[1..close_pos];
        let ok = match body.split_once(',') {
            None => v.cmp(&Version::parse(body)?) == Ordering::Equal,
            Some((lo, hi)) => {
                let lo_ok = if lo.trim().is_empty() {
                    true
                } else {
                    let l = Version::parse(lo)?;
                    if open == '[' { v >= &l } else { v > &l }
                };
                let hi_ok = if hi.trim().is_empty() {
                    true
                } else {
                    let h = Version::parse(hi)?;
                    if close == ']' { v <= &h } else { v < &h }
                };
                lo_ok && hi_ok
            }
        };
        any |= ok;
        rest = rest[close_pos + 1..].trim_start_matches(',').trim();
    }
    Some(any)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fabric_ranges() {
        assert_eq!(matches("0.7.2", ">=0.7.3"), Some(false));
        assert_eq!(matches("0.7.3", ">=0.7.3"), Some(true));
        assert_eq!(matches("1.21.11", "~1.21.4"), Some(true));
        assert_eq!(matches("1.22", "~1.21.4"), Some(false));
        assert_eq!(matches("0.8.15", "0.8.x || 0.9.x"), Some(true));
        assert_eq!(matches("0.10.0", "0.8.x || 0.9.x"), Some(false));
        assert_eq!(matches("0.17.2", ">=0.16 <0.18"), Some(true));
        assert_eq!(
            matches("1.0.0-beta.2", ">=1.0.0"),
            Some(false),
            "пред-релиз младше релиза"
        );
        assert_eq!(matches("0.8.15+mc1.21.11", "^0.8"), Some(true));
        assert_eq!(
            matches("26.3", ">=1.21"),
            Some(true),
            "схема 26.x сравнивается как числа"
        );
        assert_eq!(matches("1.21.11", "*"), Some(true));
    }

    #[test]
    fn maven_ranges() {
        assert_eq!(matches("1.21.1", "[1.21.1,1.22)"), Some(true));
        assert_eq!(matches("1.22", "[1.21.1,1.22)"), Some(false));
        assert_eq!(matches("2.0", "(,2.0]"), Some(true));
        assert_eq!(matches("1.5", "[1.5]"), Some(true));
        assert_eq!(matches("3.0", "[1.0,2.0),[3.0,4.0)"), Some(true));
    }

    #[test]
    fn unknown_is_not_a_verdict() {
        assert_eq!(matches("abc", ">=1.0"), None);
        assert_eq!(matches("1.0", ">=wat"), None);
        assert_eq!(matches("1.0", "[1.0"), None);
    }
}
