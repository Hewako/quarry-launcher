//! Установка версии в общий кеш: клиент, библиотеки, нативы, ассеты, logging-конфиг.
//! Ничего не пишет в папку профиля, кроме ресурсов совсем старых версий (`pre-1.6`).

use crate::download::{self, Cancel, Expected, Progress, ProgressFn, Task, Verify};
use crate::error::{Error, IoContext, Result};
use crate::metadata::version::{Artifact, VersionJson};
use crate::metadata::{self, Environment};
use crate::paths::{DataDir, safe_join};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Metadata,
    Libraries,
    Assets,
    Natives,
    Java,
    Loader,
}

impl Stage {
    pub fn label(self) -> &'static str {
        match self {
            Stage::Metadata => "Метаданные версии",
            Stage::Libraries => "Клиент и библиотеки",
            Stage::Assets => "Ресурсы игры",
            Stage::Natives => "Нативные библиотеки",
            Stage::Java => "Java",
            Stage::Loader => "Установка загрузчика",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct InstallEvent {
    pub stage: Stage,
    pub progress: Option<Progress>,
}

pub type EventFn = Arc<dyn Fn(InstallEvent) + Send + Sync>;

/// План установки (результат [`Installer::plan`]).
#[derive(Debug, Clone)]
pub struct Plan {
    pub tasks: Vec<Task>,
    pub client_jar: PathBuf,
    pub libraries: Vec<PathBuf>,
    pub native_jars: Vec<(PathBuf, Vec<String>)>,
    pub logging_argument: Option<String>,
    pub index_ref: crate::metadata::version::AssetIndexRef,
    pub index_path: PathBuf,
}

/// Результат установки: всё, что нужно для сборки команды запуска.
#[derive(Debug, Clone)]
pub struct Installed {
    pub version: VersionJson,
    pub client_jar: PathBuf,
    /// Classpath без клиента, в порядке JSON; клиент добавляется последним.
    pub libraries: Vec<PathBuf>,
    pub natives_dir: PathBuf,
    pub assets_root: PathBuf,
    /// `${game_assets}` для старых версий (виртуальная раскладка или `resources/`).
    pub game_assets: PathBuf,
    pub assets_index_name: String,
    /// Готовый аргумент logging-конфига (`-Dlog4j.configurationFile=…`).
    pub logging_argument: Option<String>,
    /// Файлы без опубликованного хэша (скачаны, но не проверены).
    pub unverified: Vec<PathBuf>,
}

impl Installed {
    pub fn classpath(&self) -> Vec<PathBuf> {
        let mut cp = self.libraries.clone();
        cp.push(self.client_jar.clone());
        cp
    }
}

#[derive(Debug, Deserialize)]
struct AssetIndex {
    objects: BTreeMap<String, AssetObject>,
    #[serde(default, rename = "virtual")]
    is_virtual: bool,
    #[serde(default)]
    map_to_resources: bool,
}

#[derive(Debug, Deserialize)]
struct AssetObject {
    hash: String,
    size: u64,
}

fn artifact_task(base: &Path, art: &Artifact) -> Result<(Task, PathBuf)> {
    let rel = art
        .path
        .as_deref()
        .ok_or_else(|| Error::invalid(format!("У библиотеки нет пути: {}", art.url)))?;
    let dest = safe_join(base, rel)
        .ok_or_else(|| Error::invalid(format!("Недопустимый путь библиотеки: {rel}")))?;
    let expected = match &art.sha1 {
        Some(h) => Expected::Sha1(h.to_ascii_lowercase()),
        None => Expected::Unverified,
    };
    Ok((
        Task {
            url: art.url.clone(),
            dest: dest.clone(),
            expected,
            size: art.size.filter(|s| *s > 0),
        },
        dest,
    ))
}

/// Для Maven-библиотек без хэша в профиле (Quilt) — взять `.sha1` рядом с файлом в том же
/// репозитории. Нет файла `.sha1` — библиотека останется непроверенной (это видно в отчёте).
async fn fill_maven_hashes(tasks: &mut [Task]) {
    let sem = Arc::new(tokio::sync::Semaphore::new(8));
    let mut set = tokio::task::JoinSet::new();
    for (i, t) in tasks.iter().enumerate() {
        if t.expected != Expected::Unverified || !t.url.starts_with("https://") {
            continue;
        }
        let url = format!("{}.sha1", t.url);
        let sem = sem.clone();
        set.spawn(async move {
            let _p = sem.acquire_owned().await.ok()?;
            let text = crate::net::get_text(&url).await.ok()?;
            let hash = text.split_whitespace().next()?.to_ascii_lowercase();
            (hash.len() == 40 && hash.chars().all(|c| c.is_ascii_hexdigit())).then_some((i, hash))
        });
    }
    while let Some(Ok(res)) = set.join_next().await {
        if let Some((i, hash)) = res {
            tasks[i].expected = Expected::Sha1(hash);
        }
    }
}

pub struct Installer<'a> {
    pub data: &'a DataDir,
    pub env: Environment,
    pub verify: Verify,
    pub cancel: Cancel,
    pub on_event: Option<EventFn>,
}

impl Installer<'_> {
    fn emit(&self, stage: Stage, progress: Option<Progress>) {
        if let Some(cb) = &self.on_event {
            cb(InstallEvent { stage, progress });
        }
    }

