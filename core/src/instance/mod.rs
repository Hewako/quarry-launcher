//! Профили (инстансы): изолированные папки игры, `instance.toml`, корзина вместо удаления.

use crate::error::{Error, IoContext, Result};
use crate::loader::LoaderKind;
use crate::paths::{DataDir, write_atomic};
use crate::settings::LaunchOverrides;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

pub const FORMAT: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LaunchOutcome {
    Success,
    Crashed,
    Killed,
    FailedToStart,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LastLaunch {
    /// Unix-время начала запуска.
    pub at: u64,
    pub outcome: LaunchOutcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(default)]
    pub duration_secs: u64,
    /// Короткое объяснение (для краша — что известно о причине).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log_file: Option<PathBuf>,
}

/// Как профиль входит в игру.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthMode {
    /// Microsoft-аккаунт (официальные серверы).
    #[default]
    Microsoft,
    /// Ely.by через authlib-injector — только совместимые серверы.
    ElyBy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Instance {
    #[serde(default = "format_version")]
    pub format: u32,
    pub id: String,
    pub name: String,
    pub game_version: String,
    #[serde(default = "vanilla")]
    pub loader: LoaderKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loader_version: Option<String>,
    #[serde(default)]
    pub created_at: u64,
    /// Папка для порядка в списке (не уровень настроек).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    #[serde(default)]
    pub auth_mode: AuthMode,
    /// Аккаунт профиля (`провайдер:uuid`); пусто — аккаунт по умолчанию.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
    /// Настройки профиля (переопределяют сборку и глобальные).
    #[serde(default)]
    pub settings: LaunchOverrides,
    /// Рекомендации автора сборки (заполняются при импорте .mrpack).
    #[serde(default)]
    pub pack_settings: LaunchOverrides,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_launch: Option<LastLaunch>,
    /// Папка игры вне Quarry (подключённая `.minecraft` без копирования). Корзина её не трогает.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_game_dir: Option<PathBuf>,
}

fn format_version() -> u32 {
    FORMAT
}
fn vanilla() -> LoaderKind {
    LoaderKind::Vanilla
}

/// Формат до версии 0.1 (`profile.toml`).
#[derive(Debug, Deserialize)]
struct LegacyProfile {
    id: String,
    name: String,
    mc_version: String,
    #[serde(default)]
    loader: Option<String>,
}

/// Профиль, который не удалось прочитать — показывается в списке с причиной.
#[derive(Debug, Clone, Serialize)]
pub struct BrokenInstance {
    pub dir: PathBuf,
    pub reason: String,
}

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub struct InstanceStore {
    data: DataDir,
}

impl InstanceStore {
    pub fn new(data: DataDir) -> Self {
        Self { data }
    }

    pub fn data(&self) -> &DataDir {
        &self.data
    }

    pub fn dir(&self, id: &str) -> Result<PathBuf> {
        validate_id(id)?;
        Ok(self.data.instances().join(id))
    }
    pub fn game_dir(&self, id: &str) -> Result<PathBuf> {
        let dir = self.dir(id)?;
        // Подключённая внешняя папка (без копирования) хранится в instance.toml.
        if let Ok(text) = std::fs::read_to_string(dir.join("instance.toml"))
            && let Ok(inst) = toml::from_str::<Instance>(&text)
            && let Some(ext) = inst.external_game_dir
        {
            return Ok(ext);
        }
        Ok(dir.join("game"))
    }
    pub fn logs_dir(&self, id: &str) -> Result<PathBuf> {
        Ok(self.dir(id)?.join("logs"))
    }

