//! The translate-in-place flow (Task 605): pure, every OS and app call behind [`FlowSys`].
//!
//! Order (spec §5.1, plan §605 as changed by 603b): the foreground target, the blocks (no
//! key is sent and nothing is armed when one hits), the hotkey keys released, the restore
//! candidate taken, one claim armed, Ctrl+C (Ctrl+A, Ctrl+C only while the clipboard has not
//! moved), the text checks, the request, the compare-and-write of the translation, Ctrl+V,
//! and the compare-and-write restore of the previous clipboard.
//!
//! Privacy: the flow never reads the clipboard (GC-03); it gets the claimed text from the
//! capture actor. Text crosses the seam only as `Redacted` or [`Cleared`] and every `String`
//! the flow owns is wiped on every path (`darkclip_core::sensitive::wipe_string`). Nothing
//! here logs, and no type here carries or prints text (GC-06).

// Run by the Windows host only; other platforms compile it and its tests (F-19).
#![cfg_attr(not(windows), allow(dead_code))]

use darkclip_ai::{AiError, TransformResult, MAX_TRANSLATE_CHARS};
use darkclip_app::capture::{ClaimError, ClaimedText, CLAIM_REPLY_SLACK_MS, CLAIM_WINDOW_MS};
use darkclip_core::sensitive::wipe_string;
use darkclip_core::{ClipId, Redacted};
use darkclip_platform::{PasteTarget, PlatformError};
use std::sync::mpsc::{Receiver, TryRecvError};

/// How long the flow waits for the shortcut's keys to come up before arming (I1 M1).
pub(crate) const KEYS_UP_WAIT_MS: u64 = 1000;
/// How long the plain Ctrl+C has to move the clipboard before the select-all retry. 1200
/// since Task 610: a slow app (Chrome) answered later than 400 ms, so the retry selected the
/// whole field. The cost: with nothing selected, select-all comes about 1.2 s after the press.
pub(crate) const COPY_WAIT_MS: u64 = 1_200;
/// The longest wait for the claim reply, counted from arming (603b).
pub(crate) const CLAIM_WAIT_MS: u64 = CLAIM_WINDOW_MS + CLAIM_REPLY_SLACK_MS;
/// The pause between the paste and the restore of the previous clipboard.
pub(crate) const RESTORE_DELAY_MS: u64 = 400;
/// The poll step of every wait.
pub(crate) const POLL_MS: u64 = 10;
/// A guard over the request wait: the transport ends every call within 30 s
/// (`ai_net::TIMEOUT`); this only stops a flow whose provider never answers.
pub(crate) const API_GUARD_MS: u64 = 35_000;

/// A reply that may hold text: wiped when the flow gives up on it.
pub(crate) trait Wipe {
    fn wipe(&mut self);
}

impl Wipe for ClaimedText {
    fn wipe(&mut self) {
        wipe_string(&mut self.text.0);
    }
}

impl Wipe for TransformResult {
    fn wipe(&mut self) {
        wipe_string(&mut self.text.0);
    }
}

/// Takes every reply already in `rx` and wipes its text (a late reply that landed in the same
/// tick as a cancel or a timeout). Returns how many texts were wiped.
pub(crate) fn drain<T: Wipe, E>(rx: &Receiver<Result<T, E>>) -> usize {
    let mut n = 0;
    while let Ok(r) = rx.try_recv() {
        if let Ok(mut v) = r {
            v.wipe();
            n += 1;
        }
    }
    n
}

/// Why a press does nothing. Static data only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Block {
    /// Not active: off, or the consent was never given.
    Off,
    NoKey,
    Private,
    /// A console or terminal window (D-095): Ctrl+C would interrupt it.
    Terminal,
    /// The target's process gave no `app_id` (I1 M2): it cannot be matched or excluded.
    UnknownApp,
    ExcludedApp,
    NotNormalWindow,
    /// The shortcut's keys did not come up in time, or the copy was refused for held keys.
    KeysHeld,
}

impl Block {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Block::Off => "off",
            Block::NoKey => "no_key",
            Block::Private => "private",
            Block::Terminal => "terminal",
            Block::UnknownApp => "unknown_app",
            Block::ExcludedApp => "excluded_app",
            Block::NotNormalWindow => "not_normal_window",
            Block::KeysHeld => "keys_held",
        }
    }
}

/// Why a started translation ended without one. Content-free: the payloads are codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FlowError {
    NothingCopied,
    Claim(ClaimError),
    Sensitive,
    Empty,
    TooLong,
    Ai(AiError),
    /// `write_own_if` found the clipboard moved (a newer copy of the user): nothing written.
    ClipboardChanged,
    /// The paste was refused; the translation stays on the clipboard
    /// (`Outcome::LeftOnClipboard`).
    NotPasted,
    /// The translation could not be written to the clipboard.
    WriteFailed,
}

impl FlowError {
    pub(crate) fn name(self) -> &'static str {
        match self {
            FlowError::NothingCopied => "nothing_copied",
            FlowError::Claim(ClaimError::Skipped(_)) => "claim_skipped",
            FlowError::Claim(ClaimError::NoCloud) => "claim_no_cloud",
            FlowError::Claim(ClaimError::NotText) => "claim_not_text",
            FlowError::Claim(ClaimError::TooLong) => "claim_too_long",
            FlowError::Claim(ClaimError::Expired) => "claim_expired",
            FlowError::Claim(ClaimError::Busy) => "claim_busy",
            FlowError::Sensitive => "sensitive",
            FlowError::Empty => "empty",
            FlowError::TooLong => "too_long",
            FlowError::Ai(AiError::NoKey) => "ai_no_key",
            FlowError::Ai(AiError::TooLong) => "ai_too_long",
            FlowError::Ai(AiError::Empty) => "ai_empty",
            FlowError::Ai(AiError::Network) => "ai_network",
            FlowError::Ai(AiError::Timeout) => "ai_timeout",
            FlowError::Ai(AiError::Auth) => "ai_auth",
            FlowError::Ai(AiError::RateLimited) => "ai_rate_limited",
            FlowError::Ai(AiError::Overloaded) => "ai_overloaded",
            FlowError::Ai(AiError::Http(_)) => "ai_http",
            FlowError::Ai(AiError::Refused) => "ai_refused",
            FlowError::Ai(AiError::Truncated) => "ai_truncated",
            FlowError::Ai(AiError::BadResponse) => "ai_bad_response",
            FlowError::ClipboardChanged => "clipboard_changed",
            FlowError::NotPasted => "not_pasted",
            FlowError::WriteFailed => "write_failed",
        }
    }
}

/// What the overlay shows. Counts and codes only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OverlayState {
    Sending { chars: usize },
    Done,
    Cancelled,
    LeftOnClipboard,
    Blocked(Block),
    Failed(FlowError),
}

/// How a press ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Outcome {
    Replaced,
    LeftOnClipboard,
    Cancelled,
    Blocked(Block),
    Failed(FlowError),
}

