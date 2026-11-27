//! The capture pipeline for one change, in privacy order: snapshot → `decide_pre_read` →
//! read → classify → kind check → sensitive scan → decode and hash → store.

use super::actor::{size_caps, Actor};
use super::claim::ClaimedChange;
use super::pending::{Carried, ExcludedRecent, Pending};
use super::wipe::{wipe_bytes, wipe_reps, wipe_string};
use super::SaveOutcome;
use crate::events::{ApiError, AppEvent, UNAVAILABLE_STORAGE_WRITE};
use crate::image;
use darkclip_core::markers::PrivacyMarker;
use darkclip_core::policy::{
    decide_pre_read, decide_translate_read, AvailableFormat, Decision, PreReadInput, SizeCaps,
    SkipReason,
};
use darkclip_core::sensitive::{masked_preview, scan, MaskHint, ScanResult, MAX_SCAN_BYTES};
use darkclip_core::{
    canonical_hash, classify, ClipType, ClipboardSource, Mime, PrivacyPolicy, RawRepresentation,
    SensitiveCategory, SensitiveHandling, Sensitivity, SensitivityLevel, Timestamp,
};
use darkclip_platform::ClipboardMeta;
use darkclip_storage::{NewClip, StorageError, UpsertOutcome};
use std::sync::PoisonError;

/// The policy decision for the steps that need no clipboard data (settings, self-write),
/// taken with empty markers and formats so the clipboard is not touched (D-021).
/// `Some(skip)` drops the change here; `None` means the clipboard may be inspected.
pub(super) fn gate_without_formats(
    policy: &PrivacyPolicy,
    now: Timestamp,
    seq: u64,
    last_self_write_seq: Option<u64>,
    caps: SizeCaps,
) -> Option<Decision> {
    let d = decide_pre_read(&PreReadInput {
        seq,
        last_self_write_seq,
        mode: &policy.mode,
        now,
        capture_enabled: policy.capture_enabled,
        markers: &[],
        source: None,
        excluded: &policy.excluded_apps,
        available: &[],
        enabled_kinds: &policy.capture_kinds,
        caps,
    });
    match d {
        Decision::Skip(
            SkipReason::Disabled | SkipReason::Paused | SkipReason::Private | SkipReason::SelfWrite,
        ) => Some(d),
        _ => None,
    }
}

/// `decide_pre_read` with the primary source, then the excluded-app check against every
/// other source seen for the same change: if any of them is excluded, the change is skipped.
/// Steps before the app check do not depend on the source, so precedence is unchanged.
#[cfg(test)]
pub(super) fn decide_all_sources(
    input: &PreReadInput<'_>,
    others: &[&ClipboardSource],
) -> Decision {
    decide_all_sources_with(decide_pre_read, input, others)
}