    fn progress_for(&self, stage: Stage) -> Option<ProgressFn> {
        let cb = self.on_event.clone()?;
        Some(Arc::new(move |p: &Progress| {
            cb(InstallEvent {
                stage,
                progress: Some(p.clone()),
            })
        }))
    }

    /// Что нужно скачать и как будет устроен classpath — без сети и без записи на диск.
    pub fn plan(&self, version: &VersionJson, game_version: &str) -> Result<Plan> {
        metadata::file_safe(game_version)?;
        let libs_dir = self.data.libraries();
        let mut tasks = Vec::new();

        // Клиент.
        let client = version
            .downloads
            .as_ref()
            .and_then(|d| d.client.clone())
            .ok_or_else(|| {
                Error::invalid(format!("В версии {} нет ссылки на клиент", version.id))
            })?;
        let client_jar = self
            .data
            .versions()
            .join(game_version)
            .join(format!("{game_version}.jar"));
        tasks.push(Task::sha1(
            &client.url,
            &client_jar,
            &client.sha1,
            Some(client.size),
        ));

        // Библиотеки и нативы.
        let mut libraries = Vec::new();
        let mut seen_cp = HashSet::new();
        let mut native_jars: Vec<(PathBuf, Vec<String>)> = Vec::new();
        for lib in version.libraries.iter().filter(|l| l.is_allowed(&self.env)) {
            if let Some(art) = lib.main_artifact() {
                let (task, dest) = artifact_task(&libs_dir, &art)?;
                tasks.push(task);
                if seen_cp.insert(dest.clone()) {
                    libraries.push(dest);
                }
            }
            if let Some(art) = lib.native_artifact(&self.env) {
                let (task, dest) = artifact_task(&libs_dir, &art)?;
                tasks.push(task);
                native_jars.push((dest, lib.extract_excludes()));
            }
        }

        // Logging-конфиг (защита от Log4Shell для 1.7–1.18 и единый формат логов).
        let mut logging_argument = None;
        if let Some(entry) = version.logging.as_ref().and_then(|l| l.client.as_ref()) {
            let name = entry.file.id.clone().unwrap_or_else(|| "client.xml".into());
            let file_name = format!(
                "{}-{}",
                &entry.file.sha1[..entry.file.sha1.len().min(8)],
                name
            );
            metadata::file_safe(&file_name)?;
            let dest = self.data.versions().join("logging").join(file_name);
            tasks.push(Task::sha1(
                &entry.file.url,
                &dest,
                &entry.file.sha1,
                Some(entry.file.size),
            ));
            logging_argument = Some(entry.argument.replace("${path}", &dest.to_string_lossy()));
        }

        // Индекс ассетов.
        let index_ref = version
            .asset_index
            .clone()
            .ok_or_else(|| Error::invalid(format!("В версии {} нет assetIndex", version.id)))?;
        metadata::file_safe(&index_ref.id)?;
        let assets_root = self.data.assets();
        let index_path = assets_root
            .join("indexes")
            .join(format!("{}.json", index_ref.id));
        tasks.push(Task::sha1(
            &index_ref.url,
            &index_path,
            &index_ref.sha1,
            Some(index_ref.size).filter(|s| *s > 0),
        ));

        Ok(Plan {
            tasks,
            client_jar,
            libraries,
            native_jars,
            logging_argument,
            index_ref,
            index_path,
        })
    }

