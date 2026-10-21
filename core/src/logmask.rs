//! Маскирование секретов и персональных данных в логах — при записи, до попадания в файл,
//! на экран и в отчёт.

use regex::Regex;
use std::sync::OnceLock;

pub const TOKEN: &str = "‹токен замаскирован›";
pub const HOME: &str = "‹HOME›";
pub const IP: &str = "‹IP›";
pub const NICK: &str = "‹ник скрыт›";

#[derive(Debug, Clone, Default)]
pub struct Masker {
    secrets: Vec<String>,
    home: Option<String>,
    /// Ник маскируется только в режиме стримера.
    nick: Option<String>,
}

/// Кандидат в IPv4 как отдельное «слово»: не часть `jdk-25.0.4.1+1` или `lib-1.2.3.4.jar`.
/// Окончательное решение — по контексту (`looks_like_address`): строка вида `26.3.0.95`
/// одинаково похожа и на адрес, и на номер версии.
fn ipv4() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(^|[^0-9A-Za-z_.+\-])((?:(?:25[0-5]|2[0-4]\d|1?\d?\d)\.){3}(?:25[0-5]|2[0-4]\d|1?\d?\d))($|[^0-9A-Za-z_.+\-]|\.$|\.[^0-9A-Za-z])",
        )
        .expect("регулярное выражение IPv4 корректно")
    })
}

/// Длинные JWT-подобные строки (`eyJ…`) и значения после `accessToken`/`token=`.
fn token_like() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)(eyJ[A-Za-z0-9_\-]{10,}\.[A-Za-z0-9_\-]{10,}\.[A-Za-z0-9_\-]*)|((?:access_?token|refresh_?token|session|token)[=:\s]+)([A-Za-z0-9_\-\.]{16,})",
        )
        .expect("регулярное выражение токенов корректно")
    })
}

impl Masker {
    pub fn new() -> Self {
        Self {
            secrets: Vec::new(),
            home: dirs::home_dir().map(|h| h.to_string_lossy().into_owned()),
            nick: None,
        }
    }

    /// Секрет (токен) — заменяется везде, где встретится. Слишком короткие строки не
    /// добавляются, чтобы не «маскировать» обычные слова.
    pub fn with_secret(mut self, secret: &str) -> Self {
        if secret.len() >= 8 {
            self.secrets.push(secret.to_string());
        }
        self
    }

    pub fn with_home(mut self, home: Option<String>) -> Self {
        self.home = home.filter(|h| h.len() > 1);
        self
    }

    pub fn with_nick(mut self, nick: Option<&str>) -> Self {
        self.nick = nick.filter(|n| n.len() >= 3).map(str::to_string);
        self
    }

    pub fn mask(&self, line: &str) -> String {
        let mut out = line.to_string();
        for s in &self.secrets {
            if out.contains(s.as_str()) {
                out = out.replace(s.as_str(), TOKEN);
            }
        }
        out = token_like()
            .replace_all(&out, |c: &regex::Captures| {
                if c.get(1).is_some() {
                    TOKEN.to_string()
                } else {
                    format!("{}{}", &c[2], TOKEN)
                }
            })
            .into_owned();
        if let Some(home) = &self.home {
            out = out.replace(home.as_str(), HOME);
        }
        out = mask_addresses(&out);
        if let Some(nick) = &self.nick {
            out = out.replace(nick.as_str(), NICK);
        }
        out
    }
}

/// Адрес — если рядом порт (`:25565`), это `InetAddress` (`/1.2.3.4`, но не папка `/1.2.3.4/`)
/// или перед ним слово про сеть. Иначе это скорее номер версии, и он остаётся как есть.
fn looks_like_address(line: &str, start: usize, end: usize) -> bool {
    let before = &line[..start];
    let after = &line[end..];
    if after.starts_with(':') && after[1..].starts_with(|c: char| c.is_ascii_digit()) {
        return true;
    }
    if before.ends_with('/') && !after.starts_with('/') {
        return true;
    }
    let window: String = before
        .chars()
        .rev()
        .take(24)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    let window = window.to_lowercase();
    [
        "connect", "server", "address", "addr", "host", "ip", "peer", "remote", "client", "lan",
    ]
    .iter()
    .any(|k| window.contains(k))
}

fn mask_addresses(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut last = 0;
    for c in ipv4().captures_iter(line) {
        let m = c.get(2).expect("группа 2 есть всегда");
        let ip = m.as_str();
        // Loopback и 0.0.0.0 не персональные.
        if ip.starts_with("127.")
            || ip == "0.0.0.0"
            || !looks_like_address(line, m.start(), m.end())
        {
            continue;
        }
        out.push_str(&line[last..m.start()]);
        out.push_str(IP);
        last = m.end();
    }
    out.push_str(&line[last..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_secret_is_masked_everywhere() {
        let m = Masker::new()
            .with_home(None)
            .with_secret("supersecrettoken123");
        assert_eq!(
            m.mask("--accessToken supersecrettoken123 and again supersecrettoken123"),
            format!("--accessToken {TOKEN} and again {TOKEN}")
        );
    }

    #[test]
    fn jwt_and_token_assignments_are_masked() {
        let m = Masker::new().with_home(None);
        let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.abc";
        assert!(!m.mask(&format!("Session: {jwt}")).contains("eyJ"));
        assert_eq!(
            m.mask("token=abcdefghijklmnopqrstuvwxyz"),
            format!("token={TOKEN}")
        );
    }

    #[test]
    fn home_and_ip() {
        let m = Masker::new().with_home(Some("/Users/steve".into()));
        assert_eq!(
            m.mask("Game directory: /Users/steve/.quarry/instances/x"),
            format!("Game directory: {HOME}/.quarry/instances/x")
        );
        assert_eq!(
            m.mask("Connecting to 93.184.216.34, 25565"),
            format!("Connecting to {IP}, 25565")
        );
        assert_eq!(m.mask("bind 127.0.0.1:25565"), "bind 127.0.0.1:25565");
        assert_eq!(m.mask("/93.184.216.34:25565"), format!("/{IP}:25565"));
        assert_eq!(m.mask("server 10.0.0.5."), format!("server {IP}."));
    }

    #[test]
    fn versions_are_not_ips() {
        let m = Masker::new().with_home(None);
        for s in [
            "NeoForge 26.3.0.95 (neoforge)",
            "~libraries/net/neoforged/neoforge/26.3.0.95/neoforge.jar",
            "jdk-25.0.4.1+1/bin/java",
            "lib-1.2.3.4.jar",
            "Fabric 0.16.10.1-beta",
            "v1.2.3.4",
        ] {
            assert_eq!(m.mask(s), s);
        }
    }

    #[test]
    fn short_strings_are_not_secrets() {
        let m = Masker::new().with_home(None).with_secret("0");
        assert_eq!(m.mask("version 1.20"), "version 1.20");
    }

    #[test]
    fn nick_only_in_streamer_mode() {
        let m = Masker::new().with_home(None);
        assert_eq!(
            m.mask("Setting user: Steve_Builder"),
            "Setting user: Steve_Builder"
        );
        let m = m.with_nick(Some("Steve_Builder"));
        assert_eq!(
            m.mask("Setting user: Steve_Builder"),
            format!("Setting user: {NICK}")
        );
    }
}
