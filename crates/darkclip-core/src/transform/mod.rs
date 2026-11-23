//! Local text transforms (Task 404). Pure and deterministic; errors are typed and never
//! carry content (GC-06).

pub const MAX_INPUT_BYTES: usize = 1024 * 1024;
pub const MAX_JSON_DEPTH: usize = 128;

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
    Trim,
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

    pub fn apply(self, _input: &str) -> Result<String, TransformError> {
        Err(TransformError::TooLarge)
    }
}

#[cfg(test)]
mod tests;
