//! Список серверов игры (`servers.dat`) и проверка сервера по протоколу Server List Ping.
//! Формат файла: minecraft.wiki/w/Servers.dat_format; протокол: minecraft.wiki/w/Java_Edition_protocol/Server_List_Ping.

use crate::error::{Error, IoContext, Result};
use crate::nbt::{self, Tag};
use crate::paths::write_atomic;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Server {
    pub name: String,
    /// Адрес как в игре: `host`, `host:port` или `[ipv6]:port`.
    pub ip: String,
    /// Иконка сервера (PNG в base64), которую сохранила игра.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// Ресурспак сервера: `Some(true)` — принимать, `Some(false)` — отклонять, `None` — спрашивать.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accept_textures: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Status {
    pub version: String,
    pub protocol: i64,
    pub online: i64,
    pub max: i64,
    /// Описание сервера (MOTD) без кодов форматирования.
    pub motd: String,
    pub latency_ms: u64,
    /// Иконка `data:image/png;base64,…`.
    pub favicon: Option<String>,
}

pub fn read(game_dir: &Path) -> Result<Vec<Server>> {
    let path = game_dir.join("servers.dat");
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(Error::io(&path, e)),
    };
    let root = nbt::read(&bytes)?;
    let list = root.get("servers").and_then(|t| t.as_list()).unwrap_or(&[]);
    Ok(list
        .iter()
        .filter_map(|s| {
            Some(Server {
                name: s
                    .get("name")
                    .and_then(|t| t.as_str())
                    .unwrap_or("Сервер Minecraft")
                    .to_string(),
                ip: s.get("ip")?.as_str()?.to_string(),
                icon: s.get("icon").and_then(|t| t.as_str()).map(str::to_string),
                accept_textures: s
                    .get("acceptTextures")
                    .and_then(|t| t.as_i64())
                    .map(|v| v != 0),
            })
        })
        .collect())
}

/// Записать список. Прежний файл сохраняется как `servers.dat_old` (так же делает игра).
pub fn write(game_dir: &Path, servers: &[Server]) -> Result<()> {
    for s in servers {
        if s.name.trim().is_empty() || s.name.chars().count() > 64 {
            return Err(Error::invalid("Название сервера — от 1 до 64 символов"));
        }
        parse_address(&s.ip)?;
    }
    let list = servers
        .iter()
        .map(|s| {
            let mut m = BTreeMap::new();
            m.insert("name".to_string(), Tag::String(s.name.trim().to_string()));
            m.insert("ip".to_string(), Tag::String(s.ip.trim().to_string()));
            if let Some(i) = &s.icon {
                m.insert("icon".to_string(), Tag::String(i.clone()));
            }
            if let Some(a) = s.accept_textures {
                m.insert("acceptTextures".to_string(), Tag::Byte(a as i8));
            }
            Tag::Compound(m)
        })
        .collect();
    let root = Tag::Compound(BTreeMap::from([("servers".to_string(), Tag::List(list))]));
    std::fs::create_dir_all(game_dir).at(game_dir)?;
    let path = game_dir.join("servers.dat");
    if path.is_file() {
        let old = game_dir.join("servers.dat_old");
        std::fs::copy(&path, &old).at(&old)?;
    }
    write_atomic(&path, &nbt::write(&root))
}

const TAGS: &str = "server-tags.toml";

/// Теги серверов профиля (адрес → теги). Хранятся отдельно: игра перезаписывает servers.dat.
pub fn load_tags(instance_dir: &Path) -> BTreeMap<String, Vec<String>> {
    std::fs::read_to_string(instance_dir.join(TAGS))
        .ok()
        .and_then(|t| toml::from_str(&t).ok())
        .unwrap_or_default()
}

pub fn set_tags(instance_dir: &Path, ip: &str, tags: &[String]) -> Result<()> {
    let mut all = load_tags(instance_dir);
    let clean: Vec<String> = tags
        .iter()
        .map(|t| t.trim().chars().take(32).collect::<String>())
        .filter(|t| !t.is_empty())
        .take(10)
        .collect();
    if clean.is_empty() {
        all.remove(ip);
    } else {
        all.insert(ip.to_string(), clean);
    }
    let text = toml::to_string_pretty(&all).map_err(|e| Error::parse(TAGS, e))?;
    write_atomic(&instance_dir.join(TAGS), text.as_bytes())
}

