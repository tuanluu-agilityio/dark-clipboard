//! Content-free file log (GC-06): `<data dir>/logs/dark-clipboard.log`, capped at 1 MiB with
//! one rotation (`dark-clipboard.log.1`).
//!
//! A line is `<unix_ms> <event> [key=value ...]`. Events and keys are `&'static str`, values are
//! numbers or `&'static str`. There is no way to pass a `String` or a model type in.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_LOG_SIZE: u64 = 1024 * 1024;

/// A value in a log line: a number or a static name.
#[derive(Clone, Copy)]
pub enum Val {
    Num(u64),
    Name(&'static str),
}

struct Inner {
    path: PathBuf,
    rotated: PathBuf,
    file: File,
    size: u64,
}

/// Shared between the main thread and the event thread.
pub struct Log {
    inner: Mutex<Option<Inner>>,
}

impl Log {
    /// Opens (creating) the log. Failure to open leaves a logger that drops every line; the
    /// app does not stop because of a log file.
    pub fn open(data_dir: &Path) -> Log {
        Log {
            inner: Mutex::new(Self::open_inner(data_dir)),
        }
    }

    fn open_inner(data_dir: &Path) -> Option<Inner> {
        let dir = data_dir.join("logs");
        fs::create_dir_all(&dir).ok()?;
        let path = dir.join("dark-clipboard.log");
        let rotated = dir.join("dark-clipboard.log.1");
        let size = fs::metadata(&path).map_or(0, |m| m.len());
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .ok()?;
        Some(Inner {
            path,
            rotated,
            file,
            size,
        })
    }

    pub fn event(&self, event: &'static str, fields: &[(&'static str, Val)]) {
        let mut line = format!("{} {}", now_ms(), event);
        for (k, v) in fields {
            match v {
                Val::Num(n) => line.push_str(&format!(" {k}={n}")),
                Val::Name(s) => line.push_str(&format!(" {k}={s}")),
            }
        }
        line.push('\n');
        let mut guard = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(inner) = guard.as_mut() else {
            return;
        };
        let add = u64::try_from(line.len()).unwrap_or(u64::MAX);
        if inner.size.saturating_add(add) > MAX_LOG_SIZE {
            inner.rotate();
        }
        if inner.file.write_all(line.as_bytes()).is_ok() {
            inner.size = inner.size.saturating_add(add);
        }
    }
}

impl Inner {
    /// Keeps one old file: the current log becomes `.log.1` (replacing the previous one).
    fn rotate(&mut self) {
        let _ = fs::remove_file(&self.rotated);
        let _ = fs::rename(&self.path, &self.rotated);
        if let Ok(f) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
        {
            self.file = f;
            self.size = 0;
        }
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotates_once_at_the_cap() {
        let dir = tempfile::tempdir().unwrap();
        let log = Log::open(dir.path());
        for _ in 0..40_000 {
            log.event("start", &[("n", Val::Num(1)), ("m", Val::Name("native"))]);
        }
        let logs = dir.path().join("logs");
        let cur = fs::metadata(logs.join("dark-clipboard.log")).unwrap().len();
        assert!(cur <= MAX_LOG_SIZE);
        assert!(logs.join("dark-clipboard.log.1").exists());
        assert!(!logs.join("dark-clipboard.log.2").exists());
    }

    #[test]
    fn line_format() {
        let dir = tempfile::tempdir().unwrap();
        let log = Log::open(dir.path());
        log.event("stop", &[("ms", Val::Num(7)), ("why", Val::Name("timer"))]);
        let t = fs::read_to_string(dir.path().join("logs/dark-clipboard.log")).unwrap();
        let parts: Vec<&str> = t.trim_end().split(' ').collect();
        assert_eq!(&parts[1..], ["stop", "ms=7", "why=timer"]);
    }
}
