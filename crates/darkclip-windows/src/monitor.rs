//! `WinMonitor`: one thread with a message-only window listening for `WM_CLIPBOARDUPDATE`.
//!
//! The thread resolves the source at notification time (F-01) through the resolver closure
//! and sends `ChangeSignal { seq, at, source }`. It never opens the clipboard and never reads
//! content; it reports and never decides (GC-04).

use crate::win::{self, MessageWindow, RawWindow};
use darkclip_core::{ClipboardSource, Timestamp};
use darkclip_platform::{ChangeSignal, ClipboardMonitor, PlatformError};
use std::rc::Rc;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::{SystemTime, UNIX_EPOCH};

/// Resolves the clipboard's source app on the monitor thread at signal time.
pub type SignalSourceFn = Box<dyn Fn() -> Option<ClipboardSource> + Send>;

/// The live window, or `None` once the monitor thread has started tearing it down. The stop
/// message is posted only while holding this lock, so it never reaches a destroyed window.
type WindowSlot = Arc<Mutex<Option<RawWindow>>>;

fn lock(slot: &WindowSlot) -> MutexGuard<'_, Option<RawWindow>> {
    slot.lock().unwrap_or_else(PoisonError::into_inner)
}

fn now() -> Timestamp {
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX));
    Timestamp(ms)
}

struct Running {
    window: WindowSlot,
    /// Returns the resolver so the monitor can be started again.
    thread: JoinHandle<Option<SignalSourceFn>>,
}

/// Win32 clipboard monitor (ADR-0002).
pub struct WinMonitor {
    resolver: Option<SignalSourceFn>,
    running: Option<Running>,
}

impl WinMonitor {
    pub fn new(resolver: SignalSourceFn) -> Self {
        Self {
            resolver: Some(resolver),
            running: None,
        }
    }

    /// Whether a monitor thread is running (tests).
    pub fn is_running(&self) -> bool {
        self.running.is_some()
    }
}

/// Body of the monitor thread. Returns the resolver for a later restart.
fn monitor_thread(
    resolver: SignalSourceFn,
    tx: Sender<ChangeSignal>,
    slot: &WindowSlot,
    ready: &Sender<Result<(), PlatformError>>,
) -> Option<SignalSourceFn> {
    let resolver = Rc::new(resolver);
    let on_update: win::UpdateHandler = {
        let resolver = Rc::clone(&resolver);
        Box::new(move || {
            let signal = ChangeSignal {
                seq: u64::from(win::clipboard_sequence()),
                at: now(),
                source: resolver(),
            };
            // A dropped receiver ends the loop cleanly.
            tx.send(signal).is_ok()
        })
    };
    match MessageWindow::create(Some(on_update)) {
        Err(e) => {
            let _ = ready.send(Err(e));
        }
        Ok(window) => {
            *lock(slot) = Some(window.raw());
            let _ = ready.send(Ok(()));
            window.run();
            // From here on no stop message is posted; then the window is torn down.
            *lock(slot) = None;
            drop(window);
        }
    }
    Rc::into_inner(resolver)
}

impl ClipboardMonitor for WinMonitor {
    /// Returns once the window exists and listens, or with the error that prevented it.
    fn start(&mut self, tx: Sender<ChangeSignal>) -> Result<(), PlatformError> {
        if self.running.is_some() {
            return Ok(());
        }
        let resolver = self.resolver.take().unwrap_or_else(|| Box::new(|| None));
        let slot: WindowSlot = Arc::new(Mutex::new(None));
        let (ready_tx, ready_rx) = mpsc::channel();
        let thread_slot = Arc::clone(&slot);
        let thread = std::thread::Builder::new()
            .name("darkclip-win-monitor".into())
            .spawn(move || monitor_thread(resolver, tx, &thread_slot, &ready_tx))
            .map_err(|_| PlatformError::Unsupported("cannot start monitor thread"))?;
        match ready_rx.recv() {
            Ok(Ok(())) => {
                self.running = Some(Running {
                    window: slot,
                    thread,
                });
                Ok(())
            }
            Ok(Err(e)) => {
                self.resolver = thread.join().ok().flatten();
                Err(e)
            }
            Err(_) => {
                self.resolver = thread.join().ok().flatten();
                Err(PlatformError::Unsupported("monitor thread ended"))
            }
        }
    }

