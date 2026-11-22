//! Pure Win32 clipboard format mapping (Task 102). No OS calls, compiled on every OS so its
//! tests run everywhere.
//!
//! - Standard format ids get fixed names (`CF_UNICODETEXT`, `CF_DIB`, ...).
//! - Privacy markers are matched by **registered format id** (D-021), never by the enumerated
//!   name; a match reports the canonical marker name. The marker list is fixed in code and
//!   never configurable (GC-07).
//! - At most one `PlatformFormat` per `Mime` (S-1): image `PNG` > `CF_DIBV5` > `CF_DIB`; text
//!   `CF_UNICODETEXT` only; `HTML Format`, `Rich Text Format`, `CF_HDROP`. Everything else is
//!   listed with `mime: None`.
//!
//! Nothing here decides to skip (GC-04).

#![cfg_attr(not(windows), allow(dead_code))]

use darkclip_core::Mime;
use darkclip_platform::PlatformFormat;

// Standard clipboard formats (winuser.h).
pub(crate) const CF_TEXT: u32 = 1;
pub(crate) const CF_BITMAP: u32 = 2;
pub(crate) const CF_METAFILEPICT: u32 = 3;
pub(crate) const CF_SYLK: u32 = 4;
pub(crate) const CF_DIF: u32 = 5;
pub(crate) const CF_TIFF: u32 = 6;
pub(crate) const CF_OEMTEXT: u32 = 7;
pub(crate) const CF_DIB: u32 = 8;
pub(crate) const CF_PALETTE: u32 = 9;
pub(crate) const CF_PENDATA: u32 = 10;
pub(crate) const CF_RIFF: u32 = 11;
pub(crate) const CF_WAVE: u32 = 12;
pub(crate) const CF_UNICODETEXT: u32 = 13;
pub(crate) const CF_ENHMETAFILE: u32 = 14;
pub(crate) const CF_HDROP: u32 = 15;
pub(crate) const CF_LOCALE: u32 = 16;
pub(crate) const CF_DIBV5: u32 = 17;

/// Registered (`RegisterClipboardFormatW`) format ids are in `0xC000..=0xFFFF`.
const REGISTERED_FIRST: u32 = 0xC000;
const REGISTERED_LAST: u32 = 0xFFFF;

// Registered content formats mapped to a MIME (canonical names).
pub(crate) const PNG: &str = "PNG";
pub(crate) const HTML_FORMAT: &str = "HTML Format";
pub(crate) const RTF_FORMAT: &str = "Rich Text Format";

// Marker names spelled out for tests and the test writer only; production code uses the core
// list below, so the adapter can never fall behind it (D-032).
#[cfg(test)]
pub(crate) const EXCLUDE_MONITOR: &str = "ExcludeClipboardContentFromMonitorProcessing";
#[cfg(test)]
pub(crate) const HISTORY: &str = "CanIncludeInClipboardHistory";
#[cfg(test)]
pub(crate) const CLOUD: &str = "CanUploadToCloudClipboard";
#[cfg(test)]
pub(crate) const VIEWER_IGNORE: &str = "Clipboard Viewer Ignore";

/// Every Windows marker format name, taken from the core table (`windows_markers`). Fixed in
/// code; no setting changes it (GC-07).
pub(crate) const MARKER_NAMES: &[&str] = darkclip_core::markers::WINDOWS_MARKER_NAMES;

/// Marker formats whose DWORD value is read (the only data read in `snapshot_meta`).
pub(crate) fn marker_has_dword(name: &str) -> bool {
    darkclip_core::markers::WINDOWS_DWORD_MARKER_NAMES.contains(&name)
}

/// Fixed name of a standard clipboard format.
pub(crate) fn standard_format_name(id: u32) -> Option<&'static str> {
    Some(match id {
        CF_TEXT => "CF_TEXT",
        CF_BITMAP => "CF_BITMAP",
        CF_METAFILEPICT => "CF_METAFILEPICT",
        CF_SYLK => "CF_SYLK",
        CF_DIF => "CF_DIF",
        CF_TIFF => "CF_TIFF",
        CF_OEMTEXT => "CF_OEMTEXT",
        CF_DIB => "CF_DIB",
        CF_PALETTE => "CF_PALETTE",
        CF_PENDATA => "CF_PENDATA",
        CF_RIFF => "CF_RIFF",
        CF_WAVE => "CF_WAVE",
        CF_UNICODETEXT => "CF_UNICODETEXT",
        CF_ENHMETAFILE => "CF_ENHMETAFILE",
        CF_HDROP => "CF_HDROP",
        CF_LOCALE => "CF_LOCALE",
        CF_DIBV5 => "CF_DIBV5",
        _ => return None,
    })
}

