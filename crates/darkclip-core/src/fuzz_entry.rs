//! Fuzz entry points (Task 406). Built only for tests and with the `fuzzing` feature (the
//! `fuzz/` cargo-fuzz targets enable it); never part of a normal build.
//!
//! Each function takes arbitrary bytes, drives one parser and checks its invariants with
//! `assert!`. The assertion messages never include the input (GC-06). The `#[ignore]` stable
//! harness in `harness` calls the same functions with seeded, structure-aware inputs.

use crate::model::{SensitiveCategory, Tag, PREVIEW_MAX_CHARS, TAG_NAME_MAX_CHARS};
use crate::sensitive::{masked_preview, scan, MASK_PREFIX_CHARS, MAX_SCAN_BYTES};
use crate::transform::{Transform, TransformError, MAX_INPUT_BYTES};
use crate::Lang;

const CATEGORIES: [SensitiveCategory; 7] = [
    SensitiveCategory::ApiKey,
    SensitiveCategory::Token,
    SensitiveCategory::PrivateKey,
    SensitiveCategory::Password,
    SensitiveCategory::PaymentCard,
    SensitiveCategory::Email,
    SensitiveCategory::Phone,
];

/// Splits off the first byte as a selector; the rest becomes (lossy) text.
fn split(data: &[u8]) -> (u8, String) {
    match data.split_first() {
        Some((&sel, rest)) => (sel, String::from_utf8_lossy(rest).into_owned()),
        None => (0, String::new()),
    }
}

/// Sensitive scanners: the first byte selects the enabled categories (bit per category;
/// 0 means all).
pub fn sensitive(data: &[u8]) {
    let (sel, text) = split(data);
    let enabled: Vec<SensitiveCategory> = if sel & 0x7F == 0 {
        CATEGORIES.to_vec()
    } else {
        CATEGORIES
            .iter()
            .enumerate()
            .filter(|(i, _)| sel & (1 << i) != 0)
            .map(|(_, c)| *c)
            .collect()
    };
    let r = scan(&text, &enabled);
    // Found categories are enabled ones, in declaration order, without duplicates.
    assert!(
        r.categories.iter().all(|c| enabled.contains(c)),
        "category not enabled"
    );
    let order: Vec<usize> = r
        .categories
        .iter()
        .filter_map(|c| CATEGORIES.iter().position(|k| k == c))
        .collect();
    assert!(order.windows(2).all(|w| w[0] < w[1]), "category order");
    assert_eq!(
        r.mask_hint.is_some(),
        !r.categories.is_empty(),
        "hint iff found"
    );
    if let Some(h) = &r.mask_hint {
        assert!(r.categories.contains(&h.category), "hint category found");
        assert!(
            h.visible_prefix.chars().count() <= MASK_PREFIX_CHARS,
            "prefix length"
        );
        if h.category == SensitiveCategory::Email {
            assert!(h.visible_prefix.is_empty(), "email shows no prefix");
        }
        assert!(
            text.contains(h.visible_prefix.as_str()),
            "prefix comes from the text"
        );
        for lang in [Lang::En, Lang::Vi] {
            let p = masked_preview(h, lang);
            assert!(
                !p.chars().any(|c| c.is_control()),
                "control char in preview"
            );
        }
    }
    // Text past the scan window never changes the result.
    if text.len() > MAX_SCAN_BYTES {
        let mut end = MAX_SCAN_BYTES;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        assert_eq!(scan(&text[..end], &enabled), r, "scan window");
    }
}

fn check_len(r: &Result<String, TransformError>) {
    if let Ok(s) = r {
        assert!(s.len() <= MAX_INPUT_BYTES, "transform output over cap");
    }
}

/// Every transform on the same input, plus round trips. The first byte is ignored (kept so
/// the corpus format matches the other text targets).
pub fn transforms(data: &[u8]) {
    let (_, text) = split(data);
    let mut out = Vec::with_capacity(Transform::ALL.len());
    for t in Transform::ALL {
        let r = t.apply(&text);
        check_len(&r);
        out.push((t, r));
    }
    let get = |t: Transform| out.iter().find(|(k, _)| *k == t).map(|(_, r)| r.clone());
    if let Some(Ok(enc)) = get(Transform::UrlEncode) {
        assert_eq!(
            Transform::UrlDecode.apply(&enc).as_deref(),
            Ok(text.as_str()),
            "url round trip"
        );
    }
    if let Some(Ok(enc)) = get(Transform::Base64Encode) {
        assert_eq!(
            Transform::Base64Decode.apply(&enc).as_deref(),
            Ok(text.as_str()),
            "base64 round trip"
        );
    }
    if let Some(Ok(lf)) = get(Transform::LineEndingsLf) {
        assert!(!lf.contains('\r'), "LF output has CR");
    }
    if let (Some(Ok(pretty)), Some(Ok(min))) =
        (get(Transform::JsonPretty), get(Transform::JsonMinify))
    {
        // Both succeed only on valid JSON: minifying the pretty form gives the minified one.
        assert_eq!(
            Transform::JsonMinify.apply(&pretty),
            Ok(min.clone()),
            "json round trip"
        );
        assert_eq!(
            Transform::JsonMinify.apply(&min),
            Ok(min),
            "minify idempotent"
        );
    }
    if let Some(Ok(t)) = get(Transform::Trim) {
        assert_eq!(Transform::Trim.apply(&t), Ok(t.clone()), "trim idempotent");
    }
    if text.len() <= MAX_INPUT_BYTES {
        assert!(
            get(Transform::HtmlToText).is_some_and(|r| r.is_ok()),
            "html never fails"
        );
    }
}

/// Tag-name normalisation and the preview builder.
pub fn tag_name(data: &[u8]) {
    let (_, text) = split(data);
    if let Ok(name) = Tag::validate_name(&text) {
        assert!(!name.is_empty(), "empty tag");
        assert!(name.chars().count() <= TAG_NAME_MAX_CHARS, "tag too long");
        assert!(
            name.chars()
                .all(|c| c.is_alphanumeric() || c == '_' || c == '-'),
            "tag charset"
        );
        assert_eq!(
            Tag::validate_name(&name).as_deref(),
            Ok(name.as_str()),
            "tag idempotent"
        );
    }
    let p = crate::make_preview(&text);
    assert!(p.chars().count() <= PREVIEW_MAX_CHARS, "preview too long");
    assert!(!p.contains(['\n', '\r']), "preview has a newline");
}

#[cfg(test)]
mod harness;
