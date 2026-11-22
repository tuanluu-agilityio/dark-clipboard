//! `WinAccess`: clipboard access from the capture actor's thread.
//!
//! `sequence()` (Task 101), `snapshot_meta` (Task 102), `read` (Task 103) and `write`
//! (Task 104, copy-back). Adapter rule (S-5): a failed `OpenClipboard` returns
//! `PlatformError::Busy` once; the actor owns retries. No policy here (GC-04), no content in
//! errors (GC-06).
//!
//! Both go through the small `ClipboardOs` seam so tests can count OS calls. `snapshot_meta`
//! never fetches the data of a content format: the only data it reads are the DWORDs of the
//! `CanIncludeInClipboardHistory` / `CanUploadToCloudClipboard` markers (privacy-design §2).
//! `read` fetches only the formats in the plan, checks `GlobalSize` before any copy (the only
//! image cap on Windows, since `size_hint` is `None`: an oversize image is `TooLarge`), and
//! wipes every intermediate buffer. An invalid PNG or an unsupported/malformed DIB skips that
//! representation only.

use crate::formats::{self, MarkerId, OfferedFormat, CF_DIB, CF_DIBV5, CF_HDROP, CF_UNICODETEXT};
use crate::source::WinSourceResolver;
use crate::win;
use crate::{dib, text};
use darkclip_core::markers::windows_markers;
use darkclip_core::policy::ReadPlan;
use darkclip_core::{ClipboardSource, Mime, RawRepresentation};
use darkclip_platform::{
    ClipboardAccess, ClipboardMeta, PlatformError, PlatformFormat, SourceResolver,
};
use zeroize::Zeroizing;

/// The OS calls `snapshot_meta` needs. The real implementation is `WinOs`; tests use a fake.
pub(crate) trait ClipboardOs {
    /// The clipboard, open by this thread; closed when dropped.
    type Open<'a>: OpenClipboardView
    where
        Self: 'a;

    /// `RegisterClipboardFormatW`; `None` when registration fails (id 0).
    fn register(&mut self, name: &str) -> Option<u32>;

    /// One `OpenClipboard` attempt; `Busy` when another process holds it (S-5).
    fn open(&mut self) -> Result<Self::Open<'_>, PlatformError>;
}

/// Calls made while the clipboard is open.
pub(crate) trait OpenClipboardView {
    /// `GetClipboardSequenceNumber`, read while open so it matches the enumerated formats.
    fn sequence(&self) -> u64;
    /// Every format id, in clipboard order. An enumeration error is an error, never a
    /// shortened list (a missing marker must not look like "no marker").
    fn enum_formats(&self) -> Result<Vec<u32>, PlatformError>;
    /// `GetClipboardFormatNameW` for a registered or private format.
    fn format_name(&self, id: u32) -> Option<String>;
    /// The DWORD of a marker format: data exactly 4 bytes, else `None`. Called only for the
    /// two DWORD marker ids.
    fn dword_value(&self, id: u32) -> Option<u32>;
    /// The clipboard owner's app id (`WinSourceResolver`).
    fn owner_source(&self) -> Option<ClipboardSource>;
    /// `GetClipboardData` + `GlobalSize` of a planned format: the size of its data block, or
    /// `None` when there is no data. Reads no bytes. Only for formats in the read plan.
    fn data_size(&self, id: u32) -> Option<u64>;
    /// The first `min(GlobalSize, max)` bytes of a planned format's data (`GlobalLock`, copy,
    /// unlock), or `None` when unavailable. Only after `data_size` passed the caps.
    fn copy_data(&self, id: u32, max: usize) -> Option<Zeroizing<Vec<u8>>>;
}

/// Registered ids of the marker names, cached after the first full registration.
#[derive(Default)]
pub(crate) struct MarkerCache(Option<Vec<MarkerId>>);

impl MarkerCache {
    /// The marker ids, registering the names on first use. Needs no open clipboard, so it
    /// runs before `open` and keeps the clipboard open as briefly as possible. A failed
    /// registration is an error (the change is not captured), never a missed marker.
    fn ids<O: ClipboardOs>(&mut self, os: &mut O) -> Result<Vec<MarkerId>, PlatformError> {
        if let Some(ids) = &self.0 {
            return Ok(ids.clone());
        }
        let mut ids = Vec::with_capacity(formats::MARKER_NAMES.len());
        for &name in formats::MARKER_NAMES {
            let id = os
                .register(name)
                .ok_or(PlatformError::Backend { code: -1 })?;
            ids.push((name, id));
        }
        self.0 = Some(ids.clone());
        Ok(ids)
    }
}

/// `snapshot_meta` over the seam: names, markers (by id), sequence and source; never content.
/// The only data fetched are the DWORDs of the history and cloud markers.
pub(crate) fn snapshot<O: ClipboardOs>(
    os: &mut O,
    cache: &mut MarkerCache,
) -> Result<ClipboardMeta, PlatformError> {
    let registered = cache.ids(os)?;
    let (seq, offered, found, source) = {
        let open = os.open()?;
        let seq = open.sequence();
        let ids = open.enum_formats()?;
        let offered: Vec<OfferedFormat> = ids
            .iter()
            .map(|&id| OfferedFormat {
                id,
                name: format_name(&open, id),
            })
            .collect();
        let found: Vec<(&'static str, Option<u32>)> = formats::match_markers(&ids, &registered)
            .into_iter()
            .map(|(name, id)| {
                let value = if formats::marker_has_dword(name) {
                    open.dword_value(id)
                } else {
                    None
                };
                (name, value)
            })
            .collect();
        let source = open.owner_source();
        (seq, offered, found, source)
        // `open` closes the clipboard here.
    };
    let formats: Vec<PlatformFormat> = formats::map_formats(&offered, &registered);
    Ok(ClipboardMeta {
        seq,
        formats,
        markers: windows_markers(&found),
        source,
    })
}

/// How a planned format is read and converted.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ReadKind {
    /// `CF_UNICODETEXT`: UTF-16, converted to UTF-8.
    Text,
    /// `HTML Format`: the whole CF_HTML payload (header included) as UTF-8.
    Html,
    /// `Rich Text Format`: raw bytes.
    Rtf,
    /// Registered `PNG`: bytes as they are, signature checked.
    Png,
    /// `CF_DIBV5` / `CF_DIB`: converted to PNG.
    Dib,
    /// `CF_HDROP`: `text/uri-list`.
    Files,
}

/// Where the format id comes from.
#[derive(Clone, Copy)]
enum FormatId {
    Standard(u32),
    Registered,
}

/// The read of a planned format: its canonical name, kind and id source, or `None` for any
/// name/MIME pair this adapter does not read (never read then).
fn planned(name: &str, mime: Mime) -> Option<(&'static str, ReadKind, FormatId)> {
    use FormatId::{Registered, Standard};
    let (canonical, kind, id, want) = match name {
        "CF_UNICODETEXT" => (
            name_of(CF_UNICODETEXT)?,
            ReadKind::Text,
            Standard(CF_UNICODETEXT),
            Mime::TextPlain,
        ),
        "CF_DIBV5" => (
            name_of(CF_DIBV5)?,
            ReadKind::Dib,
            Standard(CF_DIBV5),
            Mime::ImagePng,
        ),
        "CF_DIB" => (
            name_of(CF_DIB)?,
            ReadKind::Dib,
            Standard(CF_DIB),
            Mime::ImagePng,
        ),
        "CF_HDROP" => (
            name_of(CF_HDROP)?,
            ReadKind::Files,
            Standard(CF_HDROP),
            Mime::UriList,
        ),
        _ if name.eq_ignore_ascii_case(formats::PNG) => {
            (formats::PNG, ReadKind::Png, Registered, Mime::ImagePng)
        }
        _ if name.eq_ignore_ascii_case(formats::HTML_FORMAT) => (
            formats::HTML_FORMAT,
            ReadKind::Html,
            Registered,
            Mime::TextHtml,
        ),
        _ if name.eq_ignore_ascii_case(formats::RTF_FORMAT) => (
            formats::RTF_FORMAT,
            ReadKind::Rtf,
            Registered,
            Mime::TextRtf,
        ),
        _ => return None,
    };
    (mime == want).then_some((canonical, kind, id))
}

fn name_of(id: u32) -> Option<&'static str> {
    formats::standard_format_name(id)
}

/// Bytes fetched while the clipboard was open; converted after it is closed. Wiped on drop.
struct Fetched {
    bytes: Zeroizing<Vec<u8>>,
    /// The copy holds the whole `GlobalSize` block (not cut by the read limit).
    complete: bool,
}

fn saturating_usize(v: u64) -> usize {
    usize::try_from(v).unwrap_or(usize::MAX)
}