impl Outcome {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Outcome::Replaced => "replaced",
            Outcome::LeftOnClipboard => "left_on_clipboard",
            Outcome::Cancelled => "cancelled",
            Outcome::Blocked(_) => "blocked",
            Outcome::Failed(_) => "failed",
        }
    }

    /// The reason of a block or failure, for the log.
    pub(crate) fn reason(self) -> Option<&'static str> {
        match self {
            Outcome::Blocked(b) => Some(b.name()),
            Outcome::Failed(e) => Some(e.name()),
            Outcome::Replaced | Outcome::LeftOnClipboard | Outcome::Cancelled => None,
        }
    }

    /// The overlay's final state for this outcome (`Replaced` showed `Done` at paste time).
    fn overlay(self) -> Option<OverlayState> {
        match self {
            Outcome::Replaced => None,
            Outcome::LeftOnClipboard => Some(OverlayState::LeftOnClipboard),
            Outcome::Cancelled => Some(OverlayState::Cancelled),
            Outcome::Blocked(b) => Some(OverlayState::Blocked(b)),
            Outcome::Failed(e) => Some(OverlayState::Failed(e)),
        }
    }
}

/// Text that passed the empty, cap and sensitive checks; only [`run`] makes one, so the
/// provider can only ever be handed checked text (I1 item 1). Wiped on drop; `Debug` prints
/// `<redacted>`.
pub(crate) struct Cleared(Redacted<String>);

impl Cleared {
    /// The checked text, moved out (the empty shell left behind is dropped).
    pub(crate) fn into_inner(mut self) -> Redacted<String> {
        Redacted(std::mem::take(&mut self.0 .0))
    }
}

impl Drop for Cleared {
    fn drop(&mut self) {
        wipe_string(&mut self.0 .0);
    }
}

impl std::fmt::Debug for Cleared {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("<redacted>")
    }
}

/// The facts behind the blocks, in the order they are checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BlockFacts {
    /// Enabled and consented (`AiSettings::consented`).
    pub consented: bool,
    pub has_key: bool,
    pub private: bool,
    pub terminal: bool,
    pub app_known: bool,
    pub excluded: bool,
}

/// The first block that applies: `Off`, `NoKey`, `Private`, `Terminal`, `UnknownApp`,
/// `ExcludedApp`, in this order.
pub(crate) fn first_block(f: &BlockFacts) -> Option<Block> {
    if !f.consented {
        Some(Block::Off)
    } else if !f.has_key {
        Some(Block::NoKey)
    } else if f.private {
        Some(Block::Private)
    } else if f.terminal {
        Some(Block::Terminal)
    } else if !f.app_known {
        Some(Block::UnknownApp)
    } else if f.excluded {
        Some(Block::ExcludedApp)
    } else {
        None
    }
}

/// Every real call the flow makes. The host implements it over the app API and the OS; the
/// tests over a scripted fake.
pub(crate) trait FlowSys {
    /// The foreground window of another process, checked like a paste target.
    fn target(&mut self) -> Option<PasteTarget>;
    /// The first block for `t` (see [`first_block`]).
    fn blocked(&mut self, t: &PasteTarget) -> Option<Block>;
    /// `None` when the seq cannot be read (the flow then never sends the select-all retry).
    fn clipboard_seq(&mut self) -> Option<u64>;
    /// True once every modifier and the shortcut's key are up, waiting at most `wait` ms.
    fn keys_up(&mut self, wait: u64) -> bool;
    fn claim(&mut self, app_id: &str) -> Receiver<Result<ClaimedText, ClaimError>>;
    fn send_copy(&mut self, t: &PasteTarget, select_all: bool) -> Result<(), PlatformError>;
    /// The wait for the claim reply after the first `send_copy` ended (a reply, Esc or the
    /// timeout), for the content-free `copy_ms` of the log line (Task 610). No data.
    fn copy_wait_ended(&mut self) {}
    fn is_sensitive(&mut self, text: &str) -> bool;
    fn translate(&mut self, text: Cleared) -> Receiver<Result<TransformResult, AiError>>;
    /// The own write seq; `None` when the clipboard no longer holds `expect_seq`.
    fn write_own_if(&mut self, expect_seq: u64, text: Redacted<String>) -> Result<Option<u64>, ()>;
    fn paste(&mut self, t: &PasteTarget) -> Result<(), PlatformError>;
    fn restore_candidate(&mut self) -> Option<ClipId>;
    /// `false` when the clipboard no longer holds `expect_seq`.
    fn restore_if(&mut self, expect_seq: u64, id: ClipId) -> Result<bool, ()>;
    fn overlay(&mut self, s: OverlayState);
    fn sleep(&mut self, ms: u64);
    /// Esc was pressed.
    fn cancelled(&mut self) -> bool;
}

/// The waits, in milliseconds (the constants above; tests may shorten them).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FlowConfig {
    pub keys_up_wait_ms: u64,
    pub copy_wait_ms: u64,
    pub claim_wait_ms: u64,
    pub restore_delay_ms: u64,
    pub poll_ms: u64,
    pub api_guard_ms: u64,
}

impl Default for FlowConfig {
    fn default() -> Self {
        Self {
            keys_up_wait_ms: KEYS_UP_WAIT_MS,
            copy_wait_ms: COPY_WAIT_MS,
            claim_wait_ms: CLAIM_WAIT_MS,
            restore_delay_ms: RESTORE_DELAY_MS,
            poll_ms: POLL_MS,
            api_guard_ms: API_GUARD_MS,
        }
    }
}

/// How a wait on a receiver ended.
enum Waited<T> {
    Got(T),
    Cancelled,
    TimedOut,
    Gone,
}

/// Waits on `rx` in `poll_ms` steps (counted through `sys.sleep`) until `limit` ms have passed
/// since `*elapsed`'s origin; Esc ends it at any step.
fn wait_on<S: FlowSys, T>(
    sys: &mut S,
    rx: &Receiver<T>,
    elapsed: &mut u64,
    limit: u64,
    poll_ms: u64,
) -> Waited<T> {
    loop {
        match rx.try_recv() {
            Ok(v) => return Waited::Got(v),
            Err(TryRecvError::Disconnected) => return Waited::Gone,
            Err(TryRecvError::Empty) => {}
        }
        if sys.cancelled() {
            return Waited::Cancelled;
        }
        if *elapsed >= limit {
            return Waited::TimedOut;
        }
        sys.sleep(poll_ms);
        *elapsed = elapsed.saturating_add(poll_ms.max(1));
    }
}

fn keys_held(e: &PlatformError) -> bool {
    matches!(e, PlatformError::Unsupported(r) if *r == darkclip_platform::REFUSAL_KEYS_HELD)
}

/// Runs one press. Ends with the overlay's final state (`Done` was already shown at paste
/// time for `Replaced`).
pub(crate) fn run(sys: &mut impl FlowSys, cfg: &FlowConfig) -> Outcome {
    let outcome = run_inner(sys, cfg);
    if let Some(s) = outcome.overlay() {
        sys.overlay(s);
    }
    outcome
}

