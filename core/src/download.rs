//! Загрузка файлов: общий HTTP-клиент, потоковая запись с хэшированием, повторы с паузой,
//! прогресс, отмена. Файл появляется под своим именем только после проверки хэша.

use crate::error::{Error, IoContext, Result};
use crate::net;
use crate::paths::unique_sibling;
use sha1::Sha1;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::AsyncWriteExt;
use tokio::sync::Semaphore;

/// Ожидаемый хэш файла.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expected {
    Sha1(String),
    Sha256(String),
    /// Источник хэш не публикует. Такие файлы отмечаются в отчёте как непроверенные.
    Unverified,
}

#[derive(Debug, Clone)]
pub struct Task {
    pub url: String,
    pub dest: PathBuf,
    pub expected: Expected,
    pub size: Option<u64>,
}

impl Task {
    pub fn sha1(
        url: impl Into<String>,
        dest: impl Into<PathBuf>,
        sha1: &str,
        size: Option<u64>,
    ) -> Self {
        Self {
            url: url.into(),
            dest: dest.into(),
            expected: Expected::Sha1(sha1.to_ascii_lowercase()),
            size,
        }
    }
}

/// Насколько тщательно проверять уже скачанные файлы.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verify {
    /// Перед запуском: сверяется размер (если известен), иначе хэш. Быстро.
    Quick,
    /// «Проверить файлы»: хэш каждого файла.
    Full,
}

/// Флаг отмены. Дочерний флаг отменяется вместе с родителем, но не наоборот.
#[derive(Debug, Clone, Default)]
pub struct Cancel {
    own: Arc<AtomicBool>,
    parents: Vec<Arc<AtomicBool>>,
}

impl Cancel {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn child(&self) -> Self {
        let mut parents = self.parents.clone();
        parents.push(self.own.clone());
        Self {
            own: Arc::new(AtomicBool::new(false)),
            parents,
        }
    }
    pub fn cancel(&self) {
        self.own.store(true, Ordering::SeqCst);
    }
    pub fn is_cancelled(&self) -> bool {
        self.own.load(Ordering::SeqCst) || self.parents.iter().any(|p| p.load(Ordering::SeqCst))
    }
    pub fn check(&self) -> Result<()> {
        if self.is_cancelled() {
            Err(Error::Cancelled)
        } else {
            Ok(())
        }
    }
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct Progress {
    pub files_total: u64,
    pub files_done: u64,
    /// Сумма известных размеров файлов, которые нужно скачать.
    pub bytes_total: u64,
    pub bytes_done: u64,
    pub retries: u64,
}

pub type ProgressFn = Arc<dyn Fn(&Progress) + Send + Sync>;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Summary {
    pub downloaded: u64,
    pub already_present: u64,
    pub bytes: u64,
    /// Файлы без опубликованного хэша (скачаны, но не проверены).
    pub unverified: Vec<PathBuf>,
}

struct Shared {
    files_total: u64,
    files_done: AtomicU64,
    bytes_total: AtomicU64,
    bytes_done: AtomicU64,
    retries: AtomicU64,
    last_report: Mutex<Instant>,
    on_progress: Option<ProgressFn>,
}

impl Shared {
    fn snapshot(&self) -> Progress {
        Progress {
            files_total: self.files_total,
            files_done: self.files_done.load(Ordering::Relaxed),
            bytes_total: self.bytes_total.load(Ordering::Relaxed),
            bytes_done: self.bytes_done.load(Ordering::Relaxed),
            retries: self.retries.load(Ordering::Relaxed),
        }
    }