/// `read` over the seam: one `OpenClipboard` attempt (`Busy` otherwise), only the planned
/// formats, `GlobalSize` checked before any copy, every buffer bounded by the plan's caps.
/// The clipboard is closed before conversion (UTF-16, DIB → PNG) so it is held briefly.
pub(crate) fn read_plan<O: ClipboardOs>(
    os: &mut O,
    plan: &ReadPlan,
) -> Result<Vec<RawRepresentation>, PlatformError> {
    let text_cap = saturating_usize(plan.truncate_text_at);
    let image_cap = plan.image_max_bytes;

    // Resolve ids before opening (registration needs no open clipboard).
    let mut wanted: Vec<(&'static str, ReadKind, u32, Mime)> = Vec::new();
    for f in &plan.formats {
        let Some((name, kind, id)) = planned(&f.platform_format, f.mime) else {
            continue;
        };
        let id = match id {
            FormatId::Standard(id) => id,
            FormatId::Registered => match os.register(name) {
                Some(id) => id,
                None => continue,
            },
        };
        if !wanted.iter().any(|w| w.2 == id) {
            wanted.push((name, kind, id, f.mime));
        }
    }
    if wanted.is_empty() {
        return Ok(Vec::new());
    }

    let fetched = {
        let open = os.open()?;
        let mut fetched: Vec<(&'static str, ReadKind, Mime, Fetched)> = Vec::new();
        for &(name, kind, id, mime) in &wanted {
            // GlobalSize first: nothing is locked or copied before this check.
            let Some(size) = open.data_size(id) else {
                continue;
            };
            let limit = match kind {
                ReadKind::Text => text::utf16_raw_limit(text_cap),
                ReadKind::Html | ReadKind::Rtf => text::utf8_raw_limit(text_cap),
                ReadKind::Png | ReadKind::Dib => {
                    if size > image_cap {
                        // `open` closes the clipboard; buffers already fetched are wiped.
                        return Err(PlatformError::TooLarge {
                            format: name.to_owned(),
                            bytes: size,
                        });
                    }
                    saturating_usize(size)
                }
                // The DROPFILES block is parsed here (`text::parse_dropfiles`), never handed
                // to shell32 unchecked.
                ReadKind::Files => text::dropfiles_raw_limit(text_cap),
            };
            let limit = limit.min(saturating_usize(size));
            if let Some(bytes) = open.copy_data(id, limit) {
                let complete = u64::try_from(bytes.len()).ok() == Some(size);
                fetched.push((name, kind, mime, Fetched { bytes, complete }));
            }
        }
        fetched
        // `open` closes the clipboard here.
    };

    let mut reps = Vec::with_capacity(fetched.len());
    for (
        name,
        kind,
        mime,
        Fetched {
            bytes: raw,
            complete,
        },
    ) in fetched
    {
        let rep = |bytes: Vec<u8>, truncated: bool| RawRepresentation {
            mime,
            platform_format: name.to_owned(),
            bytes,
            width: None,
            height: None,
            truncated,
        };
        match kind {
            ReadKind::Text => {
                let (s, cut) = text::utf16_capped(&raw, text_cap);
                reps.push(rep(s.into_bytes(), cut));
            }
            ReadKind::Html => {
                let (b, cut) = text::utf8_capped(&raw, text_cap);
                reps.push(rep(b, cut));
            }
            ReadKind::Rtf => {
                let (b, cut) = text::bytes_capped(&raw, text_cap);
                reps.push(rep(b, cut));
            }
            ReadKind::Png => {
                // Not a PNG: that representation is skipped.
                if let Some((w, h)) = png_dimensions(&raw) {
                    let mut r = rep(raw.to_vec(), false);
                    r.width = Some(w);
                    r.height = Some(h);
                    reps.push(r);
                }
            }
            ReadKind::Dib => match dib::dib_to_png(&raw, image_cap) {
                Ok(img) => {
                    let mut r = rep(img.png, false);
                    r.width = Some(img.width);
                    r.height = Some(img.height);
                    reps.push(r);
                }
                Err(dib::DibError::TooLarge { bytes }) => {
                    return Err(PlatformError::TooLarge {
                        format: name.to_owned(),
                        bytes,
                    });
                }
                // Unsupported layout or malformed header: that representation is skipped.
                Err(dib::DibError::Unsupported(_) | dib::DibError::Malformed(_)) => {}
            },
            ReadKind::Files => {
                // Malformed block: that representation is skipped.
                if let Some(files) = text::parse_dropfiles(&raw, complete, text_cap) {
                    let (list, cut) = text::uri_list(&files.names, text_cap);
                    if !list.is_empty() {
                        reps.push(rep(list.into_bytes(), cut || files.more));
                    }
                }
            }
        }
    }
    Ok(reps)
}

const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1A, b'\n'];

/// Width and height from the IHDR chunk of a PNG, or `None` when the signature or the first
/// chunk is not a PNG's.
fn png_dimensions(b: &[u8]) -> Option<(u32, u32)> {
    if b.get(..8)? != PNG_SIGNATURE || b.get(12..16)? != b"IHDR" {
        return None;
    }
    let be = |off: usize| -> Option<u32> {
        let s = b.get(off..off + 4)?;
        Some(u32::from_be_bytes([
            *s.first()?,
            *s.get(1)?,
            *s.get(2)?,
            *s.get(3)?,
        ]))
    };
    let (w, h) = (be(16)?, be(20)?);
    (w > 0 && h > 0).then_some((w, h))
}

/// Fixed name for standard formats, registered name otherwise, `#0xNNNN` when there is none.
fn format_name<V: OpenClipboardView>(open: &V, id: u32) -> String {
    if let Some(name) = formats::standard_format_name(id) {
        return name.to_owned();
    }
    open.format_name(id)
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| format!("#0x{id:04X}"))
}

/// The OS calls `write` needs (Task 104). The real implementation is `WinOs`; tests use a fake.
pub(crate) trait ClipboardWriteOs: ClipboardOs {
    /// A movable global memory block holding one format's data. Freed (and wiped) on drop
    /// unless `ClipboardWriter::set` handed it to the system.
    type Block;
    /// The clipboard owner window of one write (S-2, D-038): a message-only window created
    /// on the calling thread for that write only; destroyed (and its class unregistered)
    /// on drop.
    type Owner;
    /// The clipboard, open by this thread with an owner window; closed when dropped.
    type Writer<'a>: ClipboardWriter<Self::Block>
    where
        Self: 'a;

    /// `GlobalAlloc(GMEM_MOVEABLE)` holding a copy of `bytes`.
    fn alloc(&mut self, bytes: &[u8]) -> Result<Self::Block, PlatformError>;

    /// A new owner window on this thread.
    fn create_owner(&mut self) -> Result<Self::Owner, PlatformError>;

    /// One `OpenClipboard(owner)` attempt; `Busy` when another process holds it (S-5).
    fn open_for_write(&mut self, owner: &Self::Owner) -> Result<Self::Writer<'_>, PlatformError>;

    /// `GetClipboardSequenceNumber` (clipboard closed).
    fn sequence_now(&self) -> u64;

    /// `GetClipboardOwner() == owner`: nobody has emptied the clipboard since our write.
    fn owns_clipboard(&self, owner: &Self::Owner) -> bool;
}

/// Calls made while the clipboard is open for writing.
pub(crate) trait ClipboardWriter<B> {
    /// `EmptyClipboard`: our owner window becomes the clipboard owner.
    fn empty(&mut self) -> Result<(), PlatformError>;
    /// `SetClipboardData(id, block)`. On success the system owns the block; on failure the
    /// block is dropped here, which frees it.
    fn set(&mut self, id: u32, block: B) -> Result<(), PlatformError>;
    /// `GetClipboardSequenceNumber`, read while the clipboard is still open. Only the
    /// fallback value: `CloseClipboard` still bumps the number (D-038).
    fn sequence_while_open(&self) -> u64;
}

/// Nothing in the clip can be written on Windows (for example only `http:` URIs).
const NOTHING_TO_WRITE: PlatformError = PlatformError::Unsupported("nothing to write");

/// Where a written format's id comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Target {
    Standard(u32),
    Registered(&'static str),
}

/// One format to write and its data, wiped on drop.
struct Planned {
    target: Target,
    bytes: Zeroizing<Vec<u8>>,
}

/// The formats a set of representations becomes, in order: plain text → `CF_UNICODETEXT`;
/// HTML → `HTML Format`; RTF → `Rich Text Format`; PNG → `PNG`, `CF_DIBV5`, `CF_DIB`;
/// `file://` URI lists → `CF_HDROP`. Only the first convertible representation of each MIME
/// is used. A representation that cannot be converted (an undecodable PNG, a URI list with
/// any URI that is not a drive or UNC `file:` URI) is skipped. D-012: the only formats this
/// function can produce are those above; no exclusion, history or cloud marker is written.
fn plan_write(reps: &[RawRepresentation]) -> Vec<Planned> {
    let mut out = Vec::new();
    let mut done: Vec<Mime> = Vec::new();
    for rep in reps {
        if done.contains(&rep.mime) {
            continue;
        }
        let planned: Option<Vec<Planned>> = match rep.mime {
            Mime::TextPlain => Some(vec![Planned {
                target: Target::Standard(CF_UNICODETEXT),
                bytes: utf16z_of_utf8(&rep.bytes),
            }]),
            Mime::TextHtml => Some(vec![Planned {
                target: Target::Registered(formats::HTML_FORMAT),
                bytes: nul_terminated(&rep.bytes),
            }]),
            Mime::TextRtf => Some(vec![Planned {
                target: Target::Registered(formats::RTF_FORMAT),
                bytes: nul_terminated(&rep.bytes),
            }]),
            Mime::ImagePng => dib::png_to_dibs(&rep.bytes).ok().map(|pair| {
                vec![
                    Planned {
                        target: Target::Registered(formats::PNG),
                        bytes: Zeroizing::new(rep.bytes.clone()),
                    },
                    Planned {
                        target: Target::Standard(CF_DIBV5),
                        bytes: pair.v5,
                    },
                    Planned {
                        target: Target::Standard(CF_DIB),
                        bytes: pair.dib,
                    },
                ]
            }),
            Mime::UriList => uri_list_to_dropfiles(&rep.bytes).map(|block| {
                vec![Planned {
                    target: Target::Standard(CF_HDROP),
                    bytes: block,
                }]
            }),
        };
        if let Some(p) = planned {
            done.push(rep.mime);
            out.extend(p);
        }
    }
    out
}

/// UTF-8 (lossy) as UTF-16LE with a terminating NUL, in buffers reserved up front and wiped
/// on drop.
fn utf16z_of_utf8(bytes: &[u8]) -> Zeroizing<Vec<u8>> {
    let text = Zeroizing::new(String::from_utf8_lossy(bytes).into_owned());
    // Each UTF-8 byte yields at most one UTF-16 unit (two bytes).
    let mut out = Zeroizing::new(Vec::with_capacity(
        text.len().saturating_mul(2).saturating_add(2),
    ));
    for u in text.encode_utf16() {
        out.extend_from_slice(&u.to_le_bytes());
    }
    out.extend_from_slice(&[0, 0]);
    out
}

/// `bytes` with one terminating NUL: readers of `HTML Format` / `Rich Text Format` expect a
/// C string, and the stored bytes have none because `read` stops at the NUL.
fn nul_terminated(bytes: &[u8]) -> Zeroizing<Vec<u8>> {
    let mut out = Zeroizing::new(Vec::with_capacity(bytes.len().saturating_add(1)));
    out.extend_from_slice(bytes);
    if out.last() != Some(&0) {
        out.push(0);
    }
    out
}

/// Size of the `DROPFILES` header: `pFiles`, `pt` (two LONGs), `fNC`, `fWide`.
const DROPFILES_HEADER: u32 = 20;

/// A `text/uri-list` as a wide `DROPFILES` block for `CF_HDROP`: the 20-byte header
/// (`pFiles = 20`, `fWide = 1`), each path UTF-16 and NUL-terminated, then a final NUL.
/// Lines starting with `#` are comments (RFC 2483). `None` when the list has no URI or any
/// URI is not a `file:` URI naming a drive or UNC path: that representation is skipped (a
/// partial file list would paste something else than what was copied). Buffers are wiped on
/// drop.
fn uri_list_to_dropfiles(list: &[u8]) -> Option<Zeroizing<Vec<u8>>> {
    let list = std::str::from_utf8(list).ok()?;
    let mut paths: Vec<Zeroizing<String>> = Vec::new();
    for line in list.split('\n') {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        paths.push(file_uri_to_path(line)?);
    }
    if paths.is_empty() {
        return None;
    }
    let units = paths
        .iter()
        .map(|p| p.encode_utf16().count().saturating_add(1))
        .fold(1usize, usize::saturating_add);
    let mut out = Zeroizing::new(Vec::with_capacity(
        (DROPFILES_HEADER as usize).saturating_add(units.saturating_mul(2)),
    ));
    out.extend_from_slice(&DROPFILES_HEADER.to_le_bytes()); // pFiles
    out.extend_from_slice(&[0u8; 12]); // pt.x, pt.y, fNC
    out.extend_from_slice(&1u32.to_le_bytes()); // fWide
    for p in &paths {
        for u in p.encode_utf16() {
            out.extend_from_slice(&u.to_le_bytes());
        }
        out.extend_from_slice(&[0, 0]);
    }
    out.extend_from_slice(&[0, 0]);
    Some(out)
}