    /// Все профили. Битые не ломают список, а возвращаются отдельно с причиной.
    pub fn list(&self) -> Result<(Vec<Instance>, Vec<BrokenInstance>)> {
        let root = self.data.instances();
        let mut ok = Vec::new();
        let mut broken = Vec::new();
        let entries = match std::fs::read_dir(&root) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok((ok, broken)),
            Err(e) => return Err(Error::io(&root, e)),
        };
        for entry in entries.flatten() {
            let dir = entry.path();
            if !dir.is_dir() {
                continue;
            }
            let Some(id) = dir.file_name().map(|n| n.to_string_lossy().into_owned()) else {
                continue;
            };
            if validate_id(&id).is_err() {
                continue;
            }
            match self.get(&id) {
                Ok(i) => ok.push(i),
                Err(Error::NotFound(_)) => {}
                Err(e) => broken.push(BrokenInstance {
                    dir,
                    reason: e.to_string(),
                }),
            }
        }
        ok.sort_by(|a, b| {
            let la = a.last_launch.as_ref().map(|l| l.at).unwrap_or(0);
            let lb = b.last_launch.as_ref().map(|l| l.at).unwrap_or(0);
            lb.cmp(&la)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        Ok((ok, broken))
    }

    pub fn get(&self, id: &str) -> Result<Instance> {
        let dir = self.dir(id)?;
        let path = dir.join("instance.toml");
        match std::fs::read_to_string(&path) {
            Ok(text) => {
                let mut inst: Instance = toml::from_str(&text)
                    .map_err(|e| Error::parse(format!("{}", path.display()), e))?;
                // ID — это имя папки; содержимое файла его не переопределяет.
                inst.id = id.to_string();
                Ok(inst)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => self.read_legacy(id),
            Err(e) => Err(Error::io(&path, e)),
        }
    }

    /// Профиль старого формата: читаем и сохраняем в новом, старый файл не трогаем.
    fn read_legacy(&self, id: &str) -> Result<Instance> {
        let dir = self.dir(id)?;
        let path = dir.join("profile.toml");
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(Error::NotFound(format!("Профиль «{id}» не найден")));
            }
            Err(e) => return Err(Error::io(&path, e)),
        };
        let old: LegacyProfile =
            toml::from_str(&text).map_err(|e| Error::parse(format!("{}", path.display()), e))?;
        let _ = old.id;
        let inst = Instance {
            format: FORMAT,
            id: id.to_string(),
            name: old.name,
            game_version: old.mc_version,
            loader: match old.loader.as_deref() {
                Some(l) => LoaderKind::parse(l)?,
                None => LoaderKind::Vanilla,
            },
            loader_version: None,
            created_at: 0,
            group: None,
            auth_mode: AuthMode::Microsoft,
            account: None,
            settings: LaunchOverrides::default(),
            pack_settings: LaunchOverrides::default(),
            last_launch: None,
            external_game_dir: None,
        };
        self.save(&inst)?;
        Ok(inst)
    }

    pub fn save(&self, inst: &Instance) -> Result<()> {
        crate::settings::validate(&inst.settings)?;
        let dir = self.dir(&inst.id)?;
        let text = toml::to_string_pretty(inst).map_err(|e| Error::parse("instance.toml", e))?;
        write_atomic(&dir.join("instance.toml"), text.as_bytes())
    }

    pub fn create(
        &self,
        name: &str,
        game_version: &str,
        loader: LoaderKind,
        loader_version: Option<String>,
    ) -> Result<Instance> {
        let name = name.trim();
        if name.is_empty() {
            return Err(Error::invalid("Имя профиля не может быть пустым"));
        }
        if name.chars().count() > 64 {
            return Err(Error::invalid("Имя профиля длиннее 64 символов"));
        }
        crate::metadata::file_safe(game_version)?;
        if !loader.is_supported() {
            return Err(Error::invalid(format!(
                "{} пока не поддерживается",
                loader.display()
            )));
        }
        let base = slugify(name);
        if base.is_empty() {
            return Err(Error::invalid(
                "Из имени не получилось имя папки: используйте буквы или цифры",
            ));
        }
        let mut id = base.clone();
        let mut n = 2;
        while self.data.instances().join(&id).exists() {
            id = format!("{base}-{n}");
            n += 1;
        }
        validate_id(&id)?;
        let dir = self.data.instances().join(&id);
        std::fs::create_dir_all(dir.join("game")).at(&dir)?;
        let inst = Instance {
            format: FORMAT,
            id,
            name: name.to_string(),
            game_version: game_version.to_string(),
            loader,
            loader_version,
            created_at: now(),
            group: None,
            auth_mode: AuthMode::Microsoft,
            account: None,
            settings: LaunchOverrides::default(),
            pack_settings: LaunchOverrides::default(),
            last_launch: None,
            external_game_dir: None,
        };
        self.save(&inst)?;
        Ok(inst)
    }

    /// Убрать профиль в корзину (`trash/<id>-<время>`) вместе с мирами. Ничего не удаляется:
    /// восстановить можно, вернув папку в `instances/`.
    pub fn trash(&self, id: &str) -> Result<PathBuf> {
        let dir = self.dir(id)?;
        if !dir.is_dir() {
            return Err(Error::NotFound(format!("Профиль «{id}» не найден")));
        }
        let trash = self.data.trash();
        std::fs::create_dir_all(&trash).at(&trash)?;
        let target = trash.join(format!("{id}-{}", now()));
        std::fs::rename(&dir, &target).at(&dir)?;
        Ok(target)
    }
}