    /// Установить версию `version` (уже с наложенным загрузчиком). `game_version` — ванильный
    /// ID, под ним хранится клиент. `game_dir` нужен только для ресурсов `pre-1.6`.
    pub async fn install(
        &self,
        version: VersionJson,
        game_version: &str,
        game_dir: &Path,
    ) -> Result<Installed> {
        self.cancel.check()?;
        let Plan {
            mut tasks,
            client_jar,
            libraries,
            native_jars,
            logging_argument,
            index_ref,
            index_path,
        } = self.plan(&version, game_version)?;
        let assets_root = self.data.assets();
        let mut unverified = Vec::new();

        fill_maven_hashes(&mut tasks).await;
        self.emit(Stage::Libraries, None);
        let summary = download::download_all(
            tasks,
            8,
            self.verify,
            self.progress_for(Stage::Libraries),
            &self.cancel,
        )
        .await?;
        unverified.extend(summary.unverified);

        // Ассеты.
        self.cancel.check()?;
        let index_text = std::fs::read_to_string(&index_path).at(&index_path)?;
        let index: AssetIndex =
            serde_json::from_str(&index_text).map_err(|e| Error::parse("индекс ассетов", e))?;
        let mut asset_tasks = Vec::with_capacity(index.objects.len());
        for obj in index.objects.values() {
            if obj.hash.len() < 2 || !obj.hash.chars().all(|c| c.is_ascii_hexdigit()) {
                return Err(Error::invalid(format!(
                    "Недопустимый хэш ассета: {}",
                    obj.hash
                )));
            }
            let sub = &obj.hash[..2];
            asset_tasks.push(Task::sha1(
                format!(
                    "https://resources.download.minecraft.net/{sub}/{}",
                    obj.hash
                ),
                assets_root.join("objects").join(sub).join(&obj.hash),
                &obj.hash,
                Some(obj.size),
            ));
        }
        self.emit(Stage::Assets, None);
        download::download_all(
            asset_tasks,
            16,
            self.verify,
            self.progress_for(Stage::Assets),
            &self.cancel,
        )
        .await?;

        // Старые раскладки ассетов: копии по именам.
        let game_assets = if index.map_to_resources {
            let dir = game_dir.join("resources");
            copy_named_assets(&assets_root, &index, &dir)?;
            dir
        } else if index.is_virtual {
            let dir = assets_root.join("virtual").join(&index_ref.id);
            copy_named_assets(&assets_root, &index, &dir)?;
            dir
        } else {
            assets_root.clone()
        };

        // Нативы старого формата — распаковка в папку, общую для одинакового набора.
        self.emit(Stage::Natives, None);
        let natives_dir = self.extract_natives(&version.id, &native_jars)?;

        Ok(Installed {
            version,
            client_jar,
            libraries,
            natives_dir,
            assets_root,
            game_assets,
            assets_index_name: index_ref.id,
            logging_argument,
            unverified,
        })
    }