/// A `file:` URI (as `text::file_uri` writes them) as a Windows path: `file:///C:/a%20b` →
/// `C:\a b`, `file://host/share/x` → `\\host\share\x` (`localhost` counts as local).
/// `None` for another scheme, a query or fragment, a path that is neither a drive nor a UNC
/// path, a bad escape, invalid UTF-8 or an embedded NUL.
fn file_uri_to_path(uri: &str) -> Option<Zeroizing<String>> {
    if !uri.get(..5)?.eq_ignore_ascii_case("file:") || uri.contains(['?', '#']) {
        return None;
    }
    let rest = uri.get(5..)?;
    let (host, path) = match rest.strip_prefix("//") {
        Some(after) => after.split_once('/')?,
        None => ("", rest.strip_prefix('/')?),
    };
    let mut out = Zeroizing::new(String::with_capacity(uri.len().saturating_add(2)));
    if host.is_empty() || host.eq_ignore_ascii_case("localhost") {
        let decoded = percent_decode(path)?;
        let drive = matches!(decoded.as_bytes(), [d, b':'] | [d, b':', b'/', ..] if d.is_ascii_alphabetic());
        if !drive {
            return None;
        }
        out.push_str(&decoded);
    } else {
        let host = percent_decode(host)?;
        let path = percent_decode(path)?;
        if host.contains(['/', '\\']) || path.is_empty() {
            return None;
        }
        out.push_str("//");
        out.push_str(&host);
        out.push('/');
        out.push_str(&path);
    }
    if out.contains('\0') {
        return None;
    }
    // Same length in bytes: '/' and '\' are both one byte.
    let mut windows = Zeroizing::new(String::with_capacity(out.len()));
    windows.extend(out.chars().map(|c| if c == '/' { '\\' } else { c }));
    Some(windows)
}

/// Percent-decodes `s` as UTF-8; `None` on a bad escape or invalid UTF-8. Wiped on drop.
fn percent_decode(s: &str) -> Option<Zeroizing<String>> {
    let b = s.as_bytes();
    let mut out = Zeroizing::new(Vec::with_capacity(b.len()));
    let mut i = 0;
    while let Some(&c) = b.get(i) {
        if c == b'%' {
            let hex = s.get(i + 1..i + 3)?;
            if !hex.bytes().all(|h| h.is_ascii_hexdigit()) {
                return None;
            }
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(c);
            i += 1;
        }
    }
    let text = std::str::from_utf8(&out).ok()?;
    Some(Zeroizing::new(text.to_owned()))
}

/// `write` over the seam (Task 104). Converts and allocates every block before opening the
/// clipboard, so it is held briefly. Then: a fresh owner window, one `OpenClipboard` attempt
/// (`Busy` otherwise), `EmptyClipboard`, `SetClipboardData` per format, `CloseClipboard`,
/// the sequence number, the owner check, and the window is destroyed (D-038). Any failure
/// after the open closes the clipboard and returns `Backend`. On every path the clipboard is
/// closed before the window is destroyed, and every block not handed to the system is freed
/// (drop order: blocks and writer, then the window).
///
/// The window lives only for this call because the capture actor's thread never pumps
/// messages: a long-lived owner would receive `WM_DESTROYCLIPBOARD` (sent when another app
/// empties the clipboard) and could stall that app. Destroying the owner leaves the data on
/// the clipboard (nothing is delayed-rendered). Any thread may call this; each call's window
/// is created and destroyed on that thread.
pub(crate) fn write_reps<O: ClipboardWriteOs>(
    os: &mut O,
    reps: &[RawRepresentation],
) -> Result<u64, PlatformError> {
    let planned = plan_write(reps);
    if planned.is_empty() {
        return Err(NOTHING_TO_WRITE);
    }
    let mut blocks: Vec<(u32, O::Block)> = Vec::with_capacity(planned.len());
    for p in planned {
        let id = match p.target {
            Target::Standard(id) => id,
            Target::Registered(name) => os
                .register(name)
                .ok_or(PlatformError::Backend { code: -1 })?,
        };
        blocks.push((id, os.alloc(&p.bytes)?));
        // `p.bytes` is wiped here.
    }

    // Declared before the writer, so on an early return the clipboard closes first.
    let owner = os.create_owner()?;
    let before_close = {
        let mut open = os.open_for_write(&owner)?;
        open.empty().map_err(|e| after_open(&e))?;
        // On an early return the blocks left in the iterator are dropped, which frees them.
        for (id, block) in blocks {
            open.set(id, block).map_err(|e| after_open(&e))?;
        }
        open.sequence_while_open()
        // `open` closes the clipboard here.
    };
    let seq = seq_after_close(os, &owner, before_close);
    drop(owner); // DestroyWindow, UnregisterClassW
    Ok(seq)
}

/// The sequence number the capture actor must ignore for our write (D-038, capture rule 5).
///
/// `CloseClipboard` bumps the number (the system adds synthesized formats at close), and the
/// monitor's `WM_CLIPBOARDUPDATE` carries the post-close value (spike answer 8), so it is
/// read after the close. It is ours only if our window still owns the clipboard *after* the
/// read: any writer that got in between our close and the read must have emptied the
/// clipboard, which changes the owner. Otherwise the pre-close value is returned: no signal
/// carries it, so another app's copy is never suppressed (at worst our own copy-back is seen
/// once more and deduplicated).
fn seq_after_close<O: ClipboardWriteOs>(os: &O, owner: &O::Owner, before_close: u64) -> u64 {
    let after = os.sequence_now();
    if os.owns_clipboard(owner) {
        after
    } else {
        before_close
    }
}

/// A failure while the clipboard is open is always `Backend`, never `Busy`: a retry would
/// start over on a clipboard we already emptied.
fn after_open(e: &PlatformError) -> PlatformError {
    match e {
        PlatformError::Backend { code } => PlatformError::Backend { code: *code },
        _ => PlatformError::Backend { code: -1 },
    }
}

/// The real seam over `win.rs`.
#[derive(Default)]
pub(crate) struct WinOs {
    _private: (),
}

impl ClipboardOs for WinOs {
    type Open<'a> = win::ClipboardGuard;

    fn register(&mut self, name: &str) -> Option<u32> {
        win::register_clipboard_format(name)
    }

    fn open(&mut self) -> Result<win::ClipboardGuard, PlatformError> {
        win::ClipboardGuard::open_for_read()
    }
}

impl ClipboardWriteOs for WinOs {
    type Block = win::GlobalBlock;
    type Owner = win::OwnerWindow;
    type Writer<'a> = win::ClipboardGuard;

    fn alloc(&mut self, bytes: &[u8]) -> Result<win::GlobalBlock, PlatformError> {
        win::GlobalBlock::with_bytes(bytes)
    }

    fn create_owner(&mut self) -> Result<win::OwnerWindow, PlatformError> {
        win::OwnerWindow::create()
    }

    fn open_for_write(
        &mut self,
        owner: &win::OwnerWindow,
    ) -> Result<win::ClipboardGuard, PlatformError> {
        win::ClipboardGuard::open_for_write(owner)
    }

    fn sequence_now(&self) -> u64 {
        u64::from(win::clipboard_sequence())
    }

    fn owns_clipboard(&self, owner: &win::OwnerWindow) -> bool {
        owner.owns_clipboard()
    }
}

impl ClipboardWriter<win::GlobalBlock> for win::ClipboardGuard {
    fn empty(&mut self) -> Result<(), PlatformError> {
        win::ClipboardGuard::empty(self)
    }

    fn set(&mut self, id: u32, block: win::GlobalBlock) -> Result<(), PlatformError> {
        win::ClipboardGuard::set_block(self, id, block)
    }

    fn sequence_while_open(&self) -> u64 {
        u64::from(win::clipboard_sequence())
    }
}

impl OpenClipboardView for win::ClipboardGuard {
    fn sequence(&self) -> u64 {
        u64::from(win::clipboard_sequence())
    }

    fn enum_formats(&self) -> Result<Vec<u32>, PlatformError> {
        win::ClipboardGuard::enum_formats(self)
    }

    fn format_name(&self, id: u32) -> Option<String> {
        win::clipboard_format_name(id)
    }

    fn dword_value(&self, id: u32) -> Option<u32> {
        win::ClipboardGuard::dword_value(self, id)
    }

    fn owner_source(&self) -> Option<ClipboardSource> {
        WinSourceResolver.resolve()
    }

    fn data_size(&self, id: u32) -> Option<u64> {
        win::ClipboardGuard::data_size(self, id)
    }

    fn copy_data(&self, id: u32, max: usize) -> Option<Zeroizing<Vec<u8>>> {
        win::ClipboardGuard::copy_data(self, id, max)
    }
}

/// Win32 clipboard access.
#[derive(Default)]
pub struct WinAccess {
    os: WinOs,
    markers: MarkerCache,
}

impl WinAccess {
    pub fn new() -> Self {
        Self::default()
    }
}

impl ClipboardAccess for WinAccess {
    /// `GetClipboardSequenceNumber`; never fails (F-03).
    fn sequence(&self) -> Result<u64, PlatformError> {
        Ok(u64::from(win::clipboard_sequence()))
    }

    /// One `OpenClipboard` attempt (`Busy` otherwise), formats, markers, sequence, source.
    fn snapshot_meta(&mut self) -> Result<ClipboardMeta, PlatformError> {
        snapshot(&mut self.os, &mut self.markers)
    }

    /// One `OpenClipboard` attempt (`Busy` otherwise); only the planned formats, capped.
    fn read(&mut self, plan: &ReadPlan) -> Result<Vec<RawRepresentation>, PlatformError> {
        read_plan(&mut self.os, plan)
    }