fn run_inner<S: FlowSys>(sys: &mut S, cfg: &FlowConfig) -> Outcome {
    // 1-3: target, blocks, keys. Nothing is sent and nothing is armed before they pass.
    let Some(t) = sys.target() else {
        return Outcome::Blocked(Block::NotNormalWindow);
    };
    if let Some(b) = sys.blocked(&t) {
        return Outcome::Blocked(b);
    }
    let Some(app_id) = t.app_id.clone().filter(|a| !a.is_empty()) else {
        return Outcome::Blocked(Block::UnknownApp);
    };
    if !sys.keys_up(cfg.keys_up_wait_ms) {
        return Outcome::Blocked(Block::KeysHeld);
    }
    // 4. The previous clipboard, taken before our copy changes it (I1 R5).
    let candidate = sys.restore_candidate();
    // 5. One claim for both copy attempts (a second arm would replace it).
    let seq0 = sys.clipboard_seq();
    let rx = sys.claim(&app_id);
    // A reply before any key: arming failed (the host could not reach the actor), or the
    // target changed the clipboard by itself. Either way no key is sent.
    let claimed = match rx.try_recv() {
        Ok(Err(e)) => return Outcome::Failed(claim_failure(e)),
        // A change the flow did not ask for (no key was sent yet): wiped, never sent (A1).
        Ok(Ok(mut c)) => {
            c.wipe();
            drain(&rx);
            return Outcome::Failed(FlowError::NothingCopied);
        }
        Err(_) => {
            if let Err(e) = sys.send_copy(&t, false) {
                // The slot expires by itself; a late text is wiped by the actor.
                return Outcome::Blocked(if keys_held(&e) {
                    Block::KeysHeld
                } else {
                    Block::NotNormalWindow
                });
            }
            let waited = wait_for_claim(sys, cfg, &rx, &t, seq0);
            sys.copy_wait_ended();
            match waited {
                Ok(c) => c,
                Err(o) => {
                    // A text that landed with the cancel or the timeout is wiped.
                    drain(&rx);
                    return o;
                }
            }
        }
    };
    drop(rx);
    let ClaimedText { text, seq } = claimed;
    let mut text = text.0;
    let outcome = after_claim(sys, cfg, &t, &mut text, seq, candidate);
    wipe_string(&mut text);
    outcome
}

fn claim_failure(e: ClaimError) -> FlowError {
    match e {
        ClaimError::Expired => FlowError::NothingCopied,
        other => FlowError::Claim(other),
    }
}

/// Step 5's waits: the plain copy, the select-all retry only while the clipboard has not
/// moved (I1 M4), then the claim reply within `claim_wait_ms` of arming.
fn wait_for_claim<S: FlowSys>(
    sys: &mut S,
    cfg: &FlowConfig,
    rx: &Receiver<Result<ClaimedText, ClaimError>>,
    t: &PasteTarget,
    seq0: Option<u64>,
) -> Result<ClaimedText, Outcome> {
    let mut elapsed = 0u64;
    let mut moved = false;
    let mut early = None;
    while elapsed < cfg.copy_wait_ms {
        match rx.try_recv() {
            Ok(r) => {
                early = Some(r);
                break;
            }
            Err(TryRecvError::Disconnected) => {
                return Err(Outcome::Failed(FlowError::NothingCopied))
            }
            Err(TryRecvError::Empty) => {}
        }
        if sys.cancelled() {
            return Err(Outcome::Cancelled);
        }
        if let (Some(a), Some(b)) = (seq0, sys.clipboard_seq()) {
            if a != b {
                moved = true;
                break;
            }
        }
        sys.sleep(cfg.poll_ms);
        elapsed = elapsed.saturating_add(cfg.poll_ms.max(1));
    }
    if early.is_none() && !moved {
        if sys.cancelled() {
            return Err(Outcome::Cancelled);
        }
        // Re-checked right before the retry: a change that landed meanwhile is the copy. An
        // unknown seq (either one) never gives a select-all.
        let unchanged = seq0.is_some() && sys.clipboard_seq() == seq0;
        if unchanged {
            // A refused retry sends nothing; the wait below then ends as nothing copied.
            let _ = sys.send_copy(t, true);
        }
    }
    let reply = match early {
        Some(r) => r,
        None => match wait_on(sys, rx, &mut elapsed, cfg.claim_wait_ms, cfg.poll_ms) {
            Waited::Got(r) => r,
            Waited::Cancelled => return Err(Outcome::Cancelled),
            Waited::TimedOut | Waited::Gone => {
                return Err(Outcome::Failed(FlowError::NothingCopied))
            }
        },
    };
    reply.map_err(|e| Outcome::Failed(claim_failure(e)))
}

/// Steps 6-9 for a claimed text read at `seq`. `text` is wiped by the caller.
fn after_claim<S: FlowSys>(
    sys: &mut S,
    cfg: &FlowConfig,
    t: &PasteTarget,
    text: &mut String,
    seq: u64,
    candidate: Option<ClipId>,
) -> Outcome {
    // Every failure from here on puts the previous clipboard back over the target's copy.
    let restore_claimed = |sys: &mut S| {
        if let Some(id) = candidate {
            let _ = sys.restore_if(seq, id);
        }
    };
    // 6. Text checks: nothing is sent when one fails.
    let failed = if text.trim().is_empty() {
        Some(FlowError::Empty)
    } else if text.chars().count() > MAX_TRANSLATE_CHARS {
        Some(FlowError::TooLong)
    } else if sys.is_sensitive(text) {
        Some(FlowError::Sensitive)
    } else {
        None
    };
    if let Some(e) = failed {
        restore_claimed(sys);
        return Outcome::Failed(e);
    }
    let chars = text.chars().count();
    let cleared = Cleared(Redacted(std::mem::take(text)));
    // 7. The request; Esc drops the receiver and the host wipes a late answer.
    sys.overlay(OverlayState::Sending { chars });
    let rx = sys.translate(cleared);
    let mut elapsed = 0u64;
    let answer = match wait_on(sys, &rx, &mut elapsed, cfg.api_guard_ms, cfg.poll_ms) {
        Waited::Got(r) => r,
        Waited::Cancelled => {
            drain(&rx);
            drop(rx);
            restore_claimed(sys);
            return Outcome::Cancelled;
        }
        Waited::TimedOut => {
            drain(&rx);
            Err(AiError::Timeout)
        }
        Waited::Gone => Err(AiError::Network),
    };
    drop(rx);
    let translation = match answer {
        Ok(r) => r.text,
        Err(e) => {
            restore_claimed(sys);
            return Outcome::Failed(FlowError::Ai(e));
        }
    };
    // 8. Replace: written only over the claimed change (a newer copy of the user wins).
    let own = match sys.write_own_if(seq, translation) {
        Ok(Some(own)) => own,
        // The clipboard moved: the user's newer copy stays, and a restore could only refuse.
        Ok(None) => return Outcome::Failed(FlowError::ClipboardChanged),
        Err(()) => {
            restore_claimed(sys);
            return Outcome::Failed(FlowError::WriteFailed);
        }
    };
    let same_target = sys
        .target()
        .is_some_and(|now| now.window == t.window && now.pid == t.pid);
    if !same_target {
        return Outcome::LeftOnClipboard;
    }
    if sys.paste(t).is_err() {
        return fail(FlowError::NotPasted);
    }
    // Shown at paste time; the restore below runs behind it (G1).
    sys.overlay(OverlayState::Done);
    // 9. Restore, only while the clipboard still holds our translation.
    if let Some(id) = candidate {
        sys.sleep(cfg.restore_delay_ms);
        let _ = sys.restore_if(own, id);
    }
    Outcome::Replaced
}