/// `decide_all_sources` with the given decision (`decide_pre_read`, or
/// `decide_translate_read` for a claimed change).
fn decide_all_sources_with(
    decide: fn(&PreReadInput<'_>) -> Decision,
    input: &PreReadInput<'_>,
    others: &[&ClipboardSource],
) -> Decision {
    let d = decide(input);
    for other in others {
        let alt = decide(&PreReadInput {
            source: Some(*other),
            ..*input
        });
        if alt == Decision::Skip(SkipReason::ExcludedApp) {
            return alt;
        }
    }
    d
}

/// The sources of a change (primary and others) that the excluded-app step matches.
fn excluded_sources(
    decide: fn(&PreReadInput<'_>) -> Decision,
    input: &PreReadInput<'_>,
    others: &[&ClipboardSource],
) -> Vec<ClipboardSource> {
    input
        .source
        .into_iter()
        .chain(others.iter().copied())
        .filter(|s| {
            decide(&PreReadInput {
                source: Some(*s),
                ..*input
            }) == Decision::Skip(SkipReason::ExcludedApp)
        })
        .cloned()
        .collect()
}

/// Sensitive scan of the clip's text: the plain text, or, when that is empty, the HTML/RTF
/// bodies decoded as lossy UTF-8 (at most `MAX_SCAN_BYTES` each, like the plain text).
fn scan_clip(
    plain: Option<&str>,
    reps: &[RawRepresentation],
    enabled: &[SensitiveCategory],
) -> ScanResult {
    if let Some(t) = plain.filter(|t| !t.trim().is_empty()) {
        return scan(t, enabled);
    }
    let mut out = ScanResult::default();
    for r in reps
        .iter()
        .filter(|r| matches!(r.mime, Mime::TextHtml | Mime::TextRtf))
    {
        let n = r.bytes.len().min(MAX_SCAN_BYTES);
        let mut text = String::from_utf8_lossy(r.bytes.get(..n).unwrap_or_default()).into_owned();
        let found = scan(&text, enabled);
        wipe_string(&mut text);
        for c in found.categories {
            if !out.categories.contains(&c) {
                out.categories.push(c);
            }
        }
        if out.mask_hint.is_none() {
            out.mask_hint = found.mask_hint;
        }
    }
    out
}

/// Hash and optional thumbnail of a clip about to be stored.
struct Hashed {
    hash: [u8; 32],
    thumbnail: Option<Vec<u8>>,
    /// Dimensions that were missing on the decoded rep and have been filled in.
    filled_dims: Option<(u32, u32)>,
}

/// Canonical hash; for an image with a decodable PNG, over its pixels (so the same picture in
/// two encodings dedupes), plus a thumbnail (unless `want_thumbnail` is false) and missing
/// dimensions filled in on the decoded rep. A decode failure keeps the fallback hash over the
/// encoded bytes. The RGBA buffer is wiped and dropped here.
fn decode_and_hash(reps: &mut [RawRepresentation], kind: ClipType, want_thumbnail: bool) -> Hashed {
    if kind == ClipType::Image {
        let idx = reps
            .iter()
            .position(|r| r.mime == Mime::ImagePng && !r.truncated);
        let decoded = idx.and_then(|i| {
            image::decode_png(&reps[i].bytes, image::MAX_PIXELS, image::MAX_RGBA_BYTES).ok()
        });
        if let (Some(i), Some(mut img)) = (idx, decoded) {
            let hash = canonical_hash(kind, reps, Some((&img.rgba, img.width, img.height)));
            let thumb = if want_thumbnail {
                image::thumbnail(&img, image::THUMB_MAX_SIDE)
            } else {
                Vec::new()
            };
            wipe_bytes(&mut img.rgba);
            let mut filled_dims = None;
            if reps[i].width.is_none() || reps[i].height.is_none() {
                reps[i].width = Some(img.width);
                reps[i].height = Some(img.height);
                filled_dims = Some((img.width, img.height));
            }
            return Hashed {
                hash,
                thumbnail: (!thumb.is_empty()).then_some(thumb),
                filled_dims,
            };
        }
    }
    Hashed {
        hash: canonical_hash(kind, reps, None),
        thumbnail: None,
        filled_dims: None,
    }
}

impl Actor {
    pub(super) fn process(&mut self, p: Pending) {
        if self.last_processed_seq == Some(p.seq) {
            return;
        }
        // `held`: a gate reason a live claim let through (Task 603). The change is then
        // either claimed or dropped with that reason, exactly as without a claim.
        let mut held = None;
        if let Some(reason) = self.gate(p.seq) {
            if !self.claim_lets_through(reason, p.seen_at) {
                if self.claim.is_some() {
                    self.claim_gated();
                }
                self.drop_gated(p.seq, reason);
                return;
            }
            held = Some(reason);
        }
        let meta: ClipboardMeta = match self.with_retries(|a| a.snapshot_meta()) {
            Ok(m) => m,
            Err(e) => {
                if let Some(reason) = held {
                    self.drop_gated(p.seq, reason);
                    return;
                }
                self.last_processed_seq = Some(p.seq);
                self.capture_failed(&e);
                return;
            }
        };
        // The snapshot's sequence number is the truth (rule 2).
        let seq = meta.seq;
        if self.last_processed_seq == Some(seq) {
            return;
        }
        if let Some(reason) = self.gate(seq) {
            if !self.claim_lets_through(reason, p.seen_at) {
                if self.claim.is_some() {
                    self.claim_gated();
                }
                self.drop_gated(seq, reason);
                return;
            }
            held = Some(reason);
        }
        let (carried, carried_others) = match self.carried.take() {
            Some(c) if c.seq == seq => (c.primary, c.others),
            _ => (None, Vec::new()),
        };
        // The snapshot may be newer than the signalled change (D-043): then the pending
        // change's sources (and its superseded chain) belong to an older change. They are
        // checked for exclusion below, never stored as this change's source.
        let newer = seq != p.seq;
        // Every source seen for this change; the first one seen is the primary.
        let own: &[ClipboardSource] = if newer { &[] } else { &p.sources };
        let mut seen: Vec<ClipboardSource> = Vec::new();
        for s in carried.iter().chain(own).chain(meta.source.as_ref()) {
            if !seen.contains(s) {
                seen.push(s.clone());
            }
        }
        // F-14, an ownerless change (e.g. an OLE flush): rule 2 may name the stored source;
        // rule 1 checks every source the burst superseded, never as the primary (only adds
        // skips).
        let ownerless = seen.is_empty();
        let mut seen = seen.into_iter();
        let source = if ownerless {
            self.heir_source(&p, &meta)
        } else {
            seen.next()
        };
        let mut others: Vec<ClipboardSource> = seen.collect();
        if ownerless {
            for s in p
                .superseded
                .iter()
                .chain(self.recent_excluded(seq, p.signal_at))
            {
                if source.as_ref() != Some(s) && !others.contains(s) {
                    others.push(s.clone());
                }
            }
        }
        // A newer snapshot: the older change's own sources and chain are checked only.
        if newer {
            for s in p.sources.iter().chain(&p.superseded) {
                if source.as_ref() != Some(s) && !others.contains(s) {
                    others.push(s.clone());
                }
            }
        }
        // A retry of a carried seq checks every source the earlier look checked.
        for s in carried_others {
            if source.as_ref() != Some(&s) && !others.contains(&s) {
                others.push(s);
            }
        }
        let others: Vec<&ClipboardSource> = others.iter().collect();
        // Task 603: decided from the snapshot's metadata only, before anything is read. A
        // snapshot newer than the signalled change counts as first seen now.
        let claimed = self.claim.is_some() && {
            let seen_at = if newer {
                std::time::Instant::now()
            } else {
                p.seen_at
            };
            self.claims(source.as_ref(), seen_at)
        };
        if !claimed {
            if let Some(reason) = held {
                self.drop_gated(seq, reason);
                return;
            }
        }
        let available: Vec<AvailableFormat> = meta
            .formats
            .iter()
            .filter_map(|f| {
                f.mime.map(|mime| AvailableFormat {
                    mime,
                    platform_format: f.name.clone(),
                    size_hint: f.size_hint,
                })
            })
            .collect();
        let now = self.clock.now();
        let input = PreReadInput {
            seq,
            last_self_write_seq: self.last_self_write_seq,
            mode: &self.policy.mode,
            now,
            capture_enabled: self.policy.capture_enabled,
            markers: &meta.markers,
            source: source.as_ref(),
            excluded: &self.policy.excluded_apps,
            available: &available,
            enabled_kinds: &self.policy.capture_kinds,
            caps: size_caps(&self.retention),
        };
        let decide = if claimed {
            decide_translate_read
        } else {
            decide_pre_read
        };
        let decision = decide_all_sources_with(decide, &input, &others);
        let excluded_now = if decision == Decision::Skip(SkipReason::ExcludedApp) {
            excluded_sources(decide, &input, &others)
        } else {
            Vec::new()
        };
        if claimed {
            // Handed to the requester, never stored.
            self.finish_claim(ClaimedChange {
                seq,
                decision,
                excluded_now,
                markers: &meta.markers,
                signal_at: p.signal_at,
            });
            return;
        }
        let plan = match decision {
            Decision::Read(plan) => plan,
            Decision::Skip(SkipReason::NothingReadable) => {
                // Delayed rendering: the same seq may get real formats later. Not marked
                // processed; the primary source, and every other source checked, are kept
                // for that later burst.
                self.carried = Some(Carried {
                    seq,
                    primary: source,
                    others: others.iter().map(|s| (*s).clone()).collect(),
                });
                self.skip(SkipReason::NothingReadable);
                return;
            }
            Decision::Skip(reason) => {
                if reason == SkipReason::ExcludedApp {
                    self.excluded_recent = Some(ExcludedRecent {
                        seq,
                        sources: excluded_now,
                        signal_at: p.signal_at,
                    });
                }
                self.last_processed_seq = Some(seq);
                self.skip(reason);
                return;
            }
        };
        // Policy said Read: content may be read now (GC-03).
        let read = self.with_retries(|a| a.read(&plan));
        self.last_processed_seq = Some(seq);
        let mut reps = match read {
            Ok(r) => r,
            Err(e) => {
                self.capture_failed(&e);
                return;
            }
        };
        // The markers were checked for `seq`. If the clipboard changed since the snapshot,
        // the bytes may belong to a change nobody checked: discard them. That change brings
        // its own signal.
        if self.access.sequence().ok() != Some(seq) {
            wipe_reps(&mut reps);
            self.counters
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .count_changed_during_read();
            return;
        }
        if reps.is_empty() {
            self.skip(SkipReason::NothingReadable);
            return;
        }
        // NoCloud (`CanUploadToCloudClipboard` = 0): stored, but never exported
        // (privacy-design §3; Task 405).
        let no_export = meta.markers.contains(&PrivacyMarker::NoCloud);
        self.store(seq, reps, source, now, no_export);
    }

    fn store(
        &mut self,
        seq: u64,
        reps: Vec<RawRepresentation>,
        source: Option<ClipboardSource>,
        now: Timestamp,
        no_export: bool,
    ) {
        match self.prepare(reps, source, now) {
            Ok(mut new) => {
                new.no_export = no_export;
                if let Some(UpsertOutcome::Inserted(id) | UpsertOutcome::Bumped(id)) =
                    self.insert_new(new)
                {
                    self.last_stored = Some((seq, id));
                }
            }
            Err(reason) => self.skip(reason),
        }
    }

    /// Classify, kind check, sensitive scan and handling, decode and hash: everything between
    /// the read and the insert. Shared by capture and by "save edited copy" (Task 404), so an
    /// edited text meets exactly the rules a copied one does. `Err` means nothing is stored
    /// (the data is wiped); the caller decides what to count or report.
    pub(super) fn prepare(
        &mut self,
        mut reps: Vec<RawRepresentation>,
        source: Option<ClipboardSource>,
        now: Timestamp,
    ) -> Result<NewClip, SkipReason> {
        let mut classified = classify(&reps);
        if !self.policy.capture_kinds.contains(&classified.kind) {
            self.discard(
                &mut reps,
                &mut classified.preview,
                &mut classified.plain_text,
            );
            return Err(SkipReason::KindDisabled);
        }
        let found = scan_clip(
            classified.plain_text.as_deref(),
            &reps,
            &self.policy.sensitive_categories_enabled,
        );
        let (sensitivity, expires_at) = if found.categories.is_empty() {
            (Sensitivity::default(), None)
        } else {
            match self.policy.sensitive_handling {
                SensitiveHandling::Drop => {
                    self.discard(
                        &mut reps,
                        &mut classified.preview,
                        &mut classified.plain_text,
                    );
                    return Err(SkipReason::Sensitive);
                }
                SensitiveHandling::StoreMasked => {
                    let hint = found.mask_hint.clone().unwrap_or_else(|| MaskHint {
                        visible_prefix: String::new(),
                        category: found
                            .categories
                            .first()
                            .copied()
                            .unwrap_or(SensitiveCategory::Password),
                    });
                    let mut old = std::mem::replace(
                        &mut classified.preview,
                        masked_preview(&hint, self.cfg.lang),
                    );
                    wipe_string(&mut old);
                    let ttl_ms =
                        i64::from(self.retention.sensitive_ttl_seconds).saturating_mul(1000);
                    (
                        Sensitivity {
                            level: SensitivityLevel::Masked,
                            categories: found.categories.clone(),
                        },
                        Some(Timestamp(now.0.saturating_add(ttl_ms))),
                    )
                }
                SensitiveHandling::Store => (
                    Sensitivity {
                        level: SensitivityLevel::None,
                        categories: found.categories.clone(),
                    },
                    None,
                ),
            }
        };
        // Only now, after the policy decision and sensitive handling, are pixels decoded.
        let masked = sensitivity.level == SensitivityLevel::Masked;
        let hashed = decode_and_hash(&mut reps, classified.kind, !masked);
        if let (Some((w, h)), false) = (hashed.filled_dims, masked) {
            classified.preview = darkclip_core::image_preview(w, h);
        }
        let hash = hashed.hash;
        let thumbnail_png = hashed.thumbnail;
        Ok(NewClip {
            classified,
            hash,
            reps,
            source,
            sensitivity,
            expires_at,
            thumbnail_png,
            no_export: false,
        })
    }

    /// Inserts a prepared clip (retention on `StorageFull`), schedules the expiry, emits the
    /// event. Returns what storage did, or `None` when the write failed.
    pub(super) fn insert_new(&mut self, new: NewClip) -> Option<UpsertOutcome> {
        let expires_at = new.expires_at;
        let outcome = self.insert_with_retry(new);
        if outcome.is_ok() {
            if let Some(t) = expires_at {
                self.next_expiry = Some(self.next_expiry.map_or(t, |n| n.min(t)));
            }
        }
        match outcome {
            Ok(UpsertOutcome::Inserted(id)) => {
                // Limits first, so a listener never sees the store over its limits.
                self.retention_if_over_limit();
                self.emit(AppEvent::ClipAdded(id));
                Some(UpsertOutcome::Inserted(id))
            }
            Ok(UpsertOutcome::Bumped(id)) => {
                self.emit(AppEvent::ClipBumped(id));
                Some(UpsertOutcome::Bumped(id))
            }
            Err(_) => {
                self.emit(AppEvent::Unavailable(UNAVAILABLE_STORAGE_WRITE));
                None
            }
        }
    }

    /// F-02: on `StorageFull` run retention once and retry the insert once with the same
    /// value. If the retry fails with any error the failure is counted and returned (event
    /// from the caller); the actor stays alive. The clip data is wiped afterwards.
    fn insert_with_retry(&mut self, mut new: NewClip) -> Result<UpsertOutcome, StorageError> {
        let mut result = self.try_upsert(&new);
        if result == Err(StorageError::StorageFull) {
            self.run_retention();
            result = self.try_upsert(&new);
            if result.is_err() {
                self.counters
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .count_storage_full();
            }
        }
        wipe_reps(&mut new.reps);
        wipe_string(&mut new.classified.preview);
        if let Some(t) = new.classified.plain_text.as_mut() {
            wipe_string(t);
        }
        if let Some(t) = new.thumbnail_png.as_mut() {
            wipe_bytes(t);
        }
        result
    }

    fn try_upsert(&mut self, new: &NewClip) -> Result<UpsertOutcome, StorageError> {
        #[cfg(test)]
        {
            let injected = self
                .hooks
                .inject
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .pop_front();
            if let Some(e) = injected {
                return Err(e);
            }
        }
        self.storage.upsert_clip(new)
    }

    fn discard(
        &self,
        reps: &mut [RawRepresentation],
        preview: &mut String,
        plain: &mut Option<String>,
    ) {
        wipe_reps(reps);
        wipe_string(preview);
        if let Some(t) = plain.as_mut() {
            wipe_string(t);
        }
    }
}

impl Actor {
    /// "Save as new clip" (Task 404). The text is plain text with no source app. Refusals by
    /// capture's own rules are an `Ok(Dropped(reason))` (nothing stored, nothing counted as a
    /// capture skip); a bad request is an `Err`.
    pub(super) fn save_text(&mut self, mut text: String) -> Result<SaveOutcome, ApiError> {
        if text.trim().is_empty() {
            wipe_string(&mut text);
            return Err(ApiError::InvalidArgument("nothing to save"));
        }
        if text.len() as u64 > size_caps(&self.retention).text_max_bytes {
            wipe_string(&mut text);
            return Err(ApiError::InvalidArgument("too large"));
        }
        // Private mode saves nothing at all. Paused and "capture off" only stop watching the
        // clipboard; this is an explicit user action that reads no clipboard (orchestrator decision, Task 404 review).
        if self.policy.mode == darkclip_core::CaptureMode::Private {
            wipe_string(&mut text);
            return Ok(SaveOutcome::Dropped(SkipReason::Private));
        }
        let reps = vec![RawRepresentation {
            mime: Mime::TextPlain,
            platform_format: "text/plain".to_string(),
            bytes: text.into_bytes(),
            width: None,
            height: None,
            truncated: false,
        }];
        let now = self.clock.now();
        match self.prepare(reps, None, now) {
            Err(reason) => Ok(SaveOutcome::Dropped(reason)),
            Ok(mut new) => {
                // Content already in history: touch nothing (an upsert would bump the
                // original and could rewrite its sensitivity or expiry).
                match self.storage.clip_id_by_hash(&new.hash) {
                    Ok(Some(id)) => {
                        wipe_reps(&mut new.reps);
                        wipe_string(&mut new.classified.preview);
                        if let Some(t) = new.classified.plain_text.as_mut() {
                            wipe_string(t);
                        }
                        return Ok(SaveOutcome::Exists(id));
                    }
                    Ok(None) => {}
                    Err(_) => {
                        wipe_reps(&mut new.reps);
                        return Err(ApiError::Storage);
                    }
                }
                match self.insert_new(new) {
                    Some(UpsertOutcome::Inserted(id) | UpsertOutcome::Bumped(id)) => {
                        Ok(SaveOutcome::Saved(id))
                    }
                    None => Err(ApiError::Storage),
                }
            }
        }
    }
}