    /// Replaces the clipboard with `reps` (copy-back) and returns the sequence number of that
    /// change, read after `CloseClipboard` while we are still the owner (D-038). Never
    /// writes marker formats (D-012).
    fn write(&mut self, reps: &[RawRepresentation]) -> Result<u64, PlatformError> {
        write_reps(&mut self.os, reps)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::formats::{
        CF_DIB, CF_DIBV5, CF_HDROP, CF_LOCALE, CF_OEMTEXT, CF_TEXT, CF_UNICODETEXT, CLOUD,
        EXCLUDE_MONITOR, HISTORY,
    };
    use darkclip_core::markers::PrivacyMarker;
    use darkclip_core::policy::AvailableFormat;
    use darkclip_core::{Mime, Platform, SourceConfidence};
    use std::cell::{Cell, RefCell};
    use std::sync::atomic::{AtomicU32, Ordering as AtomicOrdering};
    use std::sync::{Arc, Mutex};

    /// Fake OS: a clipboard of `(id, registered name)` entries plus DWORD data for some ids.
    /// Registration mimics atoms (case-insensitive, one id per name).
    struct FakeOs {
        /// `(id, name)`; standard formats carry no name.
        clipboard: Vec<(u32, Option<&'static str>)>,
        /// Registered names and their ids.
        atoms: RefCell<Vec<(String, u32)>>,
        /// DWORD data per id (`None` = data not exactly 4 bytes or unreadable).
        dwords: Vec<(u32, Option<u32>)>,
        busy: bool,
        enum_fails: bool,
        seq: u64,
        opens: Cell<u32>,
        closes: Cell<u32>,
        registers: Cell<u32>,
        /// Every name passed to `register`, in order.
        registered_names: RefCell<Vec<String>>,
        /// Every id passed to the data getter.
        data_reads: RefCell<Vec<u32>>,
        /// Content per id (what `copy_data` returns, cut to `max`).
        content: Vec<(u32, Vec<u8>)>,
        /// `GlobalSize` overrides per id (else the content length).
        sizes: Vec<(u32, u64)>,
        /// `CF_HDROP` names.
        files: Vec<&'static str>,
        /// Every id passed to `data_size`.
        size_calls: RefCell<Vec<u32>>,
        /// Every `(id, max)` passed to `copy_data`.
        copy_calls: RefCell<Vec<(u32, usize)>>,
        /// Write seam: every call, in order (shared with the fake owner windows).
        events: Arc<Mutex<Vec<Ev>>>,
        /// Creating the owner window fails.
        owner_fails: bool,
        /// Another writer empties the clipboard right after our `CloseClipboard`.
        stolen_after_close: bool,
        /// `EmptyClipboard` fails.
        empty_fails: bool,
        /// The n-th `SetClipboardData` (0-based) fails.
        fail_set_at: Option<usize>,
        /// Blocks allocated, and blocks freed (shared with the blocks).
        allocs: Cell<u32>,
        freed: Arc<AtomicU32>,
        /// `(id, bytes)` handed to the clipboard, in order.
        written: RefCell<Vec<(u32, Vec<u8>)>>,
        /// The sequence number: +1 per `SetClipboardData`, +4 at `CloseClipboard` (the
        /// system adds synthesized formats at close, D-038), +5 for a stealing writer.
        write_seq: Cell<u64>,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Ev {
        CreateOwner,
        Open,
        Empty,
        Set(u32),
        SeqOpen(u64),
        Close,
        SeqNow(u64),
        OwnerCheck(bool),
        DestroyOwner,
    }

    /// A fake owner window: logs its destruction.
    struct FakeOwner(Arc<Mutex<Vec<Ev>>>);

    impl Drop for FakeOwner {
        fn drop(&mut self) {
            self.0.lock().unwrap().push(Ev::DestroyOwner);
        }
    }

    /// A fake global block: counts itself as freed when dropped without being handed over.
    struct FakeBlock {
        bytes: Vec<u8>,
        handed: bool,
        freed: Arc<AtomicU32>,
    }

    impl Drop for FakeBlock {
        fn drop(&mut self) {
            if !self.handed {
                self.freed.fetch_add(1, AtomicOrdering::SeqCst);
            }
        }
    }

    impl FakeOs {
        fn new() -> Self {
            Self {
                clipboard: Vec::new(),
                atoms: RefCell::new(Vec::new()),
                dwords: Vec::new(),
                busy: false,
                enum_fails: false,
                seq: 42,
                opens: Cell::new(0),
                closes: Cell::new(0),
                registers: Cell::new(0),
                registered_names: RefCell::new(Vec::new()),
                data_reads: RefCell::new(Vec::new()),
                content: Vec::new(),
                sizes: Vec::new(),
                files: Vec::new(),
                size_calls: RefCell::new(Vec::new()),
                copy_calls: RefCell::new(Vec::new()),
                events: Arc::new(Mutex::new(Vec::new())),
                owner_fails: false,
                stolen_after_close: false,
                empty_fails: false,
                fail_set_at: None,
                allocs: Cell::new(0),
                freed: Arc::new(AtomicU32::new(0)),
                written: RefCell::new(Vec::new()),
                write_seq: Cell::new(1000),
            }
        }

        fn freed(&self) -> u32 {
            self.freed.load(AtomicOrdering::SeqCst)
        }

        fn push(&self, e: Ev) {
            self.events.lock().unwrap().push(e);
        }

        fn ev(&self) -> Vec<Ev> {
            self.events.lock().unwrap().clone()
        }

        fn count(&self, e: Ev) -> usize {
            self.ev().iter().filter(|x| **x == e).count()
        }

        fn sets(&self) -> Vec<u32> {
            self.ev()
                .iter()
                .filter_map(|e| match e {
                    Ev::Set(id) => Some(*id),
                    _ => None,
                })
                .collect()
        }

        /// A standard format with content.
        fn std_data(&mut self, id: u32, bytes: Vec<u8>) -> &mut Self {
            self.standard(id);
            self.content.push((id, bytes));
            self
        }

        /// A registered format with content.
        fn reg_data(&mut self, name: &'static str, bytes: Vec<u8>) -> &mut Self {
            self.registered(name);
            let id = self.atom(name);
            self.content.push((id, bytes));
            self
        }

        /// The data of `id`: `CF_HDROP` is a DROPFILES block built from `files`.
        fn content_of(&self, id: u32) -> Option<Vec<u8>> {
            if id == CF_HDROP && !self.files.is_empty() {
                return Some(crate::text::tests::dropfiles(&self.files, true));
            }
            self.content
                .iter()
                .find(|(i, _)| *i == id)
                .map(|(_, b)| b.clone())
        }

        /// Any data fetch at all (sizes, copies, file names, DWORDs).
        fn fetched_nothing(&self) -> bool {
            self.size_calls.borrow().is_empty()
                && self.copy_calls.borrow().is_empty()
                && self.data_reads.borrow().is_empty()
        }

        fn atom(&self, name: &str) -> u32 {
            let mut atoms = self.atoms.borrow_mut();
            if let Some((_, id)) = atoms.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)) {
                return *id;
            }
            let id = 0xC100 + u32::try_from(atoms.len()).unwrap();
            atoms.push((name.to_owned(), id));
            id
        }

        fn standard(&mut self, id: u32) -> &mut Self {
            self.clipboard.push((id, None));
            self
        }

        fn registered(&mut self, name: &'static str) -> &mut Self {
            let id = self.atom(name);
            self.clipboard.push((id, Some(name)));
            self
        }

        fn dword(&mut self, name: &'static str, value: Option<u32>) -> &mut Self {
            self.registered(name);
            let id = self.atom(name);
            self.dwords.push((id, value));
            self
        }

        /// Darkshot's set as `darkshot-windows/src/clipboard.rs` writes it: the main format,
        /// then the three exclusion formats, each with DWORD 0.
        fn darkshot_text(&mut self) -> &mut Self {
            self.standard(CF_UNICODETEXT)
                .standard(CF_LOCALE)
                .standard(CF_TEXT)
                .standard(CF_OEMTEXT)
                .dword(EXCLUDE_MONITOR, Some(0))
                .dword(HISTORY, Some(0))
                .dword(CLOUD, Some(0))
        }
    }

    struct FakeOpen<'a>(&'a FakeOs);

