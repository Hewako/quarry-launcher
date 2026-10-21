//! Чтение NBT (формат данных Minecraft: level.dat, servers.dat). Только чтение и запись
//! простых файлов вроде servers.dat; большие числа и порядок байтов — big-endian.
//! Формат: minecraft.wiki/w/NBT_format (теги 0–12).

use crate::error::{Error, Result};
use std::collections::BTreeMap;
use std::io::Read;

#[derive(Debug, Clone, PartialEq)]
pub enum Tag {
    Byte(i8),
    Short(i16),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    ByteArray(Vec<i8>),
    String(String),
    List(Vec<Tag>),
    Compound(BTreeMap<String, Tag>),
    IntArray(Vec<i32>),
    LongArray(Vec<i64>),
}

impl Tag {
    pub fn get(&self, key: &str) -> Option<&Tag> {
        match self {
            Tag::Compound(m) => m.get(key),
            _ => None,
        }
    }
    /// Путь через точку: `Data.Version.Name`.
    pub fn path(&self, path: &str) -> Option<&Tag> {
        path.split('.').try_fold(self, |t, k| t.get(k))
    }
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Tag::String(s) => Some(s),
            _ => None,
        }
    }
    pub fn as_i64(&self) -> Option<i64> {
        match *self {
            Tag::Byte(v) => Some(v as i64),
            Tag::Short(v) => Some(v as i64),
            Tag::Int(v) => Some(v as i64),
            Tag::Long(v) => Some(v),
            _ => None,
        }
    }
    pub fn as_list(&self) -> Option<&[Tag]> {
        match self {
            Tag::List(v) => Some(v),
            _ => None,
        }
    }
}

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
    depth: usize,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8]> {
        if self.pos + n > self.data.len() {
            return Err(Error::parse("NBT", "файл обрывается"));
        }
        let s = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn i16(&mut self) -> Result<i16> {
        Ok(i16::from_be_bytes(
            self.take(2)?.try_into().expect("2 байта"),
        ))
    }
    fn i32(&mut self) -> Result<i32> {
        Ok(i32::from_be_bytes(
            self.take(4)?.try_into().expect("4 байта"),
        ))
    }
    fn i64(&mut self) -> Result<i64> {
        Ok(i64::from_be_bytes(
            self.take(8)?.try_into().expect("8 байт"),
        ))
    }
    fn len(&mut self) -> Result<usize> {
        let n = self.i32()?;
        if n < 0 || n as usize > self.data.len() {
            return Err(Error::parse("NBT", "неверная длина"));
        }
        Ok(n as usize)
    }
    fn string(&mut self) -> Result<String> {
        let n = self.i16()? as u16 as usize;
        // Java «modified UTF-8»; для обычных имён совпадает с UTF-8.
        Ok(String::from_utf8_lossy(self.take(n)?).into_owned())
    }
    fn payload(&mut self, id: u8) -> Result<Tag> {
        self.depth += 1;
        if self.depth > 512 {
            return Err(Error::parse("NBT", "слишком глубокая вложенность"));
        }
        let t = match id {
            1 => Tag::Byte(self.u8()? as i8),
            2 => Tag::Short(self.i16()?),
            3 => Tag::Int(self.i32()?),
            4 => Tag::Long(self.i64()?),
            5 => Tag::Float(f32::from_bits(self.i32()? as u32)),
            6 => Tag::Double(f64::from_bits(self.i64()? as u64)),
            7 => {
                let n = self.len()?;
                Tag::ByteArray(self.take(n)?.iter().map(|b| *b as i8).collect())
            }
            8 => Tag::String(self.string()?),
            9 => {
                let item = self.u8()?;
                let n = self.len()?;
                let mut v = Vec::with_capacity(n.min(4096));
                for _ in 0..n {
                    v.push(self.payload(item)?);
                }
                Tag::List(v)
            }
            10 => {
                let mut m = BTreeMap::new();
                loop {
                    let tid = self.u8()?;
                    if tid == 0 {
                        break;
                    }
                    let name = self.string()?;
                    m.insert(name, self.payload(tid)?);
                }
                Tag::Compound(m)
            }
            11 => {
                let n = self.len()?;
                Tag::IntArray((0..n).map(|_| self.i32()).collect::<Result<_>>()?)
            }
            12 => {
                let n = self.len()?;
                Tag::LongArray((0..n).map(|_| self.i64()).collect::<Result<_>>()?)
            }
            other => return Err(Error::parse("NBT", format!("неизвестный тег {other}"))),
        };
        self.depth -= 1;
        Ok(t)
    }
}

