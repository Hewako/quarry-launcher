//! Вывод игры с logging-конфигом Mojang приходит в stdout событиями log4j XML
//! (`<log4j:Event …><log4j:Message><![CDATA[…]]></log4j:Message></log4j:Event>`), как их читает
//! официальный лаунчер. Здесь события собираются в обычные строки
//! `[ЧЧ:ММ:СС] [поток/УРОВЕНЬ]: сообщение`. Строки вне событий проходят как есть.

use regex::Regex;
use std::sync::OnceLock;

#[derive(Debug, Default)]
pub struct Log4jAssembler {
    event: Option<String>,
}

fn attr_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"(\w+)="([^"]*)""#).expect("корректное выражение"))
}

fn cdata_re(tag: &str) -> Regex {
    Regex::new(&format!(
        r"(?s)<log4j:{tag}><!\[CDATA\[(.*?)\]\]></log4j:{tag}>"
    ))
    .expect("корректное выражение")
}

fn message_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| cdata_re("Message"))
}

fn throwable_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| cdata_re("Throwable"))
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

fn time_of(ms: &str) -> String {
    use chrono::TimeZone;
    ms.parse::<i64>()
        .ok()
        .and_then(|ms| chrono::Local.timestamp_millis_opt(ms).single())
        .map(|t| t.format("%H:%M:%S").to_string())
        .unwrap_or_else(|| "--:--:--".into())
}

/// Разобрать одно событие целиком.
pub fn format_event(event: &str) -> Vec<String> {
    let head_end = event.find('>').unwrap_or(event.len());
    let mut level = String::new();
    let mut thread = String::new();
    let mut timestamp = String::new();
    for c in attr_re().captures_iter(&event[..head_end]) {
        match &c[1] {
            "level" => level = c[2].to_string(),
            "thread" => thread = unescape(&c[2]),
            "timestamp" => timestamp = c[2].to_string(),
            _ => {}
        }
    }
    let message = message_re()
        .captures(event)
        .map(|c| c[1].to_string())
        .unwrap_or_default();
    let mut lines: Vec<String> = Vec::new();
    let prefix = format!("[{}] [{thread}/{level}]: ", time_of(&timestamp));
    let mut msg_lines = message.lines();
    lines.push(format!("{prefix}{}", msg_lines.next().unwrap_or("")));
    lines.extend(msg_lines.map(str::to_string));
    if let Some(t) = throwable_re().captures(event) {
        lines.extend(t[1].lines().map(str::to_string));
    }
    lines
}

impl Log4jAssembler {
    /// Добавить строку вывода; вернуть готовые строки (0, 1 или несколько).
    pub fn push(&mut self, line: &str) -> Vec<String> {
        if let Some(buf) = &mut self.event {
            buf.push('\n');
            buf.push_str(line);
            if line.contains("</log4j:Event>") {
                let event = self.event.take().unwrap_or_default();
                return format_event(&event);
            }
            return Vec::new();
        }
        let trimmed = line.trim_start();
        if trimmed.starts_with("<log4j:Event") {
            if trimmed.contains("</log4j:Event>") {
                return format_event(trimmed);
            }
            self.event = Some(trimmed.to_string());
            return Vec::new();
        }
        vec![line.to_string()]
    }

    /// Недособранное событие при завершении процесса — отдать как есть.
    pub fn flush(&mut self) -> Vec<String> {
        self.event.take().map(|e| vec![e]).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multi_line_event_becomes_one_line() {
        let mut a = Log4jAssembler::default();
        let input = [
            r#"  <log4j:Event logger="net.minecraft.client.Minecraft" timestamp="1792587133985" level="INFO" thread="Render thread">"#,
            "    <log4j:Message><![CDATA[Setting user: QuarryDev]]></log4j:Message>",
            "  </log4j:Event>",
        ];
        let mut out = Vec::new();
        for l in input {
            out.extend(a.push(l));
        }
        assert_eq!(out.len(), 1);
        assert!(
            out[0].ends_with("[Render thread/INFO]: Setting user: QuarryDev"),
            "{}",
            out[0]
        );
        assert!(out[0].starts_with('['));
    }

    #[test]
    fn throwable_and_plain_lines() {
        let mut a = Log4jAssembler::default();
        assert_eq!(a.push("plain stderr line"), ["plain stderr line"]);
        let ev = r#"<log4j:Event logger="x" timestamp="0" level="ERROR" thread="main &amp; co"><log4j:Message><![CDATA[Boom]]></log4j:Message><log4j:Throwable><![CDATA[java.lang.RuntimeException: x
	at a.b(C.java:1)]]></log4j:Throwable></log4j:Event>"#;
        let mut out = Vec::new();
        for l in ev.lines() {
            out.extend(a.push(l));
        }
        assert!(out[0].ends_with("[main & co/ERROR]: Boom"));
        assert_eq!(out[1], "java.lang.RuntimeException: x");
        assert_eq!(out[2], "\tat a.b(C.java:1)");
    }

    #[test]
    fn unfinished_event_is_flushed() {
        let mut a = Log4jAssembler::default();
        assert!(a.push(r#"<log4j:Event level="INFO">"#).is_empty());
        assert_eq!(a.flush().len(), 1);
    }
}