/// Разобрать адрес сервера на хост и порт (по умолчанию 25565).
pub fn parse_address(addr: &str) -> Result<(String, u16)> {
    let addr = addr.trim();
    let bad = || Error::invalid(format!("Непонятный адрес сервера: {addr:?}"));
    if addr.is_empty() || addr.len() > 255 || addr.chars().any(|c| c.is_whitespace() || c == '/') {
        return Err(bad());
    }
    if let Some(rest) = addr.strip_prefix('[') {
        let (host, tail) = rest.split_once(']').ok_or_else(bad)?;
        let port = match tail.strip_prefix(':') {
            Some(p) => p.parse().map_err(|_| bad())?,
            None if tail.is_empty() => 25565,
            None => return Err(bad()),
        };
        return Ok((host.to_string(), port));
    }
    match addr.rsplit_once(':') {
        Some((host, port)) if !host.contains(':') => {
            Ok((host.to_string(), port.parse().map_err(|_| bad())?))
        }
        // IPv6 без скобок или адрес без порта.
        _ => Ok((addr.to_string(), 25565)),
    }
}

fn varint(mut v: i32, out: &mut Vec<u8>) {
    loop {
        let b = (v & 0x7f) as u8;
        v = ((v as u32) >> 7) as i32;
        if v == 0 {
            out.push(b);
            return;
        }
        out.push(b | 0x80);
    }
}

fn packet(body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(body.len() + 5);
    varint(body.len() as i32, &mut out);
    out.extend_from_slice(body);
    out
}

async fn read_varint<R: AsyncReadExt + Unpin>(r: &mut R) -> Result<i32> {
    let mut v: i32 = 0;
    for i in 0..5 {
        let b = r.read_u8().await.map_err(net_err)?;
        v |= ((b & 0x7f) as i32) << (7 * i);
        if b & 0x80 == 0 {
            return Ok(v);
        }
    }
    Err(Error::parse("ответ сервера", "слишком длинное число"))
}

fn net_err(e: std::io::Error) -> Error {
    Error::invalid(format!("Сервер не ответил: {e}"))
}

/// Опросить сервер: версия, игроки, описание, задержка. Таймаут — 5 секунд.
/// SRV-записи DNS не учитываются: такой сервер может не ответить здесь, но открыться в игре.
pub async fn ping(addr: &str) -> Result<Status> {
    let (host, port) = parse_address(addr)?;
    tokio::time::timeout(Duration::from_secs(5), ping_inner(&host, port))
        .await
        .map_err(|_| Error::invalid("Сервер не ответил за 5 секунд"))?
}

async fn ping_inner(host: &str, port: u16) -> Result<Status> {
    let mut s = tokio::net::TcpStream::connect((host, port))
        .await
        .map_err(net_err)?;
    let mut hs = vec![0x00];
    varint(-1, &mut hs); // для запроса статуса версия протокола не важна
    varint(host.len() as i32, &mut hs);
    hs.extend_from_slice(host.as_bytes());
    hs.extend_from_slice(&port.to_be_bytes());
    varint(1, &mut hs);
    let mut req = packet(&hs);
    req.extend(packet(&[0x00]));
    s.write_all(&req).await.map_err(net_err)?;

    let len = read_varint(&mut s).await?;
    if !(1..=2 * 1024 * 1024).contains(&len) {
        return Err(Error::parse("ответ сервера", "неверная длина"));
    }
    let mut body = vec![0; len as usize];
    s.read_exact(&mut body).await.map_err(net_err)?;
    let mut cur = std::io::Cursor::new(&body[..]);
    if read_varint(&mut cur).await? != 0 {
        return Err(Error::parse("ответ сервера", "не ответ на запрос статуса"));
    }
    let n = read_varint(&mut cur).await? as usize;
    let start = cur.position() as usize;
    let json = body
        .get(start..start.saturating_add(n))
        .ok_or_else(|| Error::parse("ответ сервера", "обрывается"))?;
    let v: serde_json::Value =
        serde_json::from_slice(json).map_err(|e| Error::parse("ответ сервера", e))?;

    let started = Instant::now();
    let mut ping = vec![0x01];
    ping.extend_from_slice(&1i64.to_be_bytes());
    let mut pong = [0u8; 10];
    let latency_ms = match s.write_all(&packet(&ping)).await {
        Ok(()) => match tokio::time::timeout(Duration::from_secs(2), s.read_exact(&mut pong)).await
        {
            Ok(Ok(_)) => started.elapsed().as_millis() as u64,
            _ => 0,
        },
        Err(_) => 0,
    };
    Ok(Status {
        version: v["version"]["name"].as_str().unwrap_or("?").to_string(),
        protocol: v["version"]["protocol"].as_i64().unwrap_or(-1),
        online: v["players"]["online"].as_i64().unwrap_or(0),
        max: v["players"]["max"].as_i64().unwrap_or(0),
        motd: plain_text(&v["description"]).trim().to_string(),
        latency_ms,
        favicon: v["favicon"]
            .as_str()
            .filter(|f| f.starts_with("data:image/png;base64,"))
            .map(str::to_string),
    })
}