    /// Сообщать не чаще 10 раз в секунду (и всегда — по завершении файла).
    fn report(&self, force: bool) {
        let Some(cb) = &self.on_progress else { return };
        if !force {
            let mut last = self.last_report.lock().unwrap_or_else(|p| p.into_inner());
            if last.elapsed() < Duration::from_millis(100) {
                return;
            }
            *last = Instant::now();
        }
        cb(&self.snapshot());
    }
}

/// Проверить готовый файл на диске.
pub fn is_present(
    path: &Path,
    expected: &Expected,
    size: Option<u64>,
    verify: Verify,
) -> Result<bool> {
    let meta = match std::fs::metadata(path) {
        Ok(m) if m.is_file() => m,
        Ok(_) => return Ok(false),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(Error::io(path, e)),
    };
    if let Some(size) = size {
        if meta.len() != size {
            return Ok(false);
        }
        if verify == Verify::Quick {
            return Ok(true);
        }
    }
    match expected {
        Expected::Unverified => Ok(verify == Verify::Quick || size.is_none()),
        Expected::Sha1(h) => Ok(hash_file::<Sha1>(path)?.eq_ignore_ascii_case(h)),
        Expected::Sha256(h) => Ok(hash_file::<Sha256>(path)?.eq_ignore_ascii_case(h)),
    }
}

pub fn hash_file<D: Digest>(path: &Path) -> Result<String> {
    let mut file = std::fs::File::open(path).at(path)?;
    let mut hasher = D::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf).at(path)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

enum Hasher {
    Sha1(Sha1),
    Sha256(Sha256),
    None,
}

impl Hasher {
    fn for_expected(e: &Expected) -> Self {
        match e {
            Expected::Sha1(_) => Hasher::Sha1(Sha1::new()),
            Expected::Sha256(_) => Hasher::Sha256(Sha256::new()),
            Expected::Unverified => Hasher::None,
        }
    }
    fn update(&mut self, data: &[u8]) {
        match self {
            Hasher::Sha1(h) => h.update(data),
            Hasher::Sha256(h) => h.update(data),
            Hasher::None => {}
        }
    }
    fn finish(self) -> Option<String> {
        match self {
            Hasher::Sha1(h) => Some(hex::encode(h.finalize())),
            Hasher::Sha256(h) => Some(hex::encode(h.finalize())),
            Hasher::None => None,
        }
    }
}

/// Одна попытка: потоком во временный файл рядом с целью, проверка, переименование.
async fn fetch_once(task: &Task, shared: &Shared, cancel: &Cancel) -> Result<u64> {
    let resp = net::client()
        .get(&task.url)
        .send()
        .await
        .map_err(|e| net::network_error(&task.url, e))?;
    let mut resp = net::check_status(&task.url, resp).await?;
    if let (Some(len), None) = (resp.content_length(), task.size) {
        shared.bytes_total.fetch_add(len, Ordering::Relaxed);
    }

    let tmp = unique_sibling(&task.dest, "part");
    let mut file = tokio::fs::File::create(&tmp).await.at(&tmp)?;
    let mut hasher = Hasher::for_expected(&task.expected);
    let mut written: u64 = 0;

    let result: Result<()> = async {
        loop {
            cancel.check()?;
            let chunk = resp
                .chunk()
                .await
                .map_err(|e| net::network_error(&task.url, e))?;
            let Some(chunk) = chunk else { break };
            hasher.update(&chunk);
            file.write_all(&chunk).await.at(&tmp)?;
            written += chunk.len() as u64;
            shared
                .bytes_done
                .fetch_add(chunk.len() as u64, Ordering::Relaxed);
            shared.report(false);
        }
        file.flush().await.at(&tmp)?;
        Ok(())
    }
    .await;
    drop(file);

    let finish = || -> Result<()> {
        result?;
        if let Some(size) = task.size
            && written != size
        {
            return Err(Error::HashMismatch {
                path: task.dest.clone(),
                expected: format!("{size} байт"),
                actual: format!("{written} байт"),
            });
        }
        if let (Some(actual), Expected::Sha1(exp) | Expected::Sha256(exp)) =
            (hasher.finish(), &task.expected)
            && !actual.eq_ignore_ascii_case(exp)
        {
            return Err(Error::HashMismatch {
                path: task.dest.clone(),
                expected: exp.clone(),
                actual,
            });
        }
        std::fs::rename(&tmp, &task.dest).at(&task.dest)
    };
    match finish() {
        Ok(()) => Ok(written),
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            // Байты неудачной попытки не считаем скачанными.
            shared.bytes_done.fetch_sub(
                written.min(shared.bytes_done.load(Ordering::Relaxed)),
                Ordering::Relaxed,
            );
            Err(e)
        }
    }
}

