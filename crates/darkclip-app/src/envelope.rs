//! The JSON envelope every API response and CLI `--json` output serializes to
//! (`docs/architecture/schemas/api-envelope.schema.json`).
//!
//! Error messages come from the i18n table by code and never contain clip content (GC-06,
//! GC-14); the `&'static str` detail inside an `ApiError` is for callers, not for the wire.

use crate::events::ApiError;
use darkclip_core::{t, Lang, Msg};
use serde::{Serialize, Serializer};
use serde_json::{json, Map, Value};

pub const API_VERSION: u32 = 1;

/// A page of results. Serializes as `{"items": [...], "page": {...}}`, which
/// [`to_envelope`] splits into the envelope's `data` and `page`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page<T> {
    pub items: Vec<T>,
    /// The limit that was applied (after clamping).
    pub limit: u16,
    /// `None` (omitted on the wire) when there is no further page, and always for text searches.
    pub next_cursor: Option<String>,
}

impl<T: Serialize> Serialize for Page<T> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut page = Map::new();
        page.insert("limit".into(), json!(self.limit));
        page.insert("returned".into(), json!(self.items.len()));
        if let Some(c) = &self.next_cursor {
            page.insert("next_cursor".into(), json!(c));
        }
        let items = serde_json::to_value(&self.items).map_err(serde::ser::Error::custom)?;
        let mut m = Map::new();
        m.insert("items".into(), items);
        m.insert("page".into(), Value::Object(page));
        Value::Object(m).serialize(s)
    }
}

fn message(e: &ApiError) -> Msg {
    match e {
        ApiError::NotFound => Msg::ErrNotFound,
        ApiError::InvalidArgument(_) => Msg::ErrInvalidArgument,
        ApiError::PermissionDenied(_) => Msg::ErrPermissionDenied,
        ApiError::UnsupportedPlatform => Msg::ErrUnsupportedPlatform,
        ApiError::ClipboardBusy => Msg::ErrClipboardBusy,
        ApiError::Storage => Msg::ErrStorageError,
        ApiError::AppNotRunning => Msg::ErrAppNotRunning,
        ApiError::SchemaTooNew => Msg::ErrSchemaTooNew,
        ApiError::Internal => Msg::ErrInternal,
    }
}