    /// Posts the stop message and joins the thread. Idempotent.
    fn stop(&mut self) {
        let Some(running) = self.running.take() else {
            return;
        };
        let posted = {
            let w = lock(&running.window);
            // `None`: the loop already ended (receiver dropped) and is tearing down.
            w.is_none_or(|w| w.post_stop())
        };
        if posted {
            self.resolver = running.thread.join().ok().flatten();
        }
        // A failed post (message queue full) leaves the thread detached rather than blocking
        // the caller; its window is released when the thread ends or the process exits.
    }
}

impl Drop for WinMonitor {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::win::test_support;
    use std::sync::mpsc::Receiver;
    use std::time::{Duration, Instant};

    fn monitor() -> WinMonitor {
        WinMonitor::new(Box::new(|| None))
    }

    /// Stops `m` on another thread and fails if that takes 2 s or more.
    fn stop_within_2s(m: WinMonitor) -> WinMonitor {
        let (done_tx, done_rx) = mpsc::channel();
        let t = std::thread::spawn(move || {
            let mut m = m;
            m.stop();
            let _ = done_tx.send(());
            m
        });
        done_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("stop did not finish within 2 s");
        let m = t.join().unwrap();
        assert!(!m.is_running());
        m
    }

    /// The monitor thread is gone once the signal channel's sender (held by it) is dropped.
    fn assert_thread_gone(rx: &Receiver<ChangeSignal>) {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            match rx.recv_timeout(Duration::from_millis(50)) {
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
                // A signal from another app's copy meanwhile: keep draining.
                Ok(_) | Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
            assert!(
                Instant::now() < deadline,
                "monitor thread still holds the sender"
            );
        }
    }

    #[test]
    fn monitor_starts_and_stops_cleanly() {
        let mut m = monitor();
        for _ in 0..2 {
            let (tx, rx) = mpsc::channel();
            m.start(tx).unwrap();
            assert!(m.is_running());
            m = stop_within_2s(m);
            assert_thread_gone(&rx);
            // Idempotent.
            m.stop();
        }
        // Two fresh monitors back to back.
        for _ in 0..2 {
            let mut m = monitor();
            let (tx, rx) = mpsc::channel();
            m.start(tx).unwrap();
            stop_within_2s(m);
            assert_thread_gone(&rx);
        }
    }

    #[test]
    fn drop_stops_monitor() {
        let mut m = monitor();
        let (tx, rx) = mpsc::channel();
        m.start(tx).unwrap();
        let (done_tx, done_rx) = mpsc::channel();
        std::thread::spawn(move || {
            drop(m);
            let _ = done_tx.send(());
        });
        done_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("drop did not stop the monitor within 2 s");
        assert_thread_gone(&rx);
    }

    #[test]
    fn stop_without_start_is_a_no_op() {
        let mut m = monitor();
        m.stop();
        assert!(!m.is_running());
    }

    /// Waits for a signal whose seq is at least `seq` (other apps may copy meanwhile).
    fn wait_signal(rx: &Receiver<ChangeSignal>, seq: u64, within: Duration) -> ChangeSignal {
        let deadline = Instant::now() + within;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let s = rx.recv_timeout(left).expect("no signal in time");
            if s.seq >= seq {
                return s;
            }
        }
    }

    #[test]
    #[ignore = "writes the real clipboard"]
    fn real_clipboard_write_same_process_signals() {
        let resolved = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let counter = Arc::clone(&resolved);
        let mut m = WinMonitor::new(Box::new(move || {
            counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            None
        }));
        let (tx, rx) = mpsc::channel();
        m.start(tx).unwrap();
        let mut written = None;
        for _ in 0..10 {
            match test_support::write_text("darkclip test 101") {
                Ok(seq) => {
                    written = Some(seq);
                    break;
                }
                Err(PlatformError::Busy) => std::thread::sleep(Duration::from_millis(20)),
                Err(e) => panic!("write failed: {e}"),
            }
        }
        let seq = u64::from(written.expect("clipboard stayed busy"));
        let s = wait_signal(&rx, seq, Duration::from_millis(500));
        assert_eq!(s.seq, seq);
        assert!(resolved.load(std::sync::atomic::Ordering::SeqCst) >= 1);
        m.stop();
    }

    #[test]
    #[ignore = "writes the real clipboard"]
    fn real_clipboard_write_other_process_signals() {
        let mut m = monitor();
        let (tx, rx) = mpsc::channel();
        m.start(tx).unwrap();
        let before = u64::from(win::clipboard_sequence());
        let status = std::process::Command::new("powershell")
            .args(["-NoProfile", "-Command", "Set-Clipboard -Value x"])
            .status()
            .unwrap();
        assert!(status.success());
        let s = wait_signal(&rx, before + 1, Duration::from_secs(5));
        assert!(s.seq > before);
        m.stop();
    }
}