/// True for ids in the registered range (names come from `GetClipboardFormatNameW`).
pub(crate) fn is_registered_id(id: u32) -> bool {
    (REGISTERED_FIRST..=REGISTERED_LAST).contains(&id)
}

/// A marker name with the id `RegisterClipboardFormatW` returned for it.
pub(crate) type MarkerId = (&'static str, u32);

/// Enumerated ids that are markers, matched by registered id only (D-021). Returns
/// `(id, canonical name)` in enumeration order, each marker once.
pub(crate) fn match_markers(enumerated: &[u32], registered: &[MarkerId]) -> Vec<MarkerId> {
    let mut out: Vec<MarkerId> = Vec::new();
    for &id in enumerated {
        if id == 0 {
            continue;
        }
        if let Some(&(name, _)) = registered.iter().find(|(_, rid)| *rid == id) {
            if !out.iter().any(|(n, _)| *n == name) {
                out.push((name, id));
            }
        }
    }
    out
}

/// One enumerated format: its id and its name (fixed for standard formats, registered name
/// otherwise; a fallback like `#0x0200` when the OS gave none).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OfferedFormat {
    pub id: u32,
    pub name: String,
}

/// The S-1 format list: every offered format once, in enumeration order, with at most one
/// format per `Mime`. Marker formats (by id) are named canonically and have no MIME.
/// `size_hint` is always `None`: a size needs the data handle (`GetClipboardData`), which is a
/// content fetch and may trigger delayed rendering, so it is not looked up here.
pub(crate) fn map_formats(offered: &[OfferedFormat], markers: &[MarkerId]) -> Vec<PlatformFormat> {
    // Best image candidate: lower rank wins (PNG 0, CF_DIBV5 1, CF_DIB 2).
    let image = offered
        .iter()
        .enumerate()
        .filter_map(|(i, f)| image_rank(f).map(|r| (r, i)))
        .min()
        .map(|(_, i)| i);
    let mut taken: Vec<Mime> = Vec::new();
    offered
        .iter()
        .enumerate()
        .map(|(i, f)| {
            if let Some(&(name, _)) = markers.iter().find(|(_, id)| *id != 0 && *id == f.id) {
                return format(name, None);
            }
            let (name, mime) = match content_mapping(f) {
                Some((name, Mime::ImagePng)) if image == Some(i) => (name, Some(Mime::ImagePng)),
                Some((_, Mime::ImagePng)) | None => (f.name.as_str(), None),
                Some((name, mime)) if !taken.contains(&mime) => (name, Some(mime)),
                Some(_) => (f.name.as_str(), None),
            };
            if let Some(m) = mime {
                taken.push(m);
            }
            format(name, mime)
        })
        .collect()
}

fn format(name: &str, mime: Option<Mime>) -> PlatformFormat {
    PlatformFormat {
        name: name.to_owned(),
        mime,
        size_hint: None,
    }
}

fn is_registered_named(f: &OfferedFormat, canonical: &str) -> bool {
    is_registered_id(f.id) && f.name.eq_ignore_ascii_case(canonical)
}

fn image_rank(f: &OfferedFormat) -> Option<u8> {
    if is_registered_named(f, PNG) {
        Some(0)
    } else if f.id == CF_DIBV5 {
        Some(1)
    } else if f.id == CF_DIB {
        Some(2)
    } else {
        None
    }
}