const WINDOWS_RESERVED: [&str; 22] = [
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
    "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

/// ID профиля = имя папки. Допустимы буквы (в том числе кириллица), цифры, `-` и `_`;
/// не длиннее 48 символов (пути на Windows); не зарезервированные имена Windows.
pub fn validate_id(id: &str) -> Result<()> {
    let ok = !id.is_empty()
        && id.chars().count() <= 48
        && id
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
        && !WINDOWS_RESERVED.contains(&id.to_lowercase().as_str());
    if ok {
        Ok(())
    } else {
        Err(Error::invalid(format!("Недопустимый ID профиля: {id:?}")))
    }
}

pub fn slugify(name: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    for c in name.chars().flat_map(|c| c.to_lowercase()) {
        if c.is_alphanumeric() {
            out.push(c);
            prev_dash = false;
        } else if !prev_dash && !out.is_empty() {
            out.push('-');
            prev_dash = true;
        }
        if out.chars().count() >= 40 {
            break;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if WINDOWS_RESERVED.contains(&out.as_str()) {
        out.push_str("-profile");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store(name: &str) -> (InstanceStore, PathBuf) {
        let root = std::env::temp_dir().join(format!("quarry-inst-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        (InstanceStore::new(DataDir::open(&root).unwrap()), root)
    }

    #[test]
    fn slugs() {
        assert_eq!(slugify("My Survival"), "my-survival");
        assert_eq!(slugify("  Test 123  "), "test-123");
        assert_eq!(slugify("Русский мир"), "русский-мир");
        assert_eq!(slugify("!!!"), "");
        assert_eq!(slugify("CON"), "con-profile");
        assert!(slugify(&"a".repeat(100)).chars().count() <= 40);
    }

    #[test]
    fn ids_reject_traversal_and_reserved() {
        for bad in [
            "", "..", "../etc", "foo/bar", "foo\\bar", "nul", "COM1", "a.b",
        ] {
            assert!(validate_id(bad).is_err(), "{bad}");
        }
        for good in ["my-profile", "test_123", "тест"] {
            assert!(validate_id(good).is_ok(), "{good}");
        }
    }

    #[test]
    fn create_list_and_duplicates() {
        let (s, root) = store("create");
        let a = s
            .create(
                "Технология+",
                "26.3",
                LoaderKind::Fabric,
                Some("0.20.1".into()),
            )
            .unwrap();
        let b = s
            .create("Технология+", "26.3", LoaderKind::Vanilla, None)
            .unwrap();
        assert_eq!(a.id, "технология");
        assert_eq!(b.id, "технология-2");
        assert!(s.game_dir(&a.id).unwrap().is_dir());
        let (list, broken) = s.list().unwrap();
        assert_eq!(list.len(), 2);
        assert!(broken.is_empty());
        assert!(s.create("x", "26.3", LoaderKind::Forge, None).is_err());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn broken_profile_does_not_break_list() {
        let (s, root) = store("broken");
        s.create("Good", "26.3", LoaderKind::Vanilla, None).unwrap();
        let bad = s.data().instances().join("bad");
        std::fs::create_dir_all(&bad).unwrap();
        std::fs::write(bad.join("instance.toml"), "id = [broken").unwrap();
        let (list, broken) = s.list().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(broken.len(), 1);
        assert!(broken[0].reason.contains("instance.toml"));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn legacy_profile_is_converted_without_deleting_it() {
        let (s, root) = store("legacy");
        let dir = s.data().instances().join("fabric-test");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("profile.toml"),
            "id = \"fabric-test\"\nname = \"Fabric test\"\nmc_version = \"26.3\"\nloader = \"fabric\"\n",
        )
        .unwrap();
        let i = s.get("fabric-test").unwrap();
        assert_eq!(i.loader, LoaderKind::Fabric);
        assert_eq!(i.game_version, "26.3");
        assert!(dir.join("instance.toml").exists());
        assert!(dir.join("profile.toml").exists());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn trash_keeps_worlds() {
        let (s, root) = store("trash");
        let i = s
            .create("Worlds", "1.20.1", LoaderKind::Vanilla, None)
            .unwrap();
        let save = s.game_dir(&i.id).unwrap().join("saves/World1/level.dat");
        std::fs::create_dir_all(save.parent().unwrap()).unwrap();
        std::fs::write(&save, b"x").unwrap();
        let moved = s.trash(&i.id).unwrap();
        assert!(moved.join("game/saves/World1/level.dat").exists());
        assert!(matches!(s.get(&i.id), Err(Error::NotFound(_))));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn invalid_settings_are_not_saved() {
        let (s, root) = store("settings");
        let mut i = s.create("S", "26.3", LoaderKind::Vanilla, None).unwrap();
        i.settings.max_memory_mb = Some(10);
        assert!(s.save(&i).is_err());
        std::fs::remove_dir_all(&root).unwrap();
    }
}