/// Прочитать NBT (сжатый gzip или без сжатия). Возвращает корневой Compound.
pub fn read(bytes: &[u8]) -> Result<Tag> {
    let raw: Vec<u8>;
    let data = if bytes.starts_with(&[0x1f, 0x8b]) {
        let mut out = Vec::new();
        flate2::read::GzDecoder::new(bytes)
            .take(256 * 1024 * 1024)
            .read_to_end(&mut out)
            .map_err(|e| Error::parse("NBT", e))?;
        raw = out;
        &raw[..]
    } else {
        bytes
    };
    let mut r = Reader {
        data,
        pos: 0,
        depth: 0,
    };
    let id = r.u8()?;
    if id != 10 {
        return Err(Error::parse("NBT", "корень — не Compound"));
    }
    let _name = r.string()?;
    r.payload(10)
}

/// Записать корневой Compound без сжатия (нужно для servers.dat).
pub fn write(root: &Tag) -> Vec<u8> {
    fn id(t: &Tag) -> u8 {
        match t {
            Tag::Byte(_) => 1,
            Tag::Short(_) => 2,
            Tag::Int(_) => 3,
            Tag::Long(_) => 4,
            Tag::Float(_) => 5,
            Tag::Double(_) => 6,
            Tag::ByteArray(_) => 7,
            Tag::String(_) => 8,
            Tag::List(_) => 9,
            Tag::Compound(_) => 10,
            Tag::IntArray(_) => 11,
            Tag::LongArray(_) => 12,
        }
    }
    fn s(out: &mut Vec<u8>, v: &str) {
        let b = v.as_bytes();
        out.extend((b.len() as u16).to_be_bytes());
        out.extend(b);
    }
    fn p(out: &mut Vec<u8>, t: &Tag) {
        match t {
            Tag::Byte(v) => out.push(*v as u8),
            Tag::Short(v) => out.extend(v.to_be_bytes()),
            Tag::Int(v) => out.extend(v.to_be_bytes()),
            Tag::Long(v) => out.extend(v.to_be_bytes()),
            Tag::Float(v) => out.extend(v.to_bits().to_be_bytes()),
            Tag::Double(v) => out.extend(v.to_bits().to_be_bytes()),
            Tag::ByteArray(v) => {
                out.extend((v.len() as i32).to_be_bytes());
                out.extend(v.iter().map(|b| *b as u8));
            }
            Tag::String(v) => s(out, v),
            Tag::List(v) => {
                out.push(v.first().map(id).unwrap_or(0));
                out.extend((v.len() as i32).to_be_bytes());
                for x in v {
                    p(out, x);
                }
            }
            Tag::Compound(m) => {
                for (k, v) in m {
                    out.push(id(v));
                    s(out, k);
                    p(out, v);
                }
                out.push(0);
            }
            Tag::IntArray(v) => {
                out.extend((v.len() as i32).to_be_bytes());
                for x in v {
                    out.extend(x.to_be_bytes());
                }
            }
            Tag::LongArray(v) => {
                out.extend((v.len() as i32).to_be_bytes());
                for x in v {
                    out.extend(x.to_be_bytes());
                }
            }
        }
    }
    let mut out = vec![10];
    s(&mut out, "");
    p(&mut out, root);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    pub fn compound(items: Vec<(&str, Tag)>) -> Tag {
        Tag::Compound(items.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
    }

    #[test]
    fn roundtrip_and_gzip() {
        let root = compound(vec![(
            "Data",
            compound(vec![
                ("LevelName", Tag::String("База у реки".into())),
                ("GameType", Tag::Int(0)),
                ("LastPlayed", Tag::Long(1_792_000_000_000)),
                (
                    "Version",
                    compound(vec![
                        ("Name", Tag::String("1.21.11".into())),
                        ("Id", Tag::Int(4671)),
                    ]),
                ),
                ("DataVersion", Tag::Int(4671)),
                ("Pos", Tag::List(vec![Tag::Double(1.5), Tag::Double(64.0)])),
            ]),
        )]);
        let raw = write(&root);
        assert_eq!(read(&raw).unwrap(), root);
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        gz.write_all(&raw).unwrap();
        let t = read(&gz.finish().unwrap()).unwrap();
        assert_eq!(
            t.path("Data.Version.Name").and_then(|t| t.as_str()),
            Some("1.21.11")
        );
        assert_eq!(
            t.path("Data.LastPlayed").and_then(|t| t.as_i64()),
            Some(1_792_000_000_000)
        );
    }

    #[test]
    fn broken_input_is_an_error_not_a_panic() {
        assert!(read(&[]).is_err());
        assert!(read(&[10, 0, 0, 8]).is_err());
        assert!(read(&[10, 0, 0, 9, 0, 1, b'x', 1, 0x7f, 0xff, 0xff, 0xff]).is_err());
        assert!(read(&[1, 0, 0, 5]).is_err());
    }
}
