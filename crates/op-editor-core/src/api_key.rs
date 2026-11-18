//! A secret API key whose `Debug` never prints the secret.
//!
//! `AgentSettings`, the provider list and the request context all derive
//! `Debug`; wrapping the key keeps a stray `{:?}`, an `assert_eq!` failure or a
//! panic message from writing it into a log. The key still serialises as a
//! plain string (the settings file is plaintext JSON, mode 0600, like the
//! Openverse client secret), so persisted settings are unchanged in shape.

use std::fmt;

#[derive(Clone, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct ApiKey(String);

impl ApiKey {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// The secret. Call this only where the key goes on the wire or into the
    /// settings file, never into a log line or an agent-facing payload.
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// The key without surrounding whitespace, `None` when nothing is left.
    pub fn trimmed(&self) -> Option<&str> {
        let trimmed = self.0.trim();
        (!trimmed.is_empty()).then_some(trimmed)
    }

    pub fn is_blank(&self) -> bool {
        self.trimmed().is_none()
    }
}

impl From<String> for ApiKey {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for ApiKey {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

impl fmt::Debug for ApiKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_blank() {
            f.write_str("ApiKey(<unset>)")
        } else {
            f.write_str("ApiKey(<redacted>)")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_redacted_in_debug_output() {
        let key = ApiKey::new("sk-very-secret-1234");
        assert!(!format!("{key:?}").contains("secret"));
        assert!(!format!("{key:#?}").contains("1234"));
        assert_eq!(format!("{:?}", ApiKey::default()), "ApiKey(<unset>)");
        // A container that derives Debug is covered too.
        #[derive(Debug)]
        struct Holder {
            #[allow(dead_code)]
            key: ApiKey,
        }
        assert!(!format!("{:?}", Holder { key }).contains("secret"));
    }

    #[test]
    fn key_serialises_as_a_plain_string() {
        let json = serde_json::to_string(&ApiKey::new("abc")).unwrap();
        assert_eq!(json, "\"abc\"");
        let back: ApiKey = serde_json::from_str(&json).unwrap();
        assert_eq!(back.expose(), "abc");
    }

    #[test]
    fn blank_and_padded_keys_are_unset_or_trimmed() {
        assert!(ApiKey::new("  \t").is_blank());
        assert_eq!(ApiKey::new("  k ").trimmed(), Some("k"));
    }
}
