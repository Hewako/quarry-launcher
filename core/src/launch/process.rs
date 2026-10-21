//! Process Supervisor: запуск игры, захват stdout/stderr, маскированный лог в файл,
//! код выхода, время работы, поиск crash-report, остановка по запросу.

use super::args::LaunchCommand;
use super::log4j::Log4jAssembler;
use crate::error::{Error, IoContext, Result};
use crate::logmask::Masker;
use serde::Serialize;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::Notify;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Stream {
    Stdout,
    Stderr,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum GameEvent {
    Started { pid: Option<u32> },
    Log { stream: Stream, line: String },
}

pub type GameEventFn = Arc<dyn Fn(GameEvent) + Send + Sync>;

#[derive(Debug, Clone, Serialize)]
pub struct ExitInfo {
    pub code: Option<i32>,
    pub duration_secs: u64,
    /// Остановлено пользователем из лаунчера.
    pub killed: bool,
    pub crashed: bool,
    pub crash_report: Option<PathBuf>,
    pub log_file: PathBuf,
    /// Последние строки вывода (уже маскированные) — для разбора краша.
    pub tail: Vec<String>,
    /// Разбор причины краша (заполняет сценарий запуска).
    #[serde(default)]
    pub analysis: Option<crate::mods::crash::Analysis>,
    /// Пик памяти процесса игры, МБ (замер раз в 5 секунд; на Windows пока нет).
    #[serde(default)]
    pub peak_ram_mb: Option<u64>,
    /// Сколько секунд заняла подготовка до старта игры (заполняет сценарий запуска).
    #[serde(default)]
    pub prepare_secs: Option<u64>,
}

/// Запущенная игра: ожидание завершения и остановка.
pub struct GameProcess {
    pub pid: Option<u32>,
    stop: Arc<Notify>,
    done: tokio::task::JoinHandle<Result<ExitInfo>>,
}

impl GameProcess {
    pub fn stopper(&self) -> Arc<Notify> {
        self.stop.clone()
    }
    pub async fn wait(self) -> Result<ExitInfo> {
        self.done
            .await
            .map_err(|e| Error::invalid(format!("Наблюдение за игрой прервано: {e}")))?
    }
}

const TAIL_LINES: usize = 400;

/// Память процесса (RSS), МБ. Только macOS и Linux: через `ps`.
pub fn rss_mb(pid: u32) -> Option<u64> {
    if cfg!(windows) {
        return None;
    }
    let out = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    let kb: u64 = String::from_utf8_lossy(&out.stdout).trim().parse().ok()?;
    Some(kb / 1024)
}

pub async fn spawn(
    cmd: &LaunchCommand,
    log_file: &Path,
    masker: Masker,
    on_event: Option<GameEventFn>,
) -> Result<GameProcess> {
    if let Some(parent) = log_file.parent() {
        tokio::fs::create_dir_all(parent).await.at(parent)?;
    }
    tokio::fs::create_dir_all(&cmd.cwd).await.at(&cmd.cwd)?;
    let mut command = tokio::process::Command::new(&cmd.program);
    command
        .args(&cmd.args)
        .envs(cmd.env.iter().map(|(k, v)| (k, v)))
        .current_dir(&cmd.cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(false);
    #[cfg(windows)]
    command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    let started_at = SystemTime::now();
    let started = Instant::now();
    let mut child = command.spawn().map_err(|e| {
        Error::Java(format!(
            "Не удалось запустить Java ({}): {e}",
            cmd.program.display()
        ))
    })?;
    let pid = child.id();
    if let Some(cb) = &on_event {
        cb(GameEvent::Started { pid });
    }

    let file = tokio::fs::File::create(log_file).await.at(log_file)?;
    let writer = Arc::new(tokio::sync::Mutex::new(file));
    let tail = Arc::new(Mutex::new(VecDeque::with_capacity(TAIL_LINES)));
    let masker = Arc::new(masker);

    let mut readers = Vec::new();
    for (stream, pipe) in [
        (
            Stream::Stdout,
            child
                .stdout
                .take()
                .map(|s| Box::new(s) as Box<dyn tokio::io::AsyncRead + Unpin + Send>),
        ),
        (
            Stream::Stderr,
            child
                .stderr
                .take()
                .map(|s| Box::new(s) as Box<dyn tokio::io::AsyncRead + Unpin + Send>),
        ),
    ] {
        let Some(pipe) = pipe else { continue };
        let writer = writer.clone();
        let tail = tail.clone();
        let masker = masker.clone();
        let on_event = on_event.clone();
        readers.push(tokio::spawn(async move {
            let mut lines = BufReader::new(pipe).lines();
            let mut assembler = Log4jAssembler::default();
            let mut emit = |raw: String| {
                let masked = masker.mask(&raw);
                {
                    let mut t = tail.lock().unwrap_or_else(|p| p.into_inner());
                    if t.len() == TAIL_LINES {
                        t.pop_front();
                    }
                    t.push_back(masked.clone());
                }
                if let Some(cb) = &on_event {
                    cb(GameEvent::Log {
                        stream,
                        line: masked.clone(),
                    });
                }
                masked
            };
            while let Ok(Some(line)) = lines.next_line().await {
                let ready: Vec<String> = assembler.push(&line).into_iter().map(&mut emit).collect();
                if !ready.is_empty() {
                    let mut w = writer.lock().await;
                    for l in ready {
                        let _ = w.write_all(l.as_bytes()).await;
                        let _ = w.write_all(b"\n").await;
                    }
                }
            }
            let rest: Vec<String> = assembler.flush().into_iter().map(&mut emit).collect();
            let mut w = writer.lock().await;
            for l in rest {
                let _ = w.write_all(l.as_bytes()).await;
                let _ = w.write_all(b"\n").await;
            }
        }));
    }

    // Пик памяти: замер раз в 5 секунд, пока игра работает.
    let peak = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let sampler = pid.map(|pid| {
        let peak = peak.clone();
        tokio::spawn(async move {
            loop {
                if let Some(mb) = tokio::task::spawn_blocking(move || rss_mb(pid))
                    .await
                    .ok()
                    .flatten()
                {
                    peak.fetch_max(mb, std::sync::atomic::Ordering::Relaxed);
                }
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        })
    });

    let stop = Arc::new(Notify::new());
    let stop_c = stop.clone();
    let log_path = log_file.to_path_buf();
    let game_dir = cmd.cwd.clone();
    let done = tokio::spawn(async move {
        let mut killed = false;
        let status = tokio::select! {
            s = child.wait() => s,
            _ = stop_c.notified() => {
                killed = true;
                let _ = child.kill().await;
                child.wait().await
            }
        };
        if let Some(s) = sampler {
            s.abort();
        }
        for r in readers {
            let _ = r.await;
        }
        {
            let mut w = writer.lock().await;
            let _ = w.flush().await;
        }
        let status =
            status.map_err(|e| Error::invalid(format!("Ошибка ожидания процесса игры: {e}")))?;
        let code = status.code();
        let crash_report = newest_crash_report(&game_dir, started_at);
        // 130/143 — игру закрыли сигналом (Ctrl+C, завершение сеанса), это не краш.
        let closed_by_signal = matches!(code, Some(130) | Some(143));
        let crashed = !killed && (crash_report.is_some() || (code != Some(0) && !closed_by_signal));
        let tail = tail
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .iter()
            .cloned()
            .collect();
        Ok(ExitInfo {
            code,
            duration_secs: started.elapsed().as_secs(),
            killed,
            crashed,
            crash_report,
            log_file: log_path,
            tail,
            analysis: None,
            peak_ram_mb: Some(peak.load(std::sync::atomic::Ordering::Relaxed)).filter(|m| *m > 0),
            prepare_secs: None,
        })
    });

    Ok(GameProcess { pid, stop, done })
}

/// Crash-report, созданный после запуска (игра кладёт их в `crash-reports/`).
fn newest_crash_report(game_dir: &Path, since: SystemTime) -> Option<PathBuf> {
    std::fs::read_dir(game_dir.join("crash-reports"))
        .ok()?
        .flatten()
        .filter_map(|e| {
            let modified = e.metadata().ok()?.modified().ok()?;
            (modified >= since).then(|| (modified, e.path()))
        })
        .max_by_key(|(m, _)| *m)
        .map(|(_, p)| p)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn tmpdir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("quarry-proc-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[tokio::test]
    async fn captures_masked_output_and_exit_code() {
        let d = tmpdir("exit");
        let cmd = LaunchCommand {
            program: "/bin/sh".into(),
            args: vec![
                "-c".into(),
                "echo 'token is supersecret12345'; echo \"env:$QUARRY_T\"; echo err >&2; exit 3"
                    .into(),
            ],
            cwd: d.join("game"),
            env: vec![("QUARRY_T".into(), "envok".into())],
            unresolved: vec![],
        };
        let events = Arc::new(Mutex::new(Vec::new()));
        let ev = events.clone();
        let cb: GameEventFn = Arc::new(move |e| ev.lock().unwrap().push(e));
        let log = d.join("logs/run.log");
        let p = spawn(
            &cmd,
            &log,
            Masker::new().with_secret("supersecret12345"),
            Some(cb),
        )
        .await
        .unwrap();
        let info = p.wait().await.unwrap();
        assert_eq!(info.code, Some(3));
        assert!(info.crashed && !info.killed);
        let text = std::fs::read_to_string(&log).unwrap();
        assert!(
            !text.contains("supersecret12345"),
            "токен не попадает в файл лога"
        );
        assert!(text.contains("err"));
        assert!(
            text.contains("env:envok"),
            "переменные окружения доходят до игры"
        );
        assert!(
            events
                .lock()
                .unwrap()
                .iter()
                .any(|e| matches!(e, GameEvent::Started { .. }))
        );
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[tokio::test]
    async fn stop_kills_the_game() {
        let d = tmpdir("kill");
        let cmd = LaunchCommand {
            program: "/bin/sh".into(),
            args: vec!["-c".into(), "sleep 30".into()],
            cwd: d.clone(),
            env: vec![],
            unresolved: vec![],
        };
        let p = spawn(&cmd, &d.join("run.log"), Masker::new(), None)
            .await
            .unwrap();
        let stop = p.stopper();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            stop.notify_one();
        });
        let info = p.wait().await.unwrap();
        assert!(info.killed);
        assert!(!info.crashed, "остановка пользователем — не краш");
        assert!(info.duration_secs < 10);
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn rss_of_own_process() {
        let mb = rss_mb(std::process::id()).expect("ps доступен");
        assert!(mb > 0 && mb < 100_000, "{mb}");
        assert!(rss_mb(u32::MAX - 7).is_none(), "нет такого процесса");
    }
}