/// Текстовый компонент Minecraft → обычный текст без кодов форматирования `§x`.
pub fn plain_text(v: &serde_json::Value) -> String {
    fn walk(v: &serde_json::Value, out: &mut String) {
        match v {
            serde_json::Value::String(s) => out.push_str(s),
            serde_json::Value::Array(a) => a.iter().for_each(|x| walk(x, out)),
            serde_json::Value::Object(o) => {
                if let Some(t) = o.get("text") {
                    walk(t, out);
                }
                if let Some(e) = o.get("extra") {
                    walk(e, out);
                }
            }
            _ => {}
        }
    }
    let mut raw = String::new();
    walk(v, &mut raw);
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        if c == '§' {
            chars.next();
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[test]
    fn address_default_port_and_ipv6() {
        assert_eq!(
            parse_address("play.example.org").unwrap(),
            ("play.example.org".into(), 25565)
        );
        assert_eq!(
            parse_address("1.2.3.4:25570").unwrap(),
            ("1.2.3.4".into(), 25570)
        );
        assert_eq!(parse_address("[::1]:25566").unwrap(), ("::1".into(), 25566));
    }

    #[test]
    fn address_rejects_garbage() {
        assert!(parse_address("host:notaport").is_err());
        assert!(parse_address("a b").is_err());
        assert!(parse_address("").is_err());
    }

    #[test]
    fn servers_dat_roundtrip_keeps_old_copy() {
        let d = std::env::temp_dir().join(format!("quarry-servers-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        assert!(read(&d).unwrap().is_empty());
        let list = vec![
            Server {
                name: "Друзья".into(),
                ip: "mc.example.org".into(),
                icon: None,
                accept_textures: Some(true),
            },
            Server {
                name: "Локальный".into(),
                ip: "127.0.0.1:25566".into(),
                icon: Some("iVBOR".into()),
                accept_textures: None,
            },
        ];
        write(&d, &list).unwrap();
        assert_eq!(read(&d).unwrap(), list);
        write(&d, &list[..1]).unwrap();
        assert!(d.join("servers.dat_old").is_file());
        assert!(
            write(
                &d,
                &[Server {
                    name: " ".into(),
                    ..list[0].clone()
                }]
            )
            .is_err()
        );
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn motd_component_to_plain_text() {
        let v: serde_json::Value =
            serde_json::from_str(r#"{"text":"§aПривет ","extra":[{"text":"мир"},"!"]}"#).unwrap();
        assert_eq!(plain_text(&v), "Привет мир!");
    }

    /// Минимальный сервер: читает рукопожатие и запрос, отвечает статусом и понгом.
    async fn fake_server(json: &'static str) -> u16 {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            let (mut c, _) = listener.accept().await.unwrap();
            for _ in 0..2 {
                let n = c.read_u8().await.unwrap(); // пакеты короче 128 байт
                let mut b = vec![0; n as usize];
                c.read_exact(&mut b).await.unwrap();
            }
            let mut body = vec![0x00, json.len() as u8];
            body.extend_from_slice(json.as_bytes());
            let mut out = vec![body.len() as u8];
            out.extend(body);
            c.write_all(&out).await.unwrap();
            let mut ping = [0u8; 10];
            c.read_exact(&mut ping).await.unwrap();
            c.write_all(&ping).await.unwrap();
        });
        port
    }

    #[tokio::test]
    async fn ping_reads_status() {
        let port = fake_server(r#"{"version":{"name":"26.3","protocol":800},"players":{"max":20,"online":3},"description":"§eТест"}"#).await;
        let s = ping(&format!("127.0.0.1:{port}")).await.unwrap();
        assert_eq!(
            (s.version.as_str(), s.online, s.max, s.motd.as_str()),
            ("26.3", 3, 20, "Тест")
        );
    }

    #[tokio::test]
    async fn ping_closed_port_is_error() {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        drop(l);
        assert!(ping(&format!("127.0.0.1:{port}")).await.is_err());
    }

    #[test]
    fn tags_live_next_to_servers_dat() {
        let d = std::env::temp_dir().join(format!("quarry-srvtags-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        assert!(load_tags(&d).is_empty());
        set_tags(
            &d,
            "mc.example.org",
            &[" друзья ".into(), "".into(), "выживание".into()],
        )
        .unwrap();
        assert_eq!(load_tags(&d)["mc.example.org"], ["друзья", "выживание"]);
        set_tags(&d, "mc.example.org", &[]).unwrap();
        assert!(load_tags(&d).is_empty(), "пустые теги не хранятся");
        std::fs::remove_dir_all(&d).unwrap();
    }
}