    impl Drop for FakeOpen<'_> {
        fn drop(&mut self) {
            self.0.closes.set(self.0.closes.get() + 1);
        }
    }

    impl ClipboardOs for FakeOs {
        type Open<'a> = FakeOpen<'a>;

        fn register(&mut self, name: &str) -> Option<u32> {
            self.registers.set(self.registers.get() + 1);
            self.registered_names.borrow_mut().push(name.to_owned());
            Some(self.atom(name))
        }

        fn open(&mut self) -> Result<FakeOpen<'_>, PlatformError> {
            self.opens.set(self.opens.get() + 1);
            if self.busy {
                return Err(PlatformError::Busy);
            }
            Ok(FakeOpen(self))
        }
    }

    struct FakeWriter<'a>(&'a FakeOs);

    impl Drop for FakeWriter<'_> {
        fn drop(&mut self) {
            self.0.push(Ev::Close);
            self.0.write_seq.set(self.0.write_seq.get() + 4);
            if self.0.stolen_after_close {
                self.0.write_seq.set(self.0.write_seq.get() + 5);
            }
        }
    }

    impl ClipboardWriteOs for FakeOs {
        type Block = FakeBlock;
        type Owner = FakeOwner;
        type Writer<'a> = FakeWriter<'a>;

        fn alloc(&mut self, bytes: &[u8]) -> Result<FakeBlock, PlatformError> {
            self.allocs.set(self.allocs.get() + 1);
            Ok(FakeBlock {
                bytes: bytes.to_vec(),
                handed: false,
                freed: Arc::clone(&self.freed),
            })
        }

        fn create_owner(&mut self) -> Result<FakeOwner, PlatformError> {
            if self.owner_fails {
                return Err(PlatformError::Backend { code: 1407 });
            }
            self.push(Ev::CreateOwner);
            Ok(FakeOwner(Arc::clone(&self.events)))
        }

        fn open_for_write(&mut self, _owner: &FakeOwner) -> Result<FakeWriter<'_>, PlatformError> {
            self.opens.set(self.opens.get() + 1);
            if self.busy {
                return Err(PlatformError::Busy);
            }
            self.push(Ev::Open);
            Ok(FakeWriter(self))
        }

        fn sequence_now(&self) -> u64 {
            let seq = self.write_seq.get();
            self.push(Ev::SeqNow(seq));
            seq
        }

        fn owns_clipboard(&self, _owner: &FakeOwner) -> bool {
            let ours = !self.stolen_after_close;
            self.push(Ev::OwnerCheck(ours));
            ours
        }
    }

    impl ClipboardWriter<FakeBlock> for FakeWriter<'_> {
        fn empty(&mut self) -> Result<(), PlatformError> {
            self.0.push(Ev::Empty);
            if self.0.empty_fails {
                return Err(PlatformError::Backend { code: 1418 });
            }
            Ok(())
        }

        fn set(&mut self, id: u32, mut block: FakeBlock) -> Result<(), PlatformError> {
            let n = self.0.sets().len();
            self.0.push(Ev::Set(id));
            if self.0.fail_set_at == Some(n) {
                return Err(PlatformError::Busy); // any failure; mapped to Backend
            }
            block.handed = true;
            self.0
                .written
                .borrow_mut()
                .push((id, std::mem::take(&mut block.bytes)));
            self.0.write_seq.set(self.0.write_seq.get() + 1);
            Ok(())
        }

        fn sequence_while_open(&self) -> u64 {
            let seq = self.0.write_seq.get();
            self.0.push(Ev::SeqOpen(seq));
            seq
        }
    }

    impl OpenClipboardView for FakeOpen<'_> {
        fn sequence(&self) -> u64 {
            self.0.seq
        }

        fn enum_formats(&self) -> Result<Vec<u32>, PlatformError> {
            if self.0.enum_fails {
                return Err(PlatformError::Backend { code: 5 });
            }
            Ok(self.0.clipboard.iter().map(|(id, _)| *id).collect())
        }

        fn format_name(&self, id: u32) -> Option<String> {
            self.0
                .clipboard
                .iter()
                .find(|(i, _)| *i == id)
                .and_then(|(_, n)| n.map(str::to_owned))
        }

        fn dword_value(&self, id: u32) -> Option<u32> {
            self.0.data_reads.borrow_mut().push(id);
            self.0
                .dwords
                .iter()
                .find(|(i, _)| *i == id)
                .and_then(|(_, v)| *v)
        }

        fn owner_source(&self) -> Option<ClipboardSource> {
            Some(ClipboardSource {
                app_id: "darkshot.exe".to_owned(),
                app_name: None,
                platform: Platform::Windows,
                confidence: SourceConfidence::Owner,
            })
        }

        fn data_size(&self, id: u32) -> Option<u64> {
            self.0.size_calls.borrow_mut().push(id);
            if let Some((_, s)) = self.0.sizes.iter().find(|(i, _)| *i == id) {
                return Some(*s);
            }
            self.0
                .content_of(id)
                .map(|b| u64::try_from(b.len()).unwrap())
        }

        fn copy_data(&self, id: u32, max: usize) -> Option<Zeroizing<Vec<u8>>> {
            self.0.copy_calls.borrow_mut().push((id, max));
            self.0
                .content_of(id)
                .map(|b| Zeroizing::new(b[..b.len().min(max)].to_vec()))
        }
    }

    fn plan(formats: &[(Mime, &str)], text_cap: u64, image_cap: u64) -> ReadPlan {
        ReadPlan {
            formats: formats
                .iter()
                .map(|&(mime, name)| AvailableFormat {
                    mime,
                    platform_format: name.to_owned(),
                    size_hint: None,
                })
                .collect(),
            truncate_text_at: text_cap,
            image_max_bytes: image_cap,
        }
    }

    fn utf16z(s: &str) -> Vec<u8> {
        s.encode_utf16()
            .chain(std::iter::once(0))
            .flat_map(u16::to_le_bytes)
            .collect()
    }

    fn png_1x1() -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut out, 1, 1);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            let mut w = enc.write_header().unwrap();
            w.write_image_data(&[1, 2, 3, 255]).unwrap();
        }
        out
    }

    /// A 2x1 bottom-up 24-bit DIB (40-byte header).
    fn dib_2x1() -> Vec<u8> {
        let mut d = Vec::new();
        for v in [40u32, 2, 1] {
            d.extend_from_slice(&v.to_le_bytes());
        }
        d.extend_from_slice(&1u16.to_le_bytes());
        d.extend_from_slice(&24u16.to_le_bytes());
        d.extend_from_slice(&[0u8; 24]);
        d.extend_from_slice(&[0, 0, 255, 0, 255, 0, 0, 0]);
        d
    }

    /// Every content format this adapter reads, plus the ANSI text twins.
    fn full_clipboard() -> FakeOs {
        let mut os = FakeOs::new();
        os.std_data(CF_UNICODETEXT, utf16z("plain"))
            .std_data(CF_TEXT, b"plain\0".to_vec())
            .reg_data("HTML Format", b"Version:0.9\r\n<b>x</b>\0".to_vec())
            .reg_data("Rich Text Format", b"{\\rtf1 x}\0".to_vec())
            .reg_data("PNG", png_1x1())
            .std_data(CF_DIBV5, dib_2x1())
            .std_data(CF_DIB, dib_2x1())
            .standard(CF_HDROP);
        os.files = vec![r"C:\a b.txt"];
        os
    }

    fn read_with(os: &mut FakeOs, p: &ReadPlan) -> Result<Vec<RawRepresentation>, PlatformError> {
        read_plan(os, p)
    }

    #[test]
    fn reads_only_planned_formats() {
        let mut os = full_clipboard();
        let p = plan(
            &[(Mime::TextPlain, "CF_UNICODETEXT"), (Mime::ImagePng, "PNG")],
            1000,
            1 << 20,
        );
        let reps = read_with(&mut os, &p).unwrap();
        let got: Vec<(Mime, &str)> = reps
            .iter()
            .map(|r| (r.mime, r.platform_format.as_str()))
            .collect();
        assert_eq!(
            got,
            vec![(Mime::TextPlain, "CF_UNICODETEXT"), (Mime::ImagePng, "PNG")]
        );
        assert_eq!(reps[0].bytes, b"plain");
        assert_eq!(reps[1].bytes, png_1x1());
        assert_eq!((reps[1].width, reps[1].height), (Some(1), Some(1)));
        let png_id = os.atom("PNG");
        let mut sized = os.size_calls.borrow().clone();
        sized.sort_unstable();
        let mut want = vec![CF_UNICODETEXT, png_id];
        want.sort_unstable();
        assert_eq!(sized, want);
        let copied: Vec<u32> = os.copy_calls.borrow().iter().map(|c| c.0).collect();
        assert_eq!(copied.len(), 2);
        assert!(copied.iter().all(|id| want.contains(id)));
        assert!(!os.size_calls.borrow().contains(&CF_HDROP));
        assert!(os.data_reads.borrow().is_empty(), "no marker DWORDs");
        assert_eq!((os.opens.get(), os.closes.get()), (1, 1));

        // A plan naming a format this adapter does not read (or with a MIME that does not
        // match the name) reads nothing for it.
        let mut os = full_clipboard();
        let p = plan(
            &[
                (Mime::TextPlain, "CF_TEXT"),
                (Mime::TextHtml, "CF_UNICODETEXT"),
                (Mime::TextPlain, "DataObject"),
            ],
            1000,
            1 << 20,
        );
        assert!(read_with(&mut os, &p).unwrap().is_empty());
        assert!(os.fetched_nothing());
    }

    #[test]
    fn size_checked_before_copy() {
        for name in ["PNG", "CF_DIBV5", "CF_DIB"] {
            let mut os = full_clipboard();
            let id = if name == "PNG" {
                os.atom("PNG")
            } else if name == "CF_DIBV5" {
                CF_DIBV5
            } else {
                CF_DIB
            };
            os.sizes.push((id, 1 << 30));
            let p = plan(
                &[(Mime::TextPlain, "CF_UNICODETEXT"), (Mime::ImagePng, name)],
                1000,
                10 << 20,
            );
            assert_eq!(
                read_with(&mut os, &p).unwrap_err(),
                PlatformError::TooLarge {
                    format: name.to_owned(),
                    bytes: 1 << 30
                }
            );
            assert!(
                !os.copy_calls.borrow().iter().any(|c| c.0 == id),
                "{name}: no data copy of the oversize image"
            );
            assert_eq!(os.opens.get(), os.closes.get(), "{name}: closed");
        }
        // A DIB whose header decodes past the cap is TooLarge as well.
        let mut os = full_clipboard();
        // Source (54 bytes) and RGBA (8 bytes) fit in 60; the encoded PNG does not.
        let p = plan(&[(Mime::ImagePng, "CF_DIB")], 1000, 60);
        assert!(matches!(
            read_with(&mut os, &p).unwrap_err(),
            PlatformError::TooLarge { .. }
        ));
    }

    #[test]
    fn busy_once() {
        let mut os = full_clipboard();
        os.busy = true;
        let p = plan(&[(Mime::TextPlain, "CF_UNICODETEXT")], 1000, 1 << 20);
        assert_eq!(read_with(&mut os, &p).unwrap_err(), PlatformError::Busy);
        assert_eq!(os.opens.get(), 1, "one OpenClipboard attempt, no retry");
        assert_eq!(os.closes.get(), 0);
        assert!(os.fetched_nothing());
    }

    #[test]
    fn text_read_is_bounded_and_truncated() {
        let mut os = FakeOs::new();
        os.std_data(CF_UNICODETEXT, utf16z(&"Việt ".repeat(1000)));
        let p = plan(&[(Mime::TextPlain, "CF_UNICODETEXT")], 10, 1 << 20);
        let reps = read_with(&mut os, &p).unwrap();
        assert_eq!(os.copy_calls.borrow().as_slice(), &[(CF_UNICODETEXT, 42)]);
        assert_eq!(std::str::from_utf8(&reps[0].bytes).unwrap(), "Việt Vi");
        assert!(reps[0].truncated);
        // Short text: the copy is bounded by GlobalSize, not truncated.
        let mut os = FakeOs::new();
        os.std_data(CF_UNICODETEXT, utf16z("hi"));
        let reps = read_with(
            &mut os,
            &plan(&[(Mime::TextPlain, "CF_UNICODETEXT")], 10, 0),
        )
        .unwrap();
        assert_eq!(os.copy_calls.borrow().as_slice(), &[(CF_UNICODETEXT, 6)]);
        assert_eq!(reps[0].bytes, b"hi");
        assert!(!reps[0].truncated);
    }

    #[test]
    fn html_rtf_raw_and_capped() {
        let mut os = full_clipboard();
        let p = plan(
            &[
                (Mime::TextHtml, "HTML Format"),
                (Mime::TextRtf, "Rich Text Format"),
            ],
            1000,
            1 << 20,
        );
        let reps = read_with(&mut os, &p).unwrap();
        assert_eq!(reps[0].platform_format, "HTML Format");
        assert_eq!(reps[0].bytes, b"Version:0.9\r\n<b>x</b>");
        assert_eq!(reps[1].bytes, b"{\\rtf1 x}");
        assert!(
            reps.iter().all(|r| r.mime != Mime::TextPlain),
            "no text from HTML"
        );
        let mut os = full_clipboard();
        let p = plan(&[(Mime::TextHtml, "HTML Format")], 7, 1 << 20);
        let reps = read_with(&mut os, &p).unwrap();
        assert_eq!(os.copy_calls.borrow()[0].1, 8);
        assert_eq!(reps[0].bytes, b"Version");
        assert!(reps[0].truncated);
    }

    #[test]
    fn dib_converted_and_invalid_png_skipped() {
        let mut os = full_clipboard();
        let reps = read_with(&mut os, &plan(&[(Mime::ImagePng, "CF_DIBV5")], 0, 1 << 20)).unwrap();
        assert_eq!(reps[0].platform_format, "CF_DIBV5");
        assert_eq!((reps[0].width, reps[0].height), (Some(2), Some(1)));
        assert!(reps[0].bytes.starts_with(b"\x89PNG\r\n\x1a\n"));

        let mut os = FakeOs::new();
        os.reg_data("PNG", b"not a png at all".to_vec());
        os.std_data(CF_UNICODETEXT, utf16z("t"));
        let p = plan(
            &[(Mime::TextPlain, "CF_UNICODETEXT"), (Mime::ImagePng, "PNG")],
            10,
            1 << 20,
        );
        let reps = read_with(&mut os, &p).unwrap();
        assert_eq!(reps.len(), 1, "the invalid PNG is skipped, the text kept");
        // An unsupported DIB is skipped too (not an error for the whole clip).
        let mut os = FakeOs::new();
        let mut d = dib_2x1();
        d[14] = 8;
        os.std_data(CF_DIB, d);
        assert!(
            read_with(&mut os, &plan(&[(Mime::ImagePng, "CF_DIB")], 10, 1 << 20))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn hdrop_to_uri_list() {
        let mut os = full_clipboard();
        os.files = vec![r"C:\Users\Tuấn\a b.txt", r"D:\x"];
        let reps = read_with(&mut os, &plan(&[(Mime::UriList, "CF_HDROP")], 1000, 0)).unwrap();
        assert_eq!(reps[0].platform_format, "CF_HDROP");
        assert_eq!(
            std::str::from_utf8(&reps[0].bytes).unwrap(),
            "file:///C:/Users/Tu%E1%BA%A5n/a%20b.txt\r\nfile:///D:/x"
        );
        assert!(!reps[0].truncated);
        // The DROPFILES block is copied (bounded) and parsed here, not by shell32.
        let block_len = crate::text::tests::dropfiles(&os.files, true).len();
        assert_eq!(
            os.copy_calls.borrow().as_slice(),
            &[(CF_HDROP, block_len.min(text::dropfiles_raw_limit(1000)))]
        );
        let mut os = full_clipboard();
        os.files = vec![r"C:\a", r"C:\b"];
        let reps = read_with(&mut os, &plan(&[(Mime::UriList, "CF_HDROP")], 15, 0)).unwrap();
        assert_eq!(reps[0].bytes, b"file:///C:/a");
        assert!(reps[0].truncated);
    }

    #[test]
    fn hdrop_malformed_block_skipped() {
        // pFiles past the block: the representation is skipped, the text kept.
        let mut block = crate::text::tests::dropfiles(&[r"C:\a"], true);
        block[0..4].copy_from_slice(&5000u32.to_le_bytes());
        let mut os = FakeOs::new();
        os.std_data(CF_HDROP, block)
            .std_data(CF_UNICODETEXT, utf16z("t"));
        let p = plan(
            &[
                (Mime::TextPlain, "CF_UNICODETEXT"),
                (Mime::UriList, "CF_HDROP"),
            ],
            1000,
            0,
        );
        let reps = read_with(&mut os, &p).unwrap();
        assert_eq!(reps.len(), 1);
        assert_eq!(reps[0].mime, Mime::TextPlain);
        // No terminator in a complete block: skipped as well.
        let block = crate::text::tests::dropfiles(&[r"C:\a"], true);
        let mut os = FakeOs::new();
        os.std_data(CF_HDROP, block[..block.len() - 2].to_vec());
        let reps = read_with(&mut os, &plan(&[(Mime::UriList, "CF_HDROP")], 1000, 0)).unwrap();
        assert!(reps.is_empty());
    }

    #[test]
    fn empty_plan_does_not_open() {
        let mut os = full_clipboard();
        assert!(read_with(&mut os, &plan(&[], 10, 10)).unwrap().is_empty());
        assert_eq!(os.opens.get(), 0);
        let mut a = WinAccess::new();
        assert!(a.read(&plan(&[], 10, 10)).unwrap().is_empty());
    }

    fn snap(os: &mut FakeOs) -> Result<ClipboardMeta, PlatformError> {
        snapshot(os, &mut MarkerCache::default())
    }

    #[test]
    fn snapshot_never_reads_content() {
        let mut os = FakeOs::new();
        os.darkshot_text().registered("PNG");
        let meta = snap(&mut os).unwrap();
        let history = os.atom(HISTORY);
        let cloud = os.atom(CLOUD);
        let mut reads = os.data_reads.borrow().clone();
        reads.sort_unstable();
        let mut want = vec![history, cloud];
        want.sort_unstable();
        assert_eq!(reads, want, "only the two marker DWORDs are fetched");
        assert_eq!(os.opens.get(), 1);
        assert_eq!(os.closes.get(), 1, "the clipboard is closed");
        assert_eq!(meta.seq, 42);
        assert_eq!(
            meta.formats
                .iter()
                .filter_map(|f| f.mime)
                .collect::<Vec<_>>(),
            vec![Mime::TextPlain, Mime::ImagePng]
        );

        // Plain text and PNG with no markers: no data read at all.
        let mut os = FakeOs::new();
        os.standard(CF_UNICODETEXT).registered("PNG");
        snap(&mut os).unwrap();
        assert!(os.data_reads.borrow().is_empty());
    }

    #[test]
    fn busy_returns_busy_once() {
        let mut os = FakeOs::new();
        os.darkshot_text();
        os.busy = true;
        assert_eq!(snap(&mut os).unwrap_err(), PlatformError::Busy);
        assert_eq!(os.opens.get(), 1, "one OpenClipboard attempt, no retry");
        assert_eq!(os.closes.get(), 0);
        assert!(os.data_reads.borrow().is_empty());
    }

    #[test]
    fn darkshot_set_reports_darkshot_marker() {
        let mut os = FakeOs::new();
        os.darkshot_text();
        let meta = snap(&mut os).unwrap();
        assert!(
            meta.markers.contains(&PrivacyMarker::DarkshotExcluded),
            "{:?}",
            meta.markers
        );
        assert_eq!(meta.source.unwrap().app_id, "darkshot.exe");
        // Marker formats are listed with canonical names and no MIME.
        for name in [EXCLUDE_MONITOR, HISTORY, CLOUD] {
            let f = meta.formats.iter().find(|f| f.name == name).unwrap();
            assert_eq!(f.mime, None);
        }
    }

    #[test]
    fn exclude_format_any_data_reports_concealed() {
        // No data on the exclude format at all (and never fetched).
        let mut os = FakeOs::new();
        os.standard(CF_UNICODETEXT).registered(EXCLUDE_MONITOR);
        let meta = snap(&mut os).unwrap();
        assert_eq!(
            meta.markers,
            vec![PrivacyMarker::OsConcealed {
                name: EXCLUDE_MONITOR
            }]
        );
        assert!(os.data_reads.borrow().is_empty());

        // Any data, e.g. a DWORD of 1: still concealed, and not read.
        let mut os = FakeOs::new();
        os.standard(CF_UNICODETEXT).dword(EXCLUDE_MONITOR, Some(1));
        let meta = snap(&mut os).unwrap();
        assert!(meta
            .markers
            .iter()
            .any(darkclip_core::markers::is_mandatory));
        assert!(os.data_reads.borrow().is_empty());
    }

    #[test]
    fn history_dword_zero_concealed_and_one_not() {
        let mut os = FakeOs::new();
        os.standard(CF_UNICODETEXT).dword(HISTORY, Some(0));
        let meta = snap(&mut os).unwrap();
        assert_eq!(
            meta.markers,
            vec![PrivacyMarker::OsConcealed { name: HISTORY }]
        );

        let mut os = FakeOs::new();
        os.standard(CF_UNICODETEXT).dword(HISTORY, Some(1));
        let meta = snap(&mut os).unwrap();
        assert!(meta.markers.is_empty(), "{:?}", meta.markers);

        let mut os = FakeOs::new();
        os.standard(CF_UNICODETEXT).dword(CLOUD, Some(0));
        assert_eq!(snap(&mut os).unwrap().markers, vec![PrivacyMarker::NoCloud]);
    }

    #[test]
    fn unreadable_dword_reported_none() {
        // The adapter reports `None`; core fails closed (history concealed, cloud NoCloud).
        let mut os = FakeOs::new();
        os.standard(CF_UNICODETEXT)
            .dword(HISTORY, None)
            .dword(CLOUD, None);
        let meta = snap(&mut os).unwrap();
        assert_eq!(
            meta.markers,
            windows_markers(&[(HISTORY, None), (CLOUD, None)])
        );
        assert!(meta
            .markers
            .contains(&PrivacyMarker::OsConcealed { name: HISTORY }));
        assert!(meta.markers.contains(&PrivacyMarker::NoCloud));
    }

    #[test]
    fn lookalike_marker_name_under_other_id_is_not_a_marker() {
        // A name differing only by a trailing space is a different atom (another id).
        let mut os = FakeOs::new();
        os.standard(CF_UNICODETEXT)
            .registered("CanIncludeInClipboardHistory ");
        let meta = snap(&mut os).unwrap();
        assert!(meta.markers.is_empty(), "{:?}", meta.markers);
        assert!(os.data_reads.borrow().is_empty());
    }

    #[test]
    fn ole_placeholder_phase_has_no_mime() {
        let mut os = FakeOs::new();
        os.registered("DataObject").registered("Ole Private Data");
        let meta = snap(&mut os).unwrap();
        assert_eq!(meta.formats.len(), 2);
        assert!(meta.formats.iter().all(|f| f.mime.is_none()));
        assert!(meta.markers.is_empty());
    }

    #[test]
    fn enumeration_error_is_an_error_and_closes() {
        let mut os = FakeOs::new();
        os.darkshot_text();
        os.enum_fails = true;
        assert!(matches!(
            snap(&mut os).unwrap_err(),
            PlatformError::Backend { .. }
        ));
        assert_eq!(os.closes.get(), os.opens.get());
        assert!(os.data_reads.borrow().is_empty());
    }

    #[test]
    fn marker_ids_registered_once_and_before_open() {
        let mut os = FakeOs::new();
        os.darkshot_text();
        let mut cache = MarkerCache::default();
        snapshot(&mut os, &mut cache).unwrap();
        let first = os.registers.get();
        assert_eq!(first, u32::try_from(formats::MARKER_NAMES.len()).unwrap());
        snapshot(&mut os, &mut cache).unwrap();
        assert_eq!(os.registers.get(), first, "ids are cached");
        // Registration still happens when the clipboard is busy (it needs no open clipboard).
        let mut os = FakeOs::new();
        os.busy = true;
        let _ = snap(&mut os);
        assert_eq!(os.registers.get(), first);
    }

    #[test]
    fn registers_exactly_core_windows_marker_names() {
        let mut os = FakeOs::new();
        os.standard(CF_UNICODETEXT);
        snap(&mut os).unwrap();
        let names = os.registered_names.borrow().clone();
        assert_eq!(names, darkclip_core::markers::WINDOWS_MARKER_NAMES);
    }

    #[test]
    fn unnamed_private_format_gets_a_placeholder_name() {
        let mut os = FakeOs::new();
        os.standard(0x0200).standard(CF_UNICODETEXT);
        let meta = snap(&mut os).unwrap();
        let f = meta.formats.first().unwrap();
        assert_eq!(f.mime, None);
        assert!(!f.name.is_empty());
    }

    #[test]
    fn sequence_is_ok() {
        let a = WinAccess::new();
        let first = a.sequence().unwrap();
        // Never fails, and is stable or increasing (another app may copy meanwhile).
        let second = a.sequence().unwrap();
        assert!(second >= first);
    }

    #[test]
    fn snapshot_meta_on_current_clipboard_does_not_fail() {
        // Read-only: names, markers and the two marker DWORDs only.
        let mut a = WinAccess::new();
        match a.snapshot_meta() {
            Ok(meta) => {
                assert!(meta.seq > 0 || meta.formats.is_empty());
                let mimes: Vec<Mime> = meta.formats.iter().filter_map(|f| f.mime).collect();
                for m in &mimes {
                    assert_eq!(mimes.iter().filter(|x| *x == m).count(), 1, "S-1");
                }
            }
            Err(e) => assert_eq!(e, PlatformError::Busy),
        }
    }

    #[test]
    #[ignore = "writes the real clipboard"]
    fn real_clipboard_write_darkshot_set_detected() {
        let seq = (0..10)
            .find_map(|_| {
                let r = win::test_support::write_darkshot_text_set("darkclip test 102");
                if r.is_err() {
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
                r.ok()
            })
            .expect("clipboard stayed busy");
        let mut a = WinAccess::new();
        let meta = (0..10)
            .find_map(|_| match a.snapshot_meta() {
                Ok(m) => Some(m),
                Err(_) => {
                    std::thread::sleep(std::time::Duration::from_millis(20));
                    None
                }
            })
            .expect("clipboard stayed busy");
        assert_eq!(meta.seq, u64::from(seq));
        assert!(
            meta.markers.contains(&PrivacyMarker::DarkshotExcluded),
            "{:?}",
            meta.markers
        );
    }

    /// Retries `f` while the real clipboard is busy (test helper only).
    fn retry<T>(mut f: impl FnMut() -> Result<T, PlatformError>) -> T {
        for _ in 0..20 {
            match f() {
                Ok(v) => return v,
                Err(PlatformError::Busy) => {
                    std::thread::sleep(std::time::Duration::from_millis(20))
                }
                Err(e) => panic!("{e}"),
            }
        }
        panic!("clipboard stayed busy");
    }

    #[test]
    #[ignore = "writes the real clipboard"]
    fn real_clipboard_write_text_roundtrip_vietnamese() {
        let text = "Tiếng Việt: đường, phở, nước mắm 😀";
        retry(|| win::test_support::write_text(text));
        let mut a = WinAccess::new();
        let p = plan(&[(Mime::TextPlain, "CF_UNICODETEXT")], 1 << 20, 0);
        let reps = retry(|| a.read(&p));
        assert_eq!(reps.len(), 1);
        assert_eq!(std::str::from_utf8(&reps[0].bytes).unwrap(), text);
        assert!(!reps[0].truncated);
        // Capped read on a char boundary.
        let p = plan(&[(Mime::TextPlain, "CF_UNICODETEXT")], 6, 0);
        let reps = retry(|| a.read(&p));
        // "Tiế" is 5 bytes; the next char "n" fits in the 6-byte cap.
        assert_eq!(std::str::from_utf8(&reps[0].bytes).unwrap(), "Tiến");
        assert!(reps[0].truncated);
    }

    #[test]
    #[ignore = "writes the real clipboard"]
    fn real_clipboard_write_dib_reads_synthesized_dibv5() {
        // A 2x1 32-bit BI_BITFIELDS CF_DIB; Windows synthesizes CF_DIBV5 from it (the layout
        // where masks may follow the V5 header). Pixels: red, green.
        let mut d = Vec::new();
        for v in [40u32, 2, 1] {
            d.extend_from_slice(&v.to_le_bytes());
        }
        d.extend_from_slice(&1u16.to_le_bytes());
        d.extend_from_slice(&32u16.to_le_bytes());
        d.extend_from_slice(&3u32.to_le_bytes()); // BI_BITFIELDS
        d.extend_from_slice(&[0u8; 20]);
        for m in [0x00FF_0000u32, 0x0000_FF00, 0x0000_00FF] {
            d.extend_from_slice(&m.to_le_bytes());
        }
        d.extend_from_slice(&[0, 0, 255, 0, 0, 255, 0, 0]);
        retry(|| win::test_support::write_dib(&d));
        let mut a = WinAccess::new();
        for name in ["CF_DIBV5", "CF_DIB"] {
            let p = plan(&[(Mime::ImagePng, name)], 0, 1 << 20);
            let reps = retry(|| a.read(&p));
            assert_eq!(reps.len(), 1, "{name}");
            assert_eq!(
                (reps[0].width, reps[0].height),
                (Some(2), Some(1)),
                "{name}"
            );
            let mut dec = png::Decoder::new(std::io::Cursor::new(&reps[0].bytes[..]));
            dec.set_transformations(png::Transformations::EXPAND);
            let mut r = dec.read_info().unwrap();
            let mut buf = vec![0; r.output_buffer_size().unwrap()];
            r.next_frame(&mut buf).unwrap();
            assert_eq!(buf[..8], [255, 0, 0, 255, 0, 255, 0, 255], "{name}");
        }
    }

    #[test]
    #[ignore = "writes the real clipboard"]
    fn real_clipboard_write_html_non_ascii() {
        let fragment = "<b>Xin chào thế giới</b>";
        let html = format!(
            "Version:0.9\r\nStartHTML:-1\r\nEndHTML:-1\r\nStartFragment:0\r\nEndFragment:{}\r\n{fragment}",
            fragment.len()
        );
        retry(|| win::test_support::write_html(&html));
        let mut a = WinAccess::new();
        let p = plan(&[(Mime::TextHtml, "HTML Format")], 1 << 20, 0);
        let reps = retry(|| a.read(&p));
        assert_eq!(reps.len(), 1);
        assert_eq!(reps[0].platform_format, "HTML Format");
        assert_eq!(std::str::from_utf8(&reps[0].bytes).unwrap(), html);
    }

    #[test]
    #[ignore = "writes the real clipboard"]
    fn real_clipboard_write_powershell_ole_phases() {
        // Spike S5: a .NET writer produces a placeholder-only phase, then real formats. Every
        // snapshot is either placeholder-only (no MIME at all) or offers plain text once.
        let (tx, rx) = std::sync::mpsc::channel::<Result<ClipboardMeta, PlatformError>>();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel::<win::RawWindow>();
        let handle = std::thread::spawn(move || {
            let mut a = WinAccess::new();
            let on_update: win::UpdateHandler = Box::new(move || {
                let _ = tx.send(a.snapshot_meta());
                true
            });
            let w = win::MessageWindow::create(Some(on_update)).unwrap();
            ready_tx.send(w.raw()).unwrap();
            w.run();
        });
        let raw = ready_rx.recv().unwrap();
        let status = std::process::Command::new("powershell")
            .args(["-NoProfile", "-Command", "Set-Clipboard -Value x"])
            .status()
            .unwrap();
        assert!(status.success());
        std::thread::sleep(std::time::Duration::from_millis(500));
        assert!(raw.post_stop());
        handle.join().unwrap();
        let snaps: Vec<ClipboardMeta> = rx.try_iter().filter_map(Result::ok).collect();
        assert!(!snaps.is_empty());
        let mut saw_text = false;
        for s in &snaps {
            let mimes: Vec<Mime> = s.formats.iter().filter_map(|f| f.mime).collect();
            if mimes.contains(&Mime::TextPlain) {
                saw_text = true;
            }
            for m in &mimes {
                assert_eq!(mimes.iter().filter(|x| *x == m).count(), 1, "S-1");
            }
        }
        assert!(saw_text);
    }

    /// Task 104: `write` over the fake seam.
    mod write {
        use super::*;

        fn raw(mime: Mime, name: &str, bytes: Vec<u8>) -> RawRepresentation {
            RawRepresentation {
                mime,
                platform_format: name.to_owned(),
                bytes,
                width: None,
                height: None,
                truncated: false,
            }
        }

        fn write_with(os: &mut FakeOs, reps: &[RawRepresentation]) -> Result<u64, PlatformError> {
            write_reps(os, reps)
        }

        /// The owner window was created once and destroyed once, after the clipboard closed
        /// (if it was opened).
        fn assert_owner_destroyed(os: &FakeOs) {
            let ev = os.ev();
            assert_eq!(os.count(Ev::CreateOwner), 1, "{ev:?}");
            assert_eq!(os.count(Ev::DestroyOwner), 1, "{ev:?}");
            assert_eq!(*ev.last().unwrap(), Ev::DestroyOwner, "{ev:?}");
        }

        /// One representation of every kind the adapter writes.
        fn all_reps() -> Vec<RawRepresentation> {
            vec![
                raw(
                    Mime::TextPlain,
                    "CF_UNICODETEXT",
                    "Xin chào".as_bytes().to_vec(),
                ),
                raw(
                    Mime::TextHtml,
                    "HTML Format",
                    b"Version:0.9\r\n<b>x</b>".to_vec(),
                ),
                raw(Mime::TextRtf, "Rich Text Format", b"{\\rtf1 x}".to_vec()),
                raw(Mime::ImagePng, "PNG", png_1x1()),
                raw(Mime::UriList, "CF_HDROP", b"file:///C:/a%20b.txt".to_vec()),
            ]
        }

        #[test]
        fn uri_list_to_dropfiles_layout() {
            let block =
                uri_list_to_dropfiles(b"file:///C:/a%20b.txt\r\nfile://srv/share/%C4%91.txt\r\n")
                    .unwrap();
            // DROPFILES: pFiles = 20, pt = (0, 0), fNC = 0, fWide = 1.
            assert_eq!(block[..4], 20u32.to_le_bytes());
            assert_eq!(block[4..16], [0u8; 12]);
            assert_eq!(block[16..20], 1u32.to_le_bytes());
            let expected: Vec<u8> = r"C:\a b.txt"
                .encode_utf16()
                .chain([0])
                .chain(r"\\srv\share\đ.txt".encode_utf16())
                .chain([0, 0]) // end of name, then the empty name (double NUL)
                .flat_map(u16::to_le_bytes)
                .collect();
            assert_eq!(block[20..], expected[..]);
            // Task 103's parser reads it back, and `file_uri` gives the same URIs.
            let files = text::parse_dropfiles(&block, true, 1 << 20).unwrap();
            assert!(!files.more);
            let (list, cut) = text::uri_list(&files.names, 1 << 20);
            assert!(!cut);
            assert_eq!(list, "file:///C:/a%20b.txt\r\nfile://srv/share/%C4%91.txt");
            // localhost, comments and a bare drive root.
            let block = uri_list_to_dropfiles(b"# comment\nfile://localhost/D:/x\n").unwrap();
            let files = text::parse_dropfiles(&block, true, 1 << 20).unwrap();
            assert_eq!(files.names.len(), 1);
            assert_eq!(String::from_utf16(&files.names[0]).unwrap(), r"D:\x");
        }

        #[test]
        fn non_file_uris_skipped() {
            for list in [
                &b"https://example.com/a"[..],
                b"file:///C:/a.txt\r\nhttps://example.com/b",
                b"file:///home/user/a.txt", // not a Windows path
                b"file:///C:/a%2",          // bad escape
                b"file:///C:/a%00b",        // embedded NUL
                b"file:///C:/a.txt?x=1",
                b"mailto:a@example.com",
                b"# only a comment",
                b"",
            ] {
                assert!(uri_list_to_dropfiles(list).is_none(), "{list:?}");
            }
            // Through `write`: the URI list is skipped, the other representation is written.
            let mut os = FakeOs::new();
            let reps = [
                raw(Mime::UriList, "CF_HDROP", b"https://example.com/a".to_vec()),
                raw(Mime::TextPlain, "CF_UNICODETEXT", b"x".to_vec()),
            ];
            write_with(&mut os, &reps).unwrap();
            assert_eq!(os.sets(), vec![CF_UNICODETEXT]);
            // Nothing writable at all: an error, and the clipboard is never opened.
            let mut os = FakeOs::new();
            let reps = [raw(
                Mime::UriList,
                "CF_HDROP",
                b"https://example.com/a".to_vec(),
            )];
            assert!(matches!(
                write_with(&mut os, &reps),
                Err(PlatformError::Unsupported(_))
            ));
            assert_eq!(os.opens.get(), 0);
        }

        #[test]
        fn writes_png_dibv5_dib_for_image() {
            let mut os = FakeOs::new();
            let png = png_1x1();
            write_with(&mut os, &[raw(Mime::ImagePng, "PNG", png.clone())]).unwrap();
            let png_id = os.atom("PNG");
            assert_eq!(os.sets(), vec![png_id, CF_DIBV5, CF_DIB]);
            let written = os.written.borrow();
            assert_eq!(written[0].1, png, "PNG bytes as stored");
            // CF_DIBV5: V5 header, 32-bit BI_BITFIELDS, alpha mask; pixel (1, 2, 3, 255) as BGRA.
            let v5 = &written[1].1;
            assert_eq!(v5[..4], 124u32.to_le_bytes());
            assert_eq!(v5[14..16], 32u16.to_le_bytes());
            assert_eq!(v5[16..20], 3u32.to_le_bytes());
            assert_eq!(v5[52..56], 0xFF00_0000u32.to_le_bytes());
            assert_eq!(v5[124..], [3, 2, 1, 255]);
            // CF_DIB: 40-byte header, 32-bit BI_RGB.
            let d = &written[2].1;
            assert_eq!(d[..4], 40u32.to_le_bytes());
            assert_eq!(d[14..16], 32u16.to_le_bytes());
            assert_eq!(d[16..20], 0u32.to_le_bytes());
            assert_eq!(d[40..], [3, 2, 1, 255]);
            // Each block handed over exactly once; none freed by us.
            assert_eq!(os.allocs.get(), 3);
            assert_eq!(os.freed(), 0);
        }

        #[test]
        fn writes_text_html_rtf_and_files() {
            let mut os = FakeOs::new();
            write_with(&mut os, &all_reps()).unwrap();
            let html = os.atom("HTML Format");
            let rtf = os.atom("Rich Text Format");
            let png = os.atom("PNG");
            assert_eq!(
                os.sets(),
                vec![CF_UNICODETEXT, html, rtf, png, CF_DIBV5, CF_DIB, CF_HDROP]
            );
            let written = os.written.borrow();
            assert_eq!(written[0].1, utf16z("Xin chào"));
            assert_eq!(written[1].1, b"Version:0.9\r\n<b>x</b>\0");
            assert_eq!(written[2].1, b"{\\rtf1 x}\0");
            let files = text::parse_dropfiles(&written[6].1, true, 1 << 20).unwrap();
            assert_eq!(String::from_utf16(&files.names[0]).unwrap(), r"C:\a b.txt");
            // Order: owner, open, empty, sets, close, sequence, owner check, owner destroyed.
            let ev = os.ev();
            assert_eq!(ev[..3], [Ev::CreateOwner, Ev::Open, Ev::Empty]);
            let n = ev.len();
            assert!(matches!(ev[n - 5], Ev::SeqOpen(_)), "{ev:?}");
            assert_eq!(ev[n - 4], Ev::Close);
            assert!(matches!(ev[n - 3], Ev::SeqNow(_)));
            assert_eq!(ev[n - 2], Ev::OwnerCheck(true));
            assert_eq!(ev[n - 1], Ev::DestroyOwner);
        }

        #[test]
        fn no_marker_formats_written() {
            let mut os = FakeOs::new();
            write_with(&mut os, &all_reps()).unwrap();
            // No marker name was ever registered, so no marker id can have been written.
            let names = os.registered_names.borrow();
            for marker in formats::MARKER_NAMES {
                assert!(
                    !names.iter().any(|n| n.eq_ignore_ascii_case(marker)),
                    "{marker} registered"
                );
            }
            let marker_ids: Vec<u32> = formats::MARKER_NAMES.iter().map(|m| os.atom(m)).collect();
            for id in os.sets() {
                assert!(!marker_ids.contains(&id), "marker id {id:#x} written");
            }
            // Only the expected formats, each once.
            let mut sets = os.sets();
            let n = sets.len();
            sets.sort_unstable();
            sets.dedup();
            assert_eq!(sets.len(), n);
            assert_eq!(n, 7);
        }

        #[test]
        fn returns_seq_read_after_close_while_owner() {
            // D-038: CloseClipboard bumps the number (synthesized formats); the monitor's
            // WM_CLIPBOARDUPDATE carries the post-close value, so that is the one returned.
            let mut os = FakeOs::new();
            let seq = write_with(
                &mut os,
                &[raw(Mime::TextPlain, "CF_UNICODETEXT", b"x".to_vec())],
            )
            .unwrap();
            let ev = os.ev();
            let close_at = ev.iter().position(|e| *e == Ev::Close).unwrap();
            let now_at = ev.iter().position(|e| matches!(e, Ev::SeqNow(_))).unwrap();
            let check_at = ev
                .iter()
                .position(|e| matches!(e, Ev::OwnerCheck(_)))
                .unwrap();
            let destroy_at = ev.iter().position(|e| *e == Ev::DestroyOwner).unwrap();
            assert!(
                close_at < now_at && now_at < check_at && check_at < destroy_at,
                "{ev:?}"
            );
            assert_eq!(ev[now_at], Ev::SeqNow(seq));
            // 1000 + 1 set + 4 at close.
            assert_eq!(seq, 1005);
            assert_eq!(seq, os.write_seq.get());
        }

        #[test]
        fn returns_pre_close_seq_when_another_writer_got_in() {
            // Another app emptied the clipboard right after our close: the post-close number
            // may be theirs, so the pre-close value (which no signal carries) is returned and
            // their copy is never suppressed.
            let mut os = FakeOs::new();
            os.stolen_after_close = true;
            let seq = write_with(
                &mut os,
                &[raw(Mime::TextPlain, "CF_UNICODETEXT", b"x".to_vec())],
            )
            .unwrap();
            assert_eq!(seq, 1001, "the value read while open");
            assert_ne!(seq, os.write_seq.get());
            assert!(os.ev().contains(&Ev::OwnerCheck(false)));
            assert_owner_destroyed(&os);
        }

        #[test]
        fn failure_after_empty_frees_handles() {
            // The second SetClipboardData fails: the clipboard is closed, then the owner window
            // destroyed; `Backend` is returned; the failing block and every block not yet
            // handed over are freed.
            let mut os = FakeOs::new();
            os.fail_set_at = Some(1);
            let err = write_with(&mut os, &all_reps()).unwrap_err();
            assert!(matches!(err, PlatformError::Backend { .. }), "{err:?}");
            assert_eq!(os.allocs.get(), 7);
            assert_eq!(os.written.borrow().len(), 1);
            assert_eq!(os.freed(), 6);
            let ev = os.ev();
            assert_eq!(ev[ev.len() - 2..], [Ev::Close, Ev::DestroyOwner], "{ev:?}");
            assert!(!ev
                .iter()
                .any(|e| matches!(e, Ev::SeqOpen(_) | Ev::SeqNow(_))));
            assert_owner_destroyed(&os);

            // EmptyClipboard fails: closed, `Backend`, every block freed, nothing set.
            let mut os = FakeOs::new();
            os.empty_fails = true;
            let err = write_with(&mut os, &all_reps()).unwrap_err();
            assert!(matches!(err, PlatformError::Backend { .. }));
            assert_eq!(os.freed(), os.allocs.get());
            assert!(os.sets().is_empty());
            assert_eq!(
                os.ev(),
                vec![
                    Ev::CreateOwner,
                    Ev::Open,
                    Ev::Empty,
                    Ev::Close,
                    Ev::DestroyOwner
                ]
            );
        }

        #[test]
        fn busy_once() {
            let mut os = FakeOs::new();
            os.busy = true;
            assert_eq!(
                write_with(&mut os, &all_reps()).unwrap_err(),
                PlatformError::Busy
            );
            assert_eq!(
                os.opens.get(),
                1,
                "one attempt; retries belong to the actor"
            );
            assert_eq!(os.ev(), vec![Ev::CreateOwner, Ev::DestroyOwner]);
            assert_eq!(os.freed(), os.allocs.get(), "every block freed");
        }

        #[test]
        fn owner_window_destroyed_on_every_path() {
            // Success.
            let mut os = FakeOs::new();
            write_with(&mut os, &all_reps()).unwrap();
            assert_owner_destroyed(&os);
            // Busy, EmptyClipboard failure, SetClipboardData failure, stolen ownership.
            for setup in [
                (|o: &mut FakeOs| o.busy = true) as fn(&mut FakeOs),
                |o| o.empty_fails = true,
                |o| o.fail_set_at = Some(0),
                |o| o.fail_set_at = Some(6),
                |o| o.stolen_after_close = true,
            ] {
                let mut os = FakeOs::new();
                setup(&mut os);
                let _ = write_with(&mut os, &all_reps());
                assert_owner_destroyed(&os);
                assert_eq!(
                    os.freed() + u32::try_from(os.written.borrow().len()).unwrap(),
                    os.allocs.get()
                );
            }
            // The window cannot be created: nothing opened, every block freed.
            let mut os = FakeOs::new();
            os.owner_fails = true;
            assert!(matches!(
                write_with(&mut os, &all_reps()),
                Err(PlatformError::Backend { .. })
            ));
            assert_eq!(os.opens.get(), 0);
            assert!(os.ev().is_empty());
            assert_eq!(os.freed(), os.allocs.get());
            // Nothing writable: no window at all.
            let mut os = FakeOs::new();
            let reps = [raw(
                Mime::UriList,
                "CF_HDROP",
                b"https://example.com/".to_vec(),
            )];
            assert!(write_with(&mut os, &reps).is_err());
            assert!(os.ev().is_empty());
        }

        #[test]
        fn each_write_has_its_own_owner_on_its_own_thread() {
            // D-038: no window outlives a write, so any thread may write (the S-2 wrong-thread
            // check is no longer needed).
            let mut os = FakeOs::new();
            let reps = [raw(Mime::TextPlain, "CF_UNICODETEXT", b"x".to_vec())];
            write_with(&mut os, &reps).unwrap();
            let os = std::thread::spawn(move || {
                let mut os = os;
                let reps = [raw(Mime::TextPlain, "CF_UNICODETEXT", b"y".to_vec())];
                write_with(&mut os, &reps).unwrap();
                os
            })
            .join()
            .unwrap();
            assert_eq!(os.count(Ev::CreateOwner), 2);
            assert_eq!(os.count(Ev::DestroyOwner), 2);
        }

        #[test]
        #[ignore = "writes the real clipboard"]
        fn real_clipboard_write_copyback_suppressed() {
            use darkclip_platform::ClipboardMonitor;
            let mut m = crate::monitor::WinMonitor::new(Box::new(|| None));
            let (tx, rx) = std::sync::mpsc::channel();
            m.start(tx).unwrap();
            let mut a = WinAccess::new();
            let reps = [raw(
                Mime::TextPlain,
                "CF_UNICODETEXT",
                b"darkclip test 104".to_vec(),
            )];
            let seq = retry(|| a.write(&reps));
            // D-038: the monitor's signal for our write carries exactly the returned
            // (post-close) number, and our write causes no further signal.
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
            let mut found = false;
            while let Ok(s) =
                rx.recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
            {
                if s.seq == seq {
                    found = true;
                    break;
                }
                assert!(s.seq < seq, "signal {} after our write {seq}", s.seq);
            }
            assert!(found, "no signal with seq {seq}");
            let extra = rx.recv_timeout(std::time::Duration::from_millis(300));
            m.stop();
            assert!(extra.is_err(), "a further signal after our write");
        }

        #[test]
        #[ignore = "writes the real clipboard"]
        fn real_clipboard_write_image_readable_by_read() {
            // 2x1: opaque red, half-transparent green.
            let mut png_bytes = Vec::new();
            {
                let mut enc = png::Encoder::new(&mut png_bytes, 2, 1);
                enc.set_color(png::ColorType::Rgba);
                enc.set_depth(png::BitDepth::Eight);
                let mut w = enc.write_header().unwrap();
                w.write_image_data(&[255, 0, 0, 255, 0, 255, 0, 128])
                    .unwrap();
            }
            let mut a = WinAccess::new();
            retry(|| a.write(&[raw(Mime::ImagePng, "PNG", png_bytes.clone())]));
            let meta = retry(|| a.snapshot_meta());
            assert!(meta.markers.is_empty(), "D-012: {:?}", meta.markers);
            for (name, alpha) in [("PNG", 128), ("CF_DIBV5", 128), ("CF_DIB", 255)] {
                let p = plan(&[(Mime::ImagePng, name)], 0, 1 << 20);
                let reps = retry(|| a.read(&p));
                assert_eq!(reps.len(), 1, "{name}");
                assert_eq!(
                    (reps[0].width, reps[0].height),
                    (Some(2), Some(1)),
                    "{name}"
                );
                let mut dec = png::Decoder::new(std::io::Cursor::new(&reps[0].bytes[..]));
                dec.set_transformations(png::Transformations::EXPAND);
                let mut r = dec.read_info().unwrap();
                let mut buf = vec![0; r.output_buffer_size().unwrap()];
                r.next_frame(&mut buf).unwrap();
                assert_eq!(buf[..8], [255, 0, 0, 255, 0, 255, 0, alpha], "{name}");
            }
        }
    }
}