    /// Папка нативов определяется составом архивов, поэтому параллельные профили одной версии
    /// не перезаписывают файлы друг друга. Распаковка — во временную папку и переименование.
    /// Папка нативов для набора архивов (без распаковки).
    pub fn natives_dir_for(
        &self,
        version_id: &str,
        jars: &[(PathBuf, Vec<String>)],
    ) -> Result<PathBuf> {
        let mut key = metadata::sha1_hex(version_id.as_bytes());
        for (jar, _) in jars {
            key = metadata::sha1_hex(format!("{key}{}", jar.display()).as_bytes());
        }
        Ok(self.data.natives().join(format!(
            "{}-{}",
            metadata::file_safe(version_id)?,
            &key[..10]
        )))
    }

    fn extract_natives(
        &self,
        version_id: &str,
        jars: &[(PathBuf, Vec<String>)],
    ) -> Result<PathBuf> {
        let dir = self.natives_dir_for(version_id, jars)?;
        if dir.join(".complete").exists() {
            return Ok(dir);
        }
        let staging = crate::paths::unique_sibling(&dir, "staging");
        std::fs::create_dir_all(&staging).at(&staging)?;
        for (jar, excludes) in jars {
            let excludes = excludes.clone();
            crate::archive::extract_zip_filtered(
                jar,
                &staging,
                |name| {
                    // Только сами нативные библиотеки: рядом в архивах лежат .sha1, .git и т. п.
                    let native = [".dylib", ".jnilib", ".so", ".dll"]
                        .iter()
                        .any(|ext| name.ends_with(ext));
                    native && !excludes.iter().any(|e| name.starts_with(e.as_str()))
                },
                true,
            )?;
        }
        std::fs::write(staging.join(".complete"), b"").at(&staging)?;
        match std::fs::rename(&staging, &dir) {
            Ok(()) => Ok(dir),
            // Параллельный запуск успел раньше — его копия такая же.
            Err(_) if dir.join(".complete").exists() => {
                let _ = std::fs::remove_dir_all(&staging);
                Ok(dir)
            }
            Err(e) => Err(Error::io(&dir, e)),
        }
    }
}

fn copy_named_assets(assets_root: &Path, index: &AssetIndex, dest: &Path) -> Result<()> {
    for (name, obj) in &index.objects {
        let target = safe_join(dest, name)
            .ok_or_else(|| Error::invalid(format!("Недопустимое имя ассета: {name}")))?;
        if target
            .metadata()
            .map(|m| m.len() == obj.size)
            .unwrap_or(false)
        {
            continue;
        }
        let src = assets_root
            .join("objects")
            .join(&obj.hash[..2])
            .join(&obj.hash);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).at(parent)?;
        }
        std::fs::copy(&src, &target).at(&target)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadata::{Arch, Os};

    #[test]
    fn asset_index_flags() {
        let idx: AssetIndex = serde_json::from_str(
            r#"{"virtual":true,"objects":{"sound/a.ogg":{"hash":"ab12","size":3}}}"#,
        )
        .unwrap();
        assert!(idx.is_virtual && !idx.map_to_resources);
        let idx: AssetIndex =
            serde_json::from_str(r#"{"map_to_resources":true,"objects":{}}"#).unwrap();
        assert!(idx.map_to_resources);
    }

    #[test]
    fn library_paths_cannot_escape_cache() {
        let art = Artifact {
            path: Some("../../evil.jar".into()),
            sha1: None,
            size: None,
            url: "https://x".into(),
        };
        assert!(artifact_task(Path::new("/cache/libraries"), &art).is_err());
    }

    #[test]
    fn natives_dir_depends_on_archive_set() {
        let root = std::env::temp_dir().join(format!("quarry-natives-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let data = DataDir::new(&root);
        let inst = Installer {
            data: &data,
            env: Environment::new(Os::Osx, Arch::Arm64, "15.1"),
            verify: Verify::Quick,
            cancel: Cancel::new(),
            on_event: None,
        };
        let a = inst.extract_natives("26.3", &[]).unwrap();
        let again = inst.extract_natives("26.3", &[]).unwrap();
        assert_eq!(a, again);
        assert!(a.join(".complete").exists());
        let b = inst.extract_natives("1.16.5", &[]).unwrap();
        assert_ne!(a, b);
        std::fs::remove_dir_all(&root).unwrap();
    }
}