/// Canonical name and MIME of a content format this adapter can read, if any. Standard
/// formats map by id; registered ones by name (atoms compare case-insensitively).
fn content_mapping(f: &OfferedFormat) -> Option<(&'static str, Mime)> {
    match f.id {
        CF_UNICODETEXT => Some(("CF_UNICODETEXT", Mime::TextPlain)),
        CF_HDROP => Some(("CF_HDROP", Mime::UriList)),
        CF_DIBV5 => Some(("CF_DIBV5", Mime::ImagePng)),
        CF_DIB => Some(("CF_DIB", Mime::ImagePng)),
        _ if is_registered_named(f, PNG) => Some((PNG, Mime::ImagePng)),
        _ if is_registered_named(f, HTML_FORMAT) => Some((HTML_FORMAT, Mime::TextHtml)),
        _ if is_registered_named(f, RTF_FORMAT) => Some((RTF_FORMAT, Mime::TextRtf)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID_PNG: u32 = 0xC0A1;
    const ID_HTML: u32 = 0xC0A2;
    const ID_RTF: u32 = 0xC0A3;
    const ID_DATAOBJECT: u32 = 0xC0A4;
    const ID_OLE_PRIVATE: u32 = 0xC0A5;
    const ID_EXCLUDE: u32 = 0xC101;
    const ID_HISTORY: u32 = 0xC102;
    const ID_CLOUD: u32 = 0xC103;
    const ID_VIEWER: u32 = 0xC104;

    fn registered() -> Vec<MarkerId> {
        vec![
            (EXCLUDE_MONITOR, ID_EXCLUDE),
            (HISTORY, ID_HISTORY),
            (CLOUD, ID_CLOUD),
            (VIEWER_IGNORE, ID_VIEWER),
        ]
    }

    fn off(id: u32, name: &str) -> OfferedFormat {
        OfferedFormat {
            id,
            name: name.to_owned(),
        }
    }

    fn std_fmt(id: u32) -> OfferedFormat {
        off(id, standard_format_name(id).unwrap())
    }

    fn with_mime(list: &[PlatformFormat], mime: Mime) -> Vec<&str> {
        list.iter()
            .filter(|f| f.mime == Some(mime))
            .map(|f| f.name.as_str())
            .collect()
    }

    #[test]
    fn standard_names_are_fixed() {
        assert_eq!(standard_format_name(CF_UNICODETEXT), Some("CF_UNICODETEXT"));
        assert_eq!(standard_format_name(CF_DIB), Some("CF_DIB"));
        assert_eq!(standard_format_name(CF_DIBV5), Some("CF_DIBV5"));
        assert_eq!(standard_format_name(CF_HDROP), Some("CF_HDROP"));
        assert_eq!(standard_format_name(CF_TEXT), Some("CF_TEXT"));
        assert_eq!(standard_format_name(CF_LOCALE), Some("CF_LOCALE"));
        assert_eq!(standard_format_name(0), None);
        assert_eq!(standard_format_name(ID_PNG), None);
    }

    #[test]
    fn prefers_png_over_dibv5_over_dib() {
        let all = [
            std_fmt(CF_DIB),
            std_fmt(CF_BITMAP),
            std_fmt(CF_DIBV5),
            off(ID_PNG, "PNG"),
        ];
        let out = map_formats(&all, &registered());
        assert_eq!(out.len(), all.len(), "every offered format is listed");
        assert_eq!(with_mime(&out, Mime::ImagePng), vec!["PNG"]);

        let no_png = [std_fmt(CF_DIB), std_fmt(CF_BITMAP), std_fmt(CF_DIBV5)];
        let out = map_formats(&no_png, &registered());
        assert_eq!(with_mime(&out, Mime::ImagePng), vec!["CF_DIBV5"]);

        let dib_only = [std_fmt(CF_DIB), std_fmt(CF_BITMAP)];
        let out = map_formats(&dib_only, &registered());
        assert_eq!(with_mime(&out, Mime::ImagePng), vec!["CF_DIB"]);

        // CF_BITMAP alone is not an image this adapter reads.
        let out = map_formats(&[std_fmt(CF_BITMAP)], &registered());
        assert!(with_mime(&out, Mime::ImagePng).is_empty());
    }

    #[test]
    fn registered_png_name_any_case_but_only_in_registered_range() {
        let out = map_formats(&[off(ID_PNG, "png")], &registered());
        assert_eq!(with_mime(&out, Mime::ImagePng), vec!["PNG"]);
        // A standard-range id never maps by name.
        let out = map_formats(&[off(0x0201, "PNG")], &registered());
        assert!(with_mime(&out, Mime::ImagePng).is_empty());
    }

    #[test]
    fn text_only_unicodetext() {
        let set = [
            std_fmt(CF_UNICODETEXT),
            std_fmt(CF_LOCALE),
            std_fmt(CF_TEXT),
            std_fmt(CF_OEMTEXT),
        ];
        let out = map_formats(&set, &registered());
        assert_eq!(with_mime(&out, Mime::TextPlain), vec!["CF_UNICODETEXT"]);
        assert_eq!(out.iter().filter(|f| f.mime.is_some()).count(), 1);

        let ansi_only = [std_fmt(CF_TEXT), std_fmt(CF_OEMTEXT)];
        let out = map_formats(&ansi_only, &registered());
        assert!(out.iter().all(|f| f.mime.is_none()), "{out:?}");
    }

    #[test]
    fn placeholders_have_no_mime() {
        let set = [
            off(ID_DATAOBJECT, "DataObject"),
            off(ID_OLE_PRIVATE, "Ole Private Data"),
            off(ID_EXCLUDE, EXCLUDE_MONITOR),
            off(ID_HISTORY, HISTORY),
            off(ID_CLOUD, CLOUD),
            off(ID_VIEWER, VIEWER_IGNORE),
            off(0x0200, "#0x0200"),
            std_fmt(CF_LOCALE),
        ];
        let out = map_formats(&set, &registered());
        assert_eq!(out.len(), set.len());
        assert!(out.iter().all(|f| f.mime.is_none()), "{out:?}");
        assert!(out.iter().all(|f| f.size_hint.is_none()));
        let names: Vec<&str> = out.iter().map(|f| f.name.as_str()).collect();
        assert!(names.contains(&"DataObject"));
        assert!(names.contains(&"Ole Private Data"));
    }

    #[test]
    fn html_rtf_hdrop_mapping() {
        let set = [
            off(ID_HTML, "HTML Format"),
            off(ID_RTF, "Rich Text Format"),
            std_fmt(CF_HDROP),
            off(0xC0B0, "FileNameW"),
            off(0xC0B1, "FileName"),
            std_fmt(CF_UNICODETEXT),
        ];
        let out = map_formats(&set, &registered());
        assert_eq!(with_mime(&out, Mime::TextHtml), vec!["HTML Format"]);
        assert_eq!(with_mime(&out, Mime::TextRtf), vec!["Rich Text Format"]);
        assert_eq!(with_mime(&out, Mime::UriList), vec!["CF_HDROP"]);
        assert_eq!(with_mime(&out, Mime::TextPlain), vec!["CF_UNICODETEXT"]);
        assert_eq!(out.iter().filter(|f| f.mime.is_some()).count(), 4);
    }

    #[test]
    fn marker_matched_by_id_not_name_case() {
        // Lookalike names under ids that are not the registered marker ids: not markers.
        let lookalike_case = 0xC201;
        let lookalike_space = 0xC202;
        let enumerated = [CF_UNICODETEXT, lookalike_case, lookalike_space];
        assert!(match_markers(&enumerated, &registered()).is_empty());
        let names = [
            std_fmt(CF_UNICODETEXT),
            off(
                lookalike_case,
                "excludeclipboardcontentfrommonitorprocessing",
            ),
            off(lookalike_space, "CanIncludeInClipboardHistory "),
        ];
        let out = map_formats(&names, &registered());
        // Not renamed to a canonical marker name.
        assert!(!out
            .iter()
            .any(|f| f.name == EXCLUDE_MONITOR || f.name == HISTORY));

        // The registered id is a marker whatever name was enumerated for it, and reports the
        // canonical name.
        let enumerated = [CF_UNICODETEXT, ID_HISTORY, ID_EXCLUDE];
        assert_eq!(
            match_markers(&enumerated, &registered()),
            vec![(HISTORY, ID_HISTORY), (EXCLUDE_MONITOR, ID_EXCLUDE)]
        );
        let out = map_formats(&[off(ID_EXCLUDE, "garbled")], &registered());
        assert_eq!(out[0].name, EXCLUDE_MONITOR);
        assert_eq!(out[0].mime, None);
    }

    #[test]
    fn marker_matched_once_and_unregistered_never_matches() {
        let enumerated = [ID_CLOUD, ID_CLOUD];
        assert_eq!(
            match_markers(&enumerated, &registered()),
            vec![(CLOUD, ID_CLOUD)]
        );
        // An id 0 (failed registration) never matches anything.
        assert!(match_markers(&[0, CF_TEXT], &[(HISTORY, 0)]).is_empty());
    }

    #[test]
    fn dword_markers_are_history_and_cloud_only() {
        assert!(marker_has_dword(HISTORY));
        assert!(marker_has_dword(CLOUD));
        assert!(!marker_has_dword(EXCLUDE_MONITOR));
        assert!(!marker_has_dword(VIEWER_IGNORE));
        assert_eq!(MARKER_NAMES.len(), 4);
    }

    #[test]
    fn adapter_covers_every_core_windows_marker() {
        use darkclip_core::markers::{WINDOWS_DWORD_MARKER_NAMES, WINDOWS_MARKER_NAMES};
        assert_eq!(MARKER_NAMES, WINDOWS_MARKER_NAMES);
        for name in WINDOWS_MARKER_NAMES {
            assert_eq!(
                marker_has_dword(name),
                WINDOWS_DWORD_MARKER_NAMES.contains(name),
                "{name}"
            );
        }
    }

    #[test]
    fn marker_names_match_core_table() {
        // Each canonical name produces a marker in the core table (single source of meaning).
        for &name in MARKER_NAMES {
            let m = darkclip_core::markers::windows_markers(&[(name, Some(0))]);
            assert!(!m.is_empty(), "{name}");
        }
    }
}