/// Builds the envelope. A `Page` result becomes `data` (the items) plus `page`.
pub fn to_envelope<T: Serialize>(r: &Result<T, ApiError>, lang: Lang) -> Value {
    let mut out = Map::new();
    out.insert("api_version".into(), json!(API_VERSION));
    match r {
        Ok(data) => {
            out.insert("ok".into(), Value::Bool(true));
            let v = match serde_json::to_value(data) {
                Ok(v) => v,
                Err(_) => return to_envelope::<()>(&Err(ApiError::Internal), lang),
            };
            match v {
                Value::Object(mut m)
                    if m.len() == 2 && m.contains_key("items") && m.contains_key("page") =>
                {
                    out.insert("data".into(), m.remove("items").unwrap_or(Value::Null));
                    out.insert("page".into(), m.remove("page").unwrap_or(Value::Null));
                }
                other => {
                    out.insert("data".into(), other);
                }
            }
        }
        Err(e) => {
            out.insert("ok".into(), Value::Bool(false));
            out.insert(
                "error".into(),
                json!({ "code": e.code(), "message": t(lang, message(e)) }),
            );
        }
    }
    Value::Object(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::ClipSummaryView;
    use darkclip_core::{ClipId, ClipType, Timestamp};
    use std::collections::BTreeSet;

    const SEARCH_EXAMPLE: &str =
        include_str!("../../../docs/architecture/examples/api-envelope.search.example.json");
    const ERROR_EXAMPLE: &str =
        include_str!("../../../docs/architecture/examples/api-envelope.error.example.json");
    const SCHEMA: &str =
        include_str!("../../../docs/architecture/schemas/api-envelope.schema.json");

    fn keys(v: &Value) -> BTreeSet<String> {
        v.as_object().unwrap().keys().cloned().collect()
    }

    fn example_summary() -> ClipSummaryView {
        ClipSummaryView {
            id: ClipId::parse("019a8f3b-7e21-7c4d-9a10-3f2b6c1d8e90").unwrap(),
            kind: ClipType::Code,
            preview: "SELECT id, title FROM invoices WHERE paid = 0 ORDER BY due_at;".into(),
            pinned: true,
            last_copied_at: Timestamp::parse_rfc3339("2026-11-22T04:01:44.903Z").unwrap(),
            masked: false,
            source_app: None,
        }
    }

    #[test]
    fn envelope_ok_matches_schema() {
        let example: Value = serde_json::from_str(SEARCH_EXAMPLE).unwrap();
        let page = Page {
            items: vec![example_summary()],
            limit: 20,
            next_cursor: None,
        };
        let env = to_envelope(&Ok(page), Lang::En);
        // Same document as the example, field for field.
        assert_eq!(env, example);
        assert_eq!(keys(&env), keys(&example));
        assert_eq!(env["api_version"], 1);
        assert_eq!(env["ok"], true);
        assert!(env.get("error").is_none());
        assert_eq!(keys(&env["page"]), keys(&example["page"]));
        assert_eq!(keys(&env["data"][0]), keys(&example["data"][0]));
        // A non-page result has `data` and no `page`.
        let plain = to_envelope(&Ok(json!({"clips": 3})), Lang::En);
        assert_eq!(plain["data"]["clips"], 3);
        assert!(plain.get("page").is_none());
        assert_eq!(
            keys(&plain),
            BTreeSet::from(["api_version".into(), "ok".into(), "data".into()])
        );
        // A cursor is present only when there is one.
        let with = to_envelope(
            &Ok(Page {
                items: Vec::<ClipSummaryView>::new(),
                limit: 5,
                next_cursor: Some("abc".into()),
            }),
            Lang::En,
        );
        assert_eq!(with["page"]["next_cursor"], "abc");
        assert_eq!(with["page"]["returned"], 0);
    }

    #[test]
    fn envelope_error_matches_schema() {
        let example: Value = serde_json::from_str(ERROR_EXAMPLE).unwrap();
        let env = to_envelope::<()>(&Err(ApiError::PermissionDenied("x")), Lang::En);
        assert_eq!(keys(&env), keys(&example));
        assert_eq!(keys(&env["error"]), keys(&example["error"]));
        assert_eq!(env["api_version"], 1);
        assert_eq!(env["ok"], false);
        assert_eq!(env["error"]["code"], example["error"]["code"]);
        assert!(env.get("data").is_none());

        // Every code is in the schema enum; messages are localized, bounded and generic.
        let schema: Value = serde_json::from_str(SCHEMA).unwrap();
        let allowed: BTreeSet<String> = schema["properties"]["error"]["properties"]["code"]["enum"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        let all = [
            ApiError::NotFound,
            ApiError::InvalidArgument("secret-looking detail"),
            ApiError::PermissionDenied("secret-looking detail"),
            ApiError::UnsupportedPlatform,
            ApiError::ClipboardBusy,
            ApiError::Storage,
            ApiError::AppNotRunning,
            ApiError::SchemaTooNew,
            ApiError::Internal,
        ];
        let codes: BTreeSet<String> = all.iter().map(|e| e.code().to_string()).collect();
        assert_eq!(codes, allowed);
        for e in &all {
            let en = to_envelope::<()>(&Err(e.clone()), Lang::En);
            let vi = to_envelope::<()>(&Err(e.clone()), Lang::Vi);
            let (m_en, m_vi) = (
                en["error"]["message"].as_str().unwrap(),
                vi["error"]["message"].as_str().unwrap(),
            );
            assert!(!m_en.is_empty() && m_en.len() <= 512);
            assert_ne!(m_en, m_vi);
            assert!(!m_en.contains("secret-looking"));
        }
    }
}