/// The outcome of a failure: a refused paste leaves the translation on the clipboard.
fn fail(e: FlowError) -> Outcome {
    match e {
        FlowError::NotPasted => Outcome::LeftOnClipboard,
        other => Outcome::Failed(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::{self, Sender};

    const SEQ0: u64 = 100;
    const CLAIMED_SEQ: u64 = 101;
    const OWN_SEQ: u64 = 102;
    const SOURCE: &str = "Xin chào thế giới";
    const TRANSLATION: &str = "Hello world";

    fn target(window: u64, pid: u32) -> PasteTarget {
        PasteTarget {
            app_id: Some("editor.exe".into()),
            window: Some(window),
            pid: Some(pid),
        }
    }

    fn candidate() -> ClipId {
        ClipId::new_v7(darkclip_core::Timestamp(1_795_320_000_000))
    }

    /// When the claim reply arrives.
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum ReplyAt {
        /// At arming (an immediate refusal).
        Claim,
        /// With the n-th `send_copy` (1 = the plain copy, 2 = the select-all retry).
        Copy(usize),
        Never,
    }

    /// A scripted `FlowSys` that records every call in order.
    struct Fake {
        calls: Vec<String>,
        targets: Vec<Option<PasteTarget>>,
        block: Option<Block>,
        keys_up: bool,
        candidate: Option<ClipId>,
        /// The clipboard seq as a function of the virtual time since arming.
        seq_at: Box<dyn Fn(u64) -> u64>,
        reply: Option<Result<ClaimedText, ClaimError>>,
        reply_at: ReplyAt,
        claim_tx: Option<Sender<Result<ClaimedText, ClaimError>>>,
        copy_results: Vec<Result<(), PlatformError>>,
        copies: usize,
        sensitive: bool,
        answer: Option<Result<TransformResult, AiError>>,
        translate_tx: Option<Sender<Result<TransformResult, AiError>>>,
        write: Result<Option<u64>, ()>,
        paste: Result<(), PlatformError>,
        restore: Result<bool, ()>,
        /// `cancelled()` turns true once a call with this label was recorded.
        cancel_after: Option<&'static str>,
        /// The claim reply is delivered inside the first `cancelled()` that answers true.
        reply_with_cancel: bool,
        seq_unknown: bool,
        armed: bool,
        since_arm: u64,
        /// The claim reply is delivered by the first `sleep` that reaches this virtual time
        /// since arming (a slow app; Task 610).
        reply_after_ms: Option<u64>,
        overlays: Vec<OverlayState>,
        sent_text: Vec<usize>,
    }

    impl Fake {
        fn new() -> Self {
            Self {
                calls: Vec::new(),
                targets: vec![Some(target(7, 70))],
                block: None,
                keys_up: true,
                candidate: Some(candidate()),
                // The plain copy moves the clipboard 30 ms after arming.
                seq_at: Box::new(|t| if t >= 30 { CLAIMED_SEQ } else { SEQ0 }),
                reply: Some(Ok(ClaimedText {
                    text: Redacted(SOURCE.into()),
                    seq: CLAIMED_SEQ,
                })),
                reply_at: ReplyAt::Copy(1),
                claim_tx: None,
                copy_results: Vec::new(),
                copies: 0,
                sensitive: false,
                answer: Some(Ok(TransformResult {
                    text: Redacted(TRANSLATION.into()),
                })),
                translate_tx: None,
                write: Ok(Some(OWN_SEQ)),
                paste: Ok(()),
                restore: Ok(true),
                cancel_after: None,
                reply_with_cancel: false,
                seq_unknown: false,
                armed: false,
                since_arm: 0,
                reply_after_ms: None,
                overlays: Vec::new(),
                sent_text: Vec::new(),
            }
        }

        fn with_text(text: &str) -> Self {
            let mut f = Self::new();
            f.reply = Some(Ok(ClaimedText {
                text: Redacted(text.into()),
                seq: CLAIMED_SEQ,
            }));
            f
        }

        fn deliver(&mut self) {
            if let (Some(tx), Some(r)) = (self.claim_tx.as_ref(), self.reply.take()) {
                let _ = tx.send(r);
            }
        }

        fn has(&self, label: &str) -> bool {
            self.calls.iter().any(|c| c.starts_with(label))
        }

        fn count(&self, label: &str) -> usize {
            self.calls.iter().filter(|c| c.starts_with(label)).count()
        }

        fn run(&mut self) -> Outcome {
            run(self, &FlowConfig::default())
        }
    }

    impl FlowSys for Fake {
        fn target(&mut self) -> Option<PasteTarget> {
            self.calls.push("target".into());
            if self.targets.len() > 1 {
                self.targets.remove(0)
            } else {
                self.targets.first().cloned().flatten()
            }
        }
        fn blocked(&mut self, _t: &PasteTarget) -> Option<Block> {
            self.calls.push("blocked".into());
            self.block
        }
        fn clipboard_seq(&mut self) -> Option<u64> {
            self.calls.push("clipboard_seq".into());
            if self.seq_unknown {
                return None;
            }
            Some((self.seq_at)(if self.armed { self.since_arm } else { 0 }))
        }
        fn keys_up(&mut self, wait: u64) -> bool {
            self.calls.push(format!("keys_up {wait}"));
            self.keys_up
        }
        fn claim(&mut self, _app_id: &str) -> Receiver<Result<ClaimedText, ClaimError>> {
            self.calls.push("claim".into());
            self.armed = true;
            let (tx, rx) = mpsc::channel();
            self.claim_tx = Some(tx);
            if self.reply_at == ReplyAt::Claim {
                self.deliver();
            }
            rx
        }
        fn send_copy(&mut self, _t: &PasteTarget, select_all: bool) -> Result<(), PlatformError> {
            self.calls.push(format!("send_copy {select_all}"));
            self.copies += 1;
            let r = if self.copy_results.is_empty() {
                Ok(())
            } else {
                self.copy_results.remove(0)
            };
            if r.is_ok() && self.reply_at == ReplyAt::Copy(self.copies) {
                self.deliver();
            }
            r
        }
        fn copy_wait_ended(&mut self) {
            self.calls.push("copy_wait_ended".into());
        }
        fn is_sensitive(&mut self, _text: &str) -> bool {
            self.calls.push("is_sensitive".into());
            self.sensitive
        }
        fn translate(&mut self, text: Cleared) -> Receiver<Result<TransformResult, AiError>> {
            self.calls.push("translate".into());
            let mut inner = text.into_inner();
            self.sent_text.push(inner.0.chars().count());
            wipe_string(&mut inner.0);
            let (tx, rx) = mpsc::channel();
            if let Some(a) = self.answer.take() {
                let _ = tx.send(a);
            }
            self.translate_tx = Some(tx);
            rx
        }
        fn write_own_if(
            &mut self,
            expect_seq: u64,
            _text: Redacted<String>,
        ) -> Result<Option<u64>, ()> {
            self.calls.push(format!("write_own_if {expect_seq}"));
            self.write
        }
        fn paste(&mut self, _t: &PasteTarget) -> Result<(), PlatformError> {
            self.calls.push("paste".into());
            self.paste.clone()
        }
        fn restore_candidate(&mut self) -> Option<ClipId> {
            self.calls.push("restore_candidate".into());
            self.candidate
        }
        fn restore_if(&mut self, expect_seq: u64, _id: ClipId) -> Result<bool, ()> {
            self.calls.push(format!("restore_if {expect_seq}"));
            self.restore
        }
        fn overlay(&mut self, s: OverlayState) {
            self.calls.push(format!("overlay {s:?}"));
            self.overlays.push(s);
        }
        fn sleep(&mut self, ms: u64) {
            self.calls.push(format!("sleep {ms}"));
            if self.armed {
                self.since_arm += ms;
                if self.reply_after_ms.is_some_and(|t| self.since_arm >= t) {
                    self.deliver();
                }
            }
        }
        fn cancelled(&mut self) -> bool {
            let c = self.cancel_after.is_some_and(|l| self.has(l));
            if c && self.reply_with_cancel {
                self.deliver();
            }
            c
        }
    }

    /// Calls other than polling (`clipboard_seq`, `sleep`).
    fn steps(f: &Fake) -> Vec<String> {
        f.calls
            .iter()
            .filter(|c| *c != "clipboard_seq" && !c.starts_with("sleep"))
            .cloned()
            .collect()
    }

    #[test]
    fn happy_path_replaces_and_restores() {
        let mut f = Fake::new();
        assert_eq!(f.run(), Outcome::Replaced);
        assert_eq!(
            steps(&f),
            vec![
                "target",
                "blocked",
                "keys_up 1000",
                "restore_candidate",
                "claim",
                "send_copy false",
                "copy_wait_ended",
                "is_sensitive",
                "overlay Sending { chars: 17 }",
                "translate",
                "write_own_if 101",
                "target",
                "paste",
                "overlay Done",
                "restore_if 102",
            ]
        );
        assert_eq!(f.sent_text, vec![SOURCE.chars().count()]);
    }

    #[test]
    fn restore_skipped_when_clipboard_changed_after_own_write() {
        // The user copied something after our write: the actor's restore refuses, and the
        // flow does nothing more.
        let mut f = Fake::new();
        f.restore = Ok(false);
        assert_eq!(f.run(), Outcome::Replaced);
        assert_eq!(f.calls.last().map(String::as_str), Some("restore_if 102"));
        assert_eq!(f.count("restore_if"), 1);
        assert_eq!(f.count("paste"), 1);
        assert_eq!(f.count("write_own_if"), 1);
    }

    #[test]
    fn restore_not_found_is_silent() {
        // F-44 (c), Task 611: the restore candidate was deleted since the press (the actor's
        // `RestoreIf` answers `NotFound`, any error maps to `Err`). The paste already
        // happened: no failure overlay, the outcome stays Replaced, nothing else is tried.
        let mut f = Fake::new();
        f.restore = Err(());
        assert_eq!(f.run(), Outcome::Replaced);
        assert_eq!(f.calls.last().map(String::as_str), Some("restore_if 102"));
        assert_eq!(f.count("restore_if"), 1);
        assert_eq!(f.count("paste"), 1);
        assert_eq!(f.overlays.last(), Some(&OverlayState::Done));
        assert!(
            !f.overlays
                .iter()
                .any(|o| matches!(o, OverlayState::Failed(_))),
            "{:?}",
            f.overlays
        );
    }

    #[test]
    fn newer_user_copy_during_api_wait_is_not_overwritten() {
        let mut f = Fake::new();
        f.write = Ok(None);
        assert_eq!(f.run(), Outcome::Failed(FlowError::ClipboardChanged));
        assert!(!f.has("paste"));
        assert!(!f.has("restore_if"));
        assert_eq!(
            f.overlays.last(),
            Some(&OverlayState::Failed(FlowError::ClipboardChanged))
        );
    }

    fn assert_failed_and_restored(f: &Fake, outcome: Outcome) {
        assert!(matches!(outcome, Outcome::Failed(_)), "{outcome:?}");
        assert!(!f.has("paste"), "{outcome:?}");
        assert!(!f.has("write_own_if"), "{outcome:?}");
        assert_eq!(
            f.calls.iter().filter(|c| *c == "restore_if 101").count(),
            1,
            "{outcome:?}"
        );
        assert_eq!(
            f.overlays.last(),
            Some(&OverlayState::Failed(match outcome {
                Outcome::Failed(e) => e,
                _ => unreachable!(),
            }))
        );
    }

    #[test]
    fn failure_never_pastes_and_restores_previous_clipboard() {
        for e in [
            AiError::NoKey,
            AiError::TooLong,
            AiError::Empty,
            AiError::Network,
            AiError::Timeout,
            AiError::Auth,
            AiError::RateLimited,
            AiError::Overloaded,
            AiError::Http(500),
            AiError::Refused,
            AiError::Truncated,
            AiError::BadResponse,
        ] {
            let mut f = Fake::new();
            f.answer = Some(Err(e));
            let o = f.run();
            assert_eq!(o, Outcome::Failed(FlowError::Ai(e)));
            assert_failed_and_restored(&f, o);
        }
        let mut f = Fake::new();
        f.sensitive = true;
        let o = f.run();
        assert_eq!(o, Outcome::Failed(FlowError::Sensitive));
        assert_failed_and_restored(&f, o);
        for (text, want) in [
            ("  \n\t ", FlowError::Empty),
            (&"a".repeat(8001) as &str, FlowError::TooLong),
        ] {
            let mut f = Fake::with_text(text);
            let o = f.run();
            assert_eq!(o, Outcome::Failed(want));
            assert_failed_and_restored(&f, o);
        }
        // A write that fails restores too.
        let mut f = Fake::new();
        f.write = Err(());
        let o = f.run();
        assert_eq!(o, Outcome::Failed(FlowError::WriteFailed));
        assert!(!f.has("paste"));
        assert_eq!(f.count("restore_if 101"), 1);
        // A provider that never answers ends at the guard, as a timeout.
        let mut f = Fake::new();
        f.answer = None;
        let o = f.run();
        assert_eq!(o, Outcome::Failed(FlowError::Ai(AiError::Timeout)));
        assert_failed_and_restored(&f, o);
        // No candidate: nothing to restore, still no paste.
        let mut f = Fake::new();
        f.candidate = None;
        f.answer = Some(Err(AiError::Auth));
        assert_eq!(f.run(), Outcome::Failed(FlowError::Ai(AiError::Auth)));
        assert!(!f.has("paste") && !f.has("restore_if"));
    }

    #[test]
    fn empty_or_over_cap_text_is_not_sent() {
        for text in ["", "   ", "\n\t "] {
            let mut f = Fake::with_text(text);
            assert_eq!(f.run(), Outcome::Failed(FlowError::Empty));
            assert!(!f.has("translate") && !f.has("is_sensitive"));
            assert!(!f.has("overlay Sending"));
        }
        // The cap counts characters, not bytes: 8000 multi-byte characters are sent.
        let mut f = Fake::with_text(&"ệ".repeat(8000));
        assert_eq!(f.run(), Outcome::Replaced);
        assert_eq!(f.sent_text, vec![8000]);
        // 8001 are refused, never truncated.
        let mut f = Fake::with_text(&"ệ".repeat(8001));
        assert_eq!(f.run(), Outcome::Failed(FlowError::TooLong));
        assert!(!f.has("translate"));
        assert!(f.sent_text.is_empty());
    }

    #[test]
    fn keys_up_before_claim() {
        let mut f = Fake::new();
        f.run();
        let order: Vec<&str> = f
            .calls
            .iter()
            .map(String::as_str)
            .filter(|c| {
                c.starts_with("keys_up")
                    || *c == "restore_candidate"
                    || *c == "clipboard_seq"
                    || *c == "claim"
                    || c.starts_with("send_copy")
            })
            .take(5)
            .collect();
        assert_eq!(
            order,
            vec![
                "keys_up 1000",
                "restore_candidate",
                "clipboard_seq",
                "claim",
                "send_copy false"
            ]
        );
    }

    #[test]
    fn held_keys_block_before_arming() {
        let mut f = Fake::new();
        f.keys_up = false;
        assert_eq!(f.run(), Outcome::Blocked(Block::KeysHeld));
        assert!(!f.has("restore_candidate") && !f.has("claim") && !f.has("send_copy"));
        assert_eq!(f.overlays, vec![OverlayState::Blocked(Block::KeysHeld)]);
        // A copy refused because keys are held is reported the same way.
        let mut f = Fake::new();
        f.copy_results = vec![Err(PlatformError::Unsupported(
            darkclip_platform::REFUSAL_KEYS_HELD,
        ))];
        assert_eq!(f.run(), Outcome::Blocked(Block::KeysHeld));
        assert_eq!(f.count("send_copy"), 1);
        let mut f = Fake::new();
        f.copy_results = vec![Err(PlatformError::Unsupported(
            "target is not a normal window",
        ))];
        assert_eq!(f.run(), Outcome::Blocked(Block::NotNormalWindow));
        assert!(!f.has("translate") && !f.has("restore_if"));
    }

    #[test]
    fn blocked_checks_in_order_and_send_nothing() {
        let all = BlockFacts {
            consented: false,
            has_key: false,
            private: true,
            terminal: true,
            app_known: false,
            excluded: true,
        };
        // Clearing each fact in turn reveals the next block, in the documented order.
        let mut f = all;
        assert_eq!(first_block(&f), Some(Block::Off));
        f.consented = true;
        assert_eq!(first_block(&f), Some(Block::NoKey));
        f.has_key = true;
        assert_eq!(first_block(&f), Some(Block::Private));
        f.private = false;
        assert_eq!(first_block(&f), Some(Block::Terminal));
        f.terminal = false;
        assert_eq!(first_block(&f), Some(Block::UnknownApp));
        f.app_known = true;
        assert_eq!(first_block(&f), Some(Block::ExcludedApp));
        f.excluded = false;
        assert_eq!(first_block(&f), None);

        for b in [
            Block::Off,
            Block::NoKey,
            Block::Private,
            Block::Terminal,
            Block::UnknownApp,
            Block::ExcludedApp,
        ] {
            let mut f = Fake::new();
            f.block = Some(b);
            assert_eq!(f.run(), Outcome::Blocked(b));
            assert_eq!(
                f.calls,
                vec!["target", "blocked", &format!("overlay Blocked({b:?})")]
            );
        }
        // No foreground window of another process.
        let mut f = Fake::new();
        f.targets = vec![None];
        assert_eq!(f.run(), Outcome::Blocked(Block::NotNormalWindow));
        assert_eq!(f.calls, vec!["target", "overlay Blocked(NotNormalWindow)"]);
        // A target without an app id is never armed, even if the host missed it.
        let mut f = Fake::new();
        f.targets = vec![Some(PasteTarget {
            app_id: None,
            ..target(7, 70)
        })];
        assert_eq!(f.run(), Outcome::Blocked(Block::UnknownApp));
        assert!(!f.has("claim") && !f.has("send_copy") && !f.has("keys_up"));
    }

    #[test]
    fn select_all_only_when_seq_unchanged() {
        // The plain copy moved the clipboard: no select-all.
        let mut f = Fake::new();
        assert_eq!(f.run(), Outcome::Replaced);
        assert_eq!(f.count("send_copy true"), 0);
        // Nothing moved within the copy wait: one select-all retry, which is claimed.
        let mut f = Fake::new();
        f.seq_at = Box::new(|_| SEQ0);
        f.reply_at = ReplyAt::Copy(2);
        assert_eq!(f.run(), Outcome::Replaced);
        assert_eq!(f.count("send_copy false"), 1);
        assert_eq!(f.count("send_copy true"), 1);
        let first = f.calls.iter().position(|c| c == "send_copy false").unwrap();
        let retry = f.calls.iter().position(|c| c == "send_copy true").unwrap();
        // The retry waited for the whole copy wait.
        let slept: u64 = f.calls[first..retry]
            .iter()
            .filter_map(|c| c.strip_prefix("sleep ")?.parse::<u64>().ok())
            .sum();
        assert_eq!(slept, COPY_WAIT_MS);
    }

    #[test]
    fn no_select_all_when_seq_moves_just_before_retry() {
        // The change lands after the last poll, right before the retry: no Ctrl+A.
        let mut f = Fake::new();
        f.seq_at = Box::new(|t| if t >= COPY_WAIT_MS { CLAIMED_SEQ } else { SEQ0 });
        // The reply is held back and delivered on the first sleep after the copy wait.
        f.reply_at = ReplyAt::Never;
        let reply = f.reply.take();
        struct Late {
            inner: Fake,
            reply: Option<Result<ClaimedText, ClaimError>>,
        }
        impl FlowSys for Late {
            fn target(&mut self) -> Option<PasteTarget> {
                self.inner.target()
            }
            fn blocked(&mut self, t: &PasteTarget) -> Option<Block> {
                self.inner.blocked(t)
            }
            fn clipboard_seq(&mut self) -> Option<u64> {
                self.inner.clipboard_seq()
            }
            fn keys_up(&mut self, wait: u64) -> bool {
                self.inner.keys_up(wait)
            }
            fn claim(&mut self, a: &str) -> Receiver<Result<ClaimedText, ClaimError>> {
                self.inner.claim(a)
            }
            fn send_copy(&mut self, t: &PasteTarget, s: bool) -> Result<(), PlatformError> {
                self.inner.send_copy(t, s)
            }
            fn is_sensitive(&mut self, text: &str) -> bool {
                self.inner.is_sensitive(text)
            }
            fn translate(&mut self, text: Cleared) -> Receiver<Result<TransformResult, AiError>> {
                self.inner.translate(text)
            }
            fn write_own_if(&mut self, e: u64, t: Redacted<String>) -> Result<Option<u64>, ()> {
                self.inner.write_own_if(e, t)
            }
            fn paste(&mut self, t: &PasteTarget) -> Result<(), PlatformError> {
                self.inner.paste(t)
            }
            fn restore_candidate(&mut self) -> Option<ClipId> {
                self.inner.restore_candidate()
            }
            fn restore_if(&mut self, e: u64, id: ClipId) -> Result<bool, ()> {
                self.inner.restore_if(e, id)
            }
            fn overlay(&mut self, s: OverlayState) {
                self.inner.overlay(s);
            }
            fn sleep(&mut self, ms: u64) {
                self.inner.sleep(ms);
                if self.inner.since_arm > COPY_WAIT_MS {
                    if let (Some(tx), Some(r)) = (self.inner.claim_tx.as_ref(), self.reply.take()) {
                        let _ = tx.send(r);
                    }
                }
            }
            fn cancelled(&mut self) -> bool {
                self.inner.cancelled()
            }
        }
        let mut late = Late { inner: f, reply };
        assert_eq!(run(&mut late, &FlowConfig::default()), Outcome::Replaced);
        assert_eq!(late.inner.count("send_copy true"), 0);
        assert_eq!(late.inner.count("send_copy false"), 1);
        assert_eq!(late.inner.count("claim"), 1);
    }

    #[test]
    fn slow_copy_is_claimed_without_select_all() {
        // Task 610 (Q-08): a slow app (Chrome) moves the clipboard 900 ms after the plain
        // Ctrl+C. No Ctrl+A is sent (a partial selection is never widened) and the claim of
        // that first copy is used.
        let mut f = Fake::new();
        f.seq_at = Box::new(|t| if t >= 900 { CLAIMED_SEQ } else { SEQ0 });
        f.reply_at = ReplyAt::Never;
        f.reply_after_ms = Some(900);
        assert_eq!(f.run(), Outcome::Replaced);
        assert_eq!(f.count("send_copy true"), 0);
        assert_eq!(f.count("send_copy false"), 1);
        assert!(f.has(&format!("write_own_if {CLAIMED_SEQ}")));
        assert_eq!(f.sent_text, vec![SOURCE.chars().count()]);
        // The copy wait's end is reported once, for `copy_ms`.
        assert_eq!(f.count("copy_wait_ended"), 1);
    }

    #[test]
    fn select_all_only_after_1200_ms_unchanged() {
        assert_eq!(COPY_WAIT_MS, 1_200);
        // Unchanged for the whole 1200 ms: one select-all, after exactly that wait.
        let mut f = Fake::new();
        f.seq_at = Box::new(|_| SEQ0);
        f.reply_at = ReplyAt::Copy(2);
        assert_eq!(f.run(), Outcome::Replaced);
        assert_eq!(f.count("send_copy true"), 1);
        let first = f.calls.iter().position(|c| c == "send_copy false").unwrap();
        let retry = f.calls.iter().position(|c| c == "send_copy true").unwrap();
        let slept: u64 = f.calls[first..retry]
            .iter()
            .filter_map(|c| c.strip_prefix("sleep ")?.parse::<u64>().ok())
            .sum();
        assert_eq!(slept, 1_200);
        // Moved at 1190 ms: no select-all.
        let mut f = Fake::new();
        f.seq_at = Box::new(|t| if t >= 1_190 { CLAIMED_SEQ } else { SEQ0 });
        f.reply_at = ReplyAt::Never;
        f.reply_after_ms = Some(1_190);
        assert_eq!(f.run(), Outcome::Replaced);
        assert_eq!(f.count("send_copy true"), 0);
    }

    #[test]
    fn one_claim_for_both_attempts() {
        let mut f = Fake::new();
        f.seq_at = Box::new(|_| SEQ0);
        f.reply_at = ReplyAt::Copy(2);
        assert_eq!(f.run(), Outcome::Replaced);
        assert_eq!(f.count("claim"), 1);
        assert_eq!(f.count("send_copy"), 2);
    }

    #[test]
    fn claim_timeout_is_nothing_copied() {
        // No reply at all: the wait ends after CLAIM_WAIT_MS from arming.
        let mut f = Fake::new();
        f.reply_at = ReplyAt::Never;
        assert_eq!(f.run(), Outcome::Failed(FlowError::NothingCopied));
        assert_eq!(f.since_arm, CLAIM_WAIT_MS);
        assert_eq!(f.count("copy_wait_ended"), 1);
        assert!(!f.has("translate") && !f.has("restore_if") && !f.has("paste"));
        // The actor's Expired is the same outcome.
        let mut f = Fake::new();
        f.reply = Some(Err(ClaimError::Expired));
        assert_eq!(f.run(), Outcome::Failed(FlowError::NothingCopied));
        // Other claim errors are reported as such, with nothing restored.
        for e in [
            ClaimError::NoCloud,
            ClaimError::NotText,
            ClaimError::TooLong,
            ClaimError::Skipped(darkclip_core::policy::SkipReason::ExcludedApp),
        ] {
            let mut f = Fake::new();
            f.reply = Some(Err(e));
            assert_eq!(f.run(), Outcome::Failed(FlowError::Claim(e)));
            assert!(!f.has("translate") && !f.has("restore_if"));
        }
        // A text before any key was sent is wiped and not used (A1).
        let mut f = Fake::new();
        f.reply_at = ReplyAt::Claim;
        assert_eq!(f.run(), Outcome::Failed(FlowError::NothingCopied));
        assert!(!f.has("send_copy") && !f.has("translate") && !f.has("restore_if"));
        // An arm that failed at once sends no key.
        let mut f = Fake::new();
        f.reply = Some(Err(ClaimError::Busy));
        f.reply_at = ReplyAt::Claim;
        assert_eq!(f.run(), Outcome::Failed(FlowError::Claim(ClaimError::Busy)));
        assert!(!f.has("send_copy"));
    }

    #[test]
    fn cancel_at_each_wait_writes_nothing() {
        // During the copy wait, during the claim wait, during the request.
        for (after, reply_at) in [
            ("send_copy false", ReplyAt::Never),
            ("send_copy true", ReplyAt::Never),
            ("translate", ReplyAt::Copy(1)),
        ] {
            let mut f = Fake::new();
            f.seq_at = Box::new(|_| SEQ0);
            f.reply_at = reply_at;
            f.answer = None;
            f.cancel_after = Some(after);
            assert_eq!(f.run(), Outcome::Cancelled, "{after}");
            assert!(!f.has("write_own_if") && !f.has("paste"), "{after}");
            assert_eq!(f.overlays.last(), Some(&OverlayState::Cancelled));
        }
        // Esc during the copy wait stops before the select-all retry.
        let mut f = Fake::new();
        f.seq_at = Box::new(|_| SEQ0);
        f.reply_at = ReplyAt::Never;
        f.cancel_after = Some("send_copy false");
        f.run();
        assert_eq!(f.count("send_copy true"), 0);
    }

    #[test]
    fn a_reply_landing_with_the_cancel_is_wiped_and_not_used() {
        // The text arrives in the same tick as Esc: the flow is cancelled, sends nothing and
        // drains the channel (the drained text is wiped, see `drain_wipes_every_text`).
        let mut f = Fake::new();
        f.seq_at = Box::new(|_| SEQ0);
        f.reply_at = ReplyAt::Never;
        f.cancel_after = Some("send_copy false");
        f.reply_with_cancel = true;
        assert_eq!(f.run(), Outcome::Cancelled);
        assert!(f.reply.is_none(), "the reply was delivered");
        assert!(!f.has("translate") && !f.has("write_own_if") && !f.has("paste"));
        assert!(!f.has("restore_if"));
        // The same for the translation.
        let mut f = Fake::new();
        f.answer = None;
        f.cancel_after = Some("translate");
        let mut sent = false;
        let o = run(
            &mut LateAnswer {
                inner: &mut f,
                sent: &mut sent,
            },
            &FlowConfig::default(),
        );
        assert_eq!(o, Outcome::Cancelled);
        assert!(sent);
        assert!(!f.has("write_own_if") && !f.has("paste"));
    }

    /// Sends the translation inside the `cancelled()` call that answers true.
    struct LateAnswer<'a> {
        inner: &'a mut Fake,
        sent: &'a mut bool,
    }

    impl FlowSys for LateAnswer<'_> {
        fn target(&mut self) -> Option<PasteTarget> {
            self.inner.target()
        }
        fn blocked(&mut self, t: &PasteTarget) -> Option<Block> {
            self.inner.blocked(t)
        }
        fn clipboard_seq(&mut self) -> Option<u64> {
            self.inner.clipboard_seq()
        }
        fn keys_up(&mut self, wait: u64) -> bool {
            self.inner.keys_up(wait)
        }
        fn claim(&mut self, a: &str) -> Receiver<Result<ClaimedText, ClaimError>> {
            self.inner.claim(a)
        }
        fn send_copy(&mut self, t: &PasteTarget, s: bool) -> Result<(), PlatformError> {
            self.inner.send_copy(t, s)
        }
        fn is_sensitive(&mut self, text: &str) -> bool {
            self.inner.is_sensitive(text)
        }
        fn translate(&mut self, text: Cleared) -> Receiver<Result<TransformResult, AiError>> {
            self.inner.translate(text)
        }
        fn write_own_if(&mut self, e: u64, t: Redacted<String>) -> Result<Option<u64>, ()> {
            self.inner.write_own_if(e, t)
        }
        fn paste(&mut self, t: &PasteTarget) -> Result<(), PlatformError> {
            self.inner.paste(t)
        }
        fn restore_candidate(&mut self) -> Option<ClipId> {
            self.inner.restore_candidate()
        }
        fn restore_if(&mut self, e: u64, id: ClipId) -> Result<bool, ()> {
            self.inner.restore_if(e, id)
        }
        fn overlay(&mut self, s: OverlayState) {
            self.inner.overlay(s);
        }
        fn sleep(&mut self, ms: u64) {
            self.inner.sleep(ms);
        }
        fn cancelled(&mut self) -> bool {
            let c = self.inner.cancelled();
            if c && !*self.sent {
                if let Some(tx) = self.inner.translate_tx.as_ref() {
                    let _ = tx.send(Ok(TransformResult {
                        text: Redacted(TRANSLATION.into()),
                    }));
                    *self.sent = true;
                }
            }
            c
        }
    }

    #[test]
    fn drain_wipes_every_text() {
        let (tx, rx) = mpsc::channel::<Result<ClaimedText, ClaimError>>();
        tx.send(Ok(ClaimedText {
            text: Redacted(SOURCE.into()),
            seq: 1,
        }))
        .unwrap();
        tx.send(Err(ClaimError::Busy)).unwrap();
        assert_eq!(drain(&rx), 1);
        assert!(rx.try_recv().is_err());
        let mut c = ClaimedText {
            text: Redacted(SOURCE.into()),
            seq: 1,
        };
        c.wipe();
        assert!(c.text.0.is_empty());
        let mut r = TransformResult {
            text: Redacted(TRANSLATION.into()),
        };
        r.wipe();
        assert!(r.text.0.is_empty());
    }

    #[test]
    fn unknown_seq_never_selects_all() {
        let mut f = Fake::new();
        f.seq_unknown = true;
        f.reply_at = ReplyAt::Never;
        assert_eq!(f.run(), Outcome::Failed(FlowError::NothingCopied));
        assert_eq!(f.count("send_copy true"), 0);
        // The plain copy still works when the seq is unknown.
        let mut f = Fake::new();
        f.seq_unknown = true;
        assert_eq!(f.run(), Outcome::Replaced);
        assert_eq!(f.count("send_copy true"), 0);
    }

    #[test]
    fn focus_moved_leaves_on_clipboard_without_paste() {
        for moved in [None, Some(target(8, 70)), Some(target(7, 71))] {
            let mut f = Fake::new();
            f.targets = vec![Some(target(7, 70)), moved];
            assert_eq!(f.run(), Outcome::LeftOnClipboard);
            assert!(!f.has("paste") && !f.has("restore_if"));
            assert_eq!(f.overlays.last(), Some(&OverlayState::LeftOnClipboard));
        }
    }

    #[test]
    fn paste_refused_leaves_on_clipboard() {
        let mut f = Fake::new();
        f.paste = Err(PlatformError::Unsupported("target did not take the focus"));
        assert_eq!(f.run(), Outcome::LeftOnClipboard);
        assert!(!f.has("restore_if"));
        assert!(!f.overlays.contains(&OverlayState::Done));
        assert_eq!(f.overlays.last(), Some(&OverlayState::LeftOnClipboard));
    }

    #[test]
    fn done_shown_at_paste_time_before_restore() {
        let mut f = Fake::new();
        assert_eq!(f.run(), Outcome::Replaced);
        let paste = f.calls.iter().position(|c| c == "paste").unwrap();
        assert_eq!(f.calls[paste + 1], "overlay Done");
        assert_eq!(f.calls[paste + 2], format!("sleep {RESTORE_DELAY_MS}"));
        assert_eq!(f.calls[paste + 3], "restore_if 102");
        // Done is the last overlay state, shown once.
        assert_eq!(f.overlays.last(), Some(&OverlayState::Done));
        assert_eq!(
            f.overlays
                .iter()
                .filter(|s| **s == OverlayState::Done)
                .count(),
            1
        );
        // No candidate: Done, and no restore.
        let mut f = Fake::new();
        f.candidate = None;
        assert_eq!(f.run(), Outcome::Replaced);
        assert!(!f.has("restore_if"));
    }

    #[test]
    fn debug_is_content_free() {
        let cleared = Cleared(Redacted(SOURCE.to_string()));
        assert_eq!(format!("{cleared:?}"), "<redacted>");
        let claimed = ClaimedText {
            text: Redacted(SOURCE.into()),
            seq: 5,
        };
        assert!(!format!("{claimed:?}").contains("chào"));
        let mut f = Fake::new();
        let o = f.run();
        for line in f.calls.iter().chain(std::iter::once(&format!("{o:?}"))) {
            assert!(
                !line.contains("chào") && !line.contains(TRANSLATION),
                "{line}"
            );
        }
        for s in &f.overlays {
            assert!(!format!("{s:?}").contains("Hello"));
        }
    }

    #[test]
    fn cleared_is_wiped_on_drop_and_moved_out_whole() {
        let c = Cleared(Redacted(SOURCE.to_string()));
        assert_eq!(c.into_inner().0, SOURCE);
    }
}