async fn fetch_with_retries(task: &Task, shared: &Shared, cancel: &Cancel) -> Result<u64> {
    if let Some(parent) = task.dest.parent() {
        tokio::fs::create_dir_all(parent).await.at(parent)?;
    }
    let mut attempt = 0;
    loop {
        attempt += 1;
        match fetch_once(task, shared, cancel).await {
            // Повреждённая передача тоже повод повторить: обычно это обрыв, а не подмена.
            Err(e)
                if (e.is_retryable() || matches!(e, Error::HashMismatch { .. }))
                    && attempt < net::ATTEMPTS =>
            {
                shared.retries.fetch_add(1, Ordering::Relaxed);
                tokio::time::sleep(net::backoff(attempt)).await;
                cancel.check()?;
            }
            other => return other,
        }
    }
}

/// Скачать недостающие файлы. Одинаковые пути скачиваются один раз. Первая ошибка
/// останавливает остальные загрузки.
pub async fn download_all(
    tasks: Vec<Task>,
    concurrency: usize,
    verify: Verify,
    on_progress: Option<ProgressFn>,
    cancel: &Cancel,
) -> Result<Summary> {
    let mut seen = HashSet::new();
    let tasks: Vec<Task> = tasks
        .into_iter()
        .filter(|t| seen.insert(t.dest.clone()))
        .collect();

    // Проверка уже скачанного — на пуле блокирующих потоков (чтение и хэширование диска).
    let checks: Vec<(Task, bool)> = tokio::task::spawn_blocking(move || {
        tasks
            .into_iter()
            .map(|t| {
                let present = is_present(&t.dest, &t.expected, t.size, verify).unwrap_or(false);
                (t, present)
            })
            .collect()
    })
    .await
    .map_err(|e| Error::invalid(format!("Проверка файлов прервана: {e}")))?;

    let mut summary = Summary::default();
    let mut todo = Vec::new();
    for (task, present) in checks {
        if present {
            summary.already_present += 1;
        } else {
            todo.push(task);
        }
    }

    let shared = Arc::new(Shared {
        files_total: todo.len() as u64,
        files_done: AtomicU64::new(0),
        bytes_total: AtomicU64::new(todo.iter().filter_map(|t| t.size).sum()),
        bytes_done: AtomicU64::new(0),
        retries: AtomicU64::new(0),
        last_report: Mutex::new(Instant::now()),
        on_progress,
    });
    shared.report(true);

    // Отменяется снаружи или при первой ошибке любой из загрузок.
    let local_cancel = cancel.child();
    let sem = Arc::new(Semaphore::new(concurrency.max(1)));
    let mut set = tokio::task::JoinSet::new();
    for task in todo {
        let sem = sem.clone();
        let shared = shared.clone();
        let local = local_cancel.clone();
        set.spawn(async move {
            let _permit = sem.acquire_owned().await.map_err(|_| Error::Cancelled)?;
            local.check()?;
            let res = fetch_with_retries(&task, &shared, &local).await;
            if res.is_ok() {
                shared.files_done.fetch_add(1, Ordering::Relaxed);
                shared.report(true);
            }
            res.map(|bytes| (bytes, task))
        });
    }

    let mut first_error: Option<Error> = None;
    while let Some(joined) = set.join_next().await {
        match joined {
            Ok(Ok((bytes, task))) => {
                summary.downloaded += 1;
                summary.bytes += bytes;
                if task.expected == Expected::Unverified {
                    summary.unverified.push(task.dest);
                }
            }
            Ok(Err(e)) => {
                local_cancel.cancel();
                // Отмена из-за чужой ошибки не должна заслонить саму ошибку.
                if first_error.is_none() || matches!(first_error, Some(Error::Cancelled)) {
                    first_error = Some(e);
                }
            }
            Err(join_err) => {
                local_cancel.cancel();
                first_error.get_or_insert(Error::invalid(format!("Загрузка прервана: {join_err}")));
            }
        }
    }
    match first_error {
        Some(e) => Err(e),
        None => Ok(summary),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("quarry-dl-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn present_file_checks() {
        let d = tmpdir("present");
        let p = d.join("hello.txt");
        std::fs::write(&p, b"hello").unwrap();
        let sha1 = Expected::Sha1("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d".into());
        let bad = Expected::Sha1("0000000000000000000000000000000000000000".into());
        assert!(is_present(&p, &sha1, Some(5), Verify::Full).unwrap());
        assert!(!is_present(&p, &bad, Some(5), Verify::Full).unwrap());
        // Быстрая проверка доверяет размеру…
        assert!(is_present(&p, &bad, Some(5), Verify::Quick).unwrap());
        // …но без размера считает хэш.
        assert!(!is_present(&p, &bad, None, Verify::Quick).unwrap());
        assert!(!is_present(&p, &sha1, Some(6), Verify::Quick).unwrap());
        assert!(!is_present(&d.join("missing"), &sha1, None, Verify::Full).unwrap());
        let sha256 = Expected::Sha256(
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824".into(),
        );
        assert!(is_present(&p, &sha256, None, Verify::Full).unwrap());
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[tokio::test]
    async fn nothing_to_download_when_all_present() {
        let d = tmpdir("noop");
        let p = d.join("hello.txt");
        std::fs::write(&p, b"hello").unwrap();
        // Два задания на один путь — считаются одним.
        let t = Task::sha1(
            "https://invalid.example/x",
            &p,
            "AAF4C61DDCC5E8A2DABEDE0F3B482CD9AEA9434D",
            Some(5),
        );
        let s = download_all(vec![t.clone(), t], 4, Verify::Full, None, &Cancel::new())
            .await
            .unwrap();
        assert_eq!(s.already_present, 1);
        assert_eq!(s.downloaded, 0);
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn child_cancel_follows_parent_only() {
        let parent = Cancel::new();
        let child = parent.child();
        child.cancel();
        assert!(!parent.is_cancelled());
        let child2 = parent.child();
        parent.cancel();
        assert!(child2.is_cancelled());
    }

    #[tokio::test]
    async fn cancelled_before_start() {
        let d = tmpdir("cancel");
        let cancel = Cancel::new();
        cancel.cancel();
        let t = Task::sha1("https://invalid.example/x", d.join("x"), "00", None);
        let err = download_all(vec![t], 1, Verify::Quick, None, &cancel)
            .await
            .unwrap_err();
        assert!(matches!(err, Error::Cancelled));
        assert!(!d.join("x").exists());
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[tokio::test]
    #[ignore = "сеть: скачивает logging-конфиг Mojang (888 байт)"]
    async fn downloads_and_verifies_real_file() {
        let d = tmpdir("net");
        let dest = d.join("client-1.12.xml");
        let t = Task::sha1(
            "https://piston-data.mojang.com/v1/objects/bd65e7d2e3c237be76cfbef4c2405033d7f91521/client-1.12.xml",
            &dest,
            "bd65e7d2e3c237be76cfbef4c2405033d7f91521",
            Some(888),
        );
        let s = download_all(vec![t], 1, Verify::Full, None, &Cancel::new())
            .await
            .unwrap();
        assert_eq!(s.downloaded, 1);
        assert_eq!(std::fs::metadata(&dest).unwrap().len(), 888);
        // Неверный хэш — файла под целевым именем нет, временных тоже.
        let bad = d.join("bad.xml");
        let t = Task::sha1(
            "https://piston-data.mojang.com/v1/objects/bd65e7d2e3c237be76cfbef4c2405033d7f91521/client-1.12.xml",
            &bad,
            "0000000000000000000000000000000000000000",
            None,
        );
        assert!(
            download_all(vec![t], 1, Verify::Full, None, &Cancel::new())
                .await
                .is_err()
        );
        assert!(!bad.exists());
        assert_eq!(std::fs::read_dir(&d).unwrap().count(), 1);
        std::fs::remove_dir_all(&d).unwrap();
    }
}
