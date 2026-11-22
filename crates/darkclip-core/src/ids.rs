//! Identifiers: UUID v7 text, time-ordered.

use crate::error::CoreError;
use crate::time::Timestamp;
use serde::{de, Deserialize, Deserializer, Serialize, Serializer};
use uuid::Uuid;

fn new_v7(now: Timestamp) -> Uuid {
    let ms = u64::try_from(now.0).unwrap_or(0);
    let secs = ms / 1000;
    let nanos = u32::try_from(ms % 1000).unwrap_or(0) * 1_000_000;
    Uuid::new_v7(uuid::Timestamp::from_unix(uuid::NoContext, secs, nanos))
}

fn parse(s: &str) -> Result<Uuid, CoreError> {
    if s.len() != 36 {
        return Err(CoreError::InvalidId);
    }
    if s.bytes().any(|b| b.is_ascii_uppercase()) {
        return Err(CoreError::InvalidId);
    }
    let u = Uuid::parse_str(s).map_err(|_| CoreError::InvalidId)?;
    if u.get_version_num() != 7 || u.get_variant() != uuid::Variant::RFC4122 {
        return Err(CoreError::InvalidId);
    }
    Ok(u)
}

macro_rules! id_type {
    ($name:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(uuid::Uuid);

        impl $name {
            /// New UUID v7 whose time component is `now`.
            pub fn new_v7(now: Timestamp) -> Self {
                Self(new_v7(now))
            }

            /// Parses hyphenated UUID v7 text.
            pub fn parse(s: &str) -> Result<Self, CoreError> {
                parse(s).map(Self)
            }

            pub fn as_uuid(&self) -> &uuid::Uuid {
                &self.0
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.0.hyphenated())
            }
        }

        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.collect_str(self)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let s = String::deserialize(d)?;
                Self::parse(&s).map_err(de::Error::custom)
            }
        }
    };
}

id_type!(ClipId);
id_type!(CollectionId);
id_type!(TagId);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uuid_v7_sorts_by_time() {
        let base = 1_795_320_104_903i64;
        let mut prev: Option<String> = None;
        for i in 0..250i64 {
            let t = Timestamp(base + i);
            let a = ClipId::new_v7(t).to_string();
            let b = ClipId::new_v7(t).to_string();
            assert_ne!(a, b, "same-ms ids must be distinct");
            if let Some(p) = &prev {
                assert!(p < &a, "{p} !< {a}");
            }
            prev = Some(a);
        }
    }

    #[test]
    fn parse_rejects_uppercase_and_bad_variant() {
        assert_eq!(
            ClipId::parse("019A8F40-02AA-7B11-8C22-5E6F7A8B9C0D"),
            Err(CoreError::InvalidId)
        );
        // variant nibble 'c' is not in [89ab]
        assert_eq!(
            ClipId::parse("019a8f40-02aa-7b11-cc22-5e6f7a8b9c0d"),
            Err(CoreError::InvalidId)
        );
    }

    #[test]
    fn parse_roundtrip_and_rejects() {
        let id = TagId::new_v7(Timestamp(1_795_320_104_903));
        assert_eq!(TagId::parse(&id.to_string()).unwrap(), id);
        assert_eq!(ClipId::parse("nope"), Err(CoreError::InvalidId));
        // v4 is not accepted
        assert_eq!(
            ClipId::parse("550e8400-e29b-41d4-a716-446655440000"),
            Err(CoreError::InvalidId)
        );
        assert!(ClipId::parse("019a8f40-02aa-7b11-8c22-5e6f7a8b9c0d").is_ok());
    }
}
