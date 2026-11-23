//! Local text transforms (Task 404). Pure and deterministic; errors are typed and never
//! carry content (GC-06). No network, no OS access.
//!
//! Layout: this file holds the public types and the line/case transforms; `json` (validate,
//! pretty, minify), `codec` (percent-encoding and Base64) and `html` (HTML to text).

mod codec;
mod html;
mod json;
#[cfg(test)]
mod tests;

/// Largest input any transform accepts (also the cap on decoded Base64 output).
pub const MAX_INPUT_BYTES: usize = 1024 * 1024;
/// Deepest JSON nesting accepted.
pub const MAX_JSON_DEPTH: usize = 128;

/// Why a transform did not produce text. Never carries any part of the input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum TransformError {
    #[error("input too large")]
    TooLarge,
    #[error("invalid JSON")]
    InvalidJson,
    #[error("JSON nested too deeply")]
    TooDeep,
    #[error("invalid percent-encoding")]
    InvalidUrl,
    #[error("invalid Base64")]
    InvalidBase64,
    #[error("result is not valid UTF-8 text")]
    NotUtf8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Transform {
    /// Whitespace at both ends of the whole text.
    Trim,
    /// Whitespace at both ends of every line; line endings are kept.
    TrimLines,
    LineEndingsLf,
    LineEndingsCrlf,
    Upper,
    Lower,
    JsonPretty,
    JsonMinify,
    UrlEncode,
    UrlDecode,
    Base64Encode,
    Base64Decode,
    HtmlToText,
}

impl Transform {
    pub const ALL: [Transform; 13] = [
        Self::Trim,
        Self::TrimLines,
        Self::LineEndingsLf,
        Self::LineEndingsCrlf,
        Self::Upper,
        Self::Lower,
        Self::JsonPretty,
        Self::JsonMinify,
        Self::UrlEncode,
        Self::UrlDecode,
        Self::Base64Encode,
        Self::Base64Decode,
        Self::HtmlToText,
    ];

    /// Applies the transform. Input over [`MAX_INPUT_BYTES`] is refused, never cut, and so
    /// is output over the same cap (checked while the output is built).
    pub fn apply(self, input: &str) -> Result<String, TransformError> {
        if input.len() > MAX_INPUT_BYTES {
            return Err(TransformError::TooLarge);
        }
        match self {
            Self::Trim => Ok(input.trim().to_string()),
            Self::TrimLines => Ok(trim_lines(input)),
            Self::LineEndingsLf => Ok(to_lf(input)),
            Self::LineEndingsCrlf => crlf(input),
            Self::Upper => map_case(input, true),
            Self::Lower => map_case(input, false),
            Self::JsonPretty => json::format(input, true),
            Self::JsonMinify => json::format(input, false),
            Self::UrlEncode => codec::url_encode(input),
            Self::UrlDecode => codec::url_decode(input),
            Self::Base64Encode => codec::base64_encode(input.as_bytes()),
            Self::Base64Decode => codec::base64_decode(input),
            Self::HtmlToText => Ok(html::to_text(input)),
        }
    }
}

fn crlf(input: &str) -> Result<String, TransformError> {
    let lf = to_lf(input);
    let extra = lf.bytes().filter(|&b| b == b'\n').count();
    if lf.len() + extra > MAX_INPUT_BYTES {
        return Err(TransformError::TooLarge);
    }
    Ok(lf.replace('\n', "\r\n"))
}

fn map_case(input: &str, upper: bool) -> Result<String, TransformError> {
    let mut out = String::with_capacity(input.len());
    for c in input.chars() {
        if upper {
            out.extend(c.to_uppercase());
        } else {
            out.extend(c.to_lowercase());
        }
        if out.len() > MAX_INPUT_BYTES {
            return Err(TransformError::TooLarge);
        }
    }
    Ok(out)
}

fn to_lf(input: &str) -> String {
    input.replace("\r\n", "\n").replace('\r', "\n")
}

fn trim_lines(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for line in input.split_inclusive('\n') {
        let (body, ending) = match line.strip_suffix("\r\n") {
            Some(b) => (b, "\r\n"),
            None => match line.strip_suffix('\n') {
                Some(b) => (b, "\n"),
                None => (line, ""),
            },
        };
        out.push_str(body.trim());
        out.push_str(ending);
    }
    out
}

impl Transform {
    /// The menu label of this transform.
    pub fn label(self) -> crate::i18n::Msg {
        use crate::i18n::Msg;
        match self {
            Self::Trim => Msg::TfTrim,
            Self::TrimLines => Msg::TfTrimLines,
            Self::LineEndingsLf => Msg::TfLf,
            Self::LineEndingsCrlf => Msg::TfCrlf,
            Self::Upper => Msg::TfUpper,
            Self::Lower => Msg::TfLower,
            Self::JsonPretty => Msg::TfJsonPretty,
            Self::JsonMinify => Msg::TfJsonMinify,
            Self::UrlEncode => Msg::TfUrlEncode,
            Self::UrlDecode => Msg::TfUrlDecode,
            Self::Base64Encode => Msg::TfB64Encode,
            Self::Base64Decode => Msg::TfB64Decode,
            Self::HtmlToText => Msg::TfHtmlToText,
        }
    }
}

impl TransformError {
    /// The user-visible explanation (never includes content).
    pub fn message(self) -> crate::i18n::Msg {
        use crate::i18n::Msg;
        match self {
            Self::TooLarge => Msg::TfErrTooLarge,
            Self::InvalidJson => Msg::TfErrJson,
            Self::TooDeep => Msg::TfErrDeep,
            Self::InvalidUrl => Msg::TfErrUrl,
            Self::InvalidBase64 => Msg::TfErrBase64,
            Self::NotUtf8 => Msg::TfErrUtf8,
        }
    }
}
