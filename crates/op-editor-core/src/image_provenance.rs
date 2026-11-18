//! Creator and licence record of a stock image that landed on a node.
//!
//! The canonical `.op` schema (jian-ops-schema) has no generic metadata field
//! on image nodes, image fills or the document, so this record lives in the
//! per-document side table [`EditorState::image_provenance`](crate::EditorState),
//! keyed by node id. It travels with the document across tab switches but is
//! NOT written to the saved file: persisting it needs a schema field.

/// JSON keys of [`ImageProvenance::to_json`] — stable, camelCase, so the
/// future schema field can adopt the exact same shape.
pub const PROVENANCE_KEYS: [&str; 5] = [
    "provider",
    "creator",
    "license",
    "licenseUrl",
    "sourcePageUrl",
];

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ImageProvenance {
    /// Provider stage id (`"openverse"`, `"wikimedia"`).
    pub provider: String,
    /// Plain-text creator name.
    pub creator: Option<String>,
    /// Short licence name, e.g. `"CC BY 4.0"`.
    pub license: Option<String>,
    pub license_url: Option<String>,
    /// Human landing page of the work at the provider.
    pub source_page_url: Option<String>,
}

impl ImageProvenance {
    /// `{provider, creator?, license?, licenseUrl?, sourcePageUrl?}`; absent
    /// values are omitted rather than written as null.
    pub fn to_json(&self) -> serde_json::Value {
        let mut object = serde_json::Map::new();
        object.insert("provider".into(), self.provider.clone().into());
        let optional = [
            ("creator", &self.creator),
            ("license", &self.license),
            ("licenseUrl", &self.license_url),
            ("sourcePageUrl", &self.source_page_url),
        ];
        for (key, value) in optional {
            if let Some(value) = value {
                object.insert(key.into(), value.clone().into());
            }
        }
        serde_json::Value::Object(object)
    }

    /// Inverse of [`to_json`](Self::to_json); `None` without a provider.
    pub fn from_json(value: &serde_json::Value) -> Option<Self> {
        let text = |key: &str| value.get(key)?.as_str().map(str::to_string);
        Some(Self {
            provider: text("provider").filter(|provider| !provider.is_empty())?,
            creator: text("creator"),
            license: text("license"),
            license_url: text("licenseUrl"),
            source_page_url: text("sourcePageUrl"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_round_trip_uses_the_stable_keys() {
        let provenance = ImageProvenance {
            provider: "wikimedia".into(),
            creator: Some("Jane Doe".into()),
            license: Some("CC BY 4.0".into()),
            license_url: Some("https://creativecommons.org/licenses/by/4.0/".into()),
            source_page_url: Some("https://commons.wikimedia.org/wiki/File:X.jpg".into()),
        };
        let json = provenance.to_json();
        let mut keys: Vec<&str> = json
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        let mut expected = PROVENANCE_KEYS.to_vec();
        expected.sort_unstable();
        assert_eq!(keys, expected);
        assert_eq!(ImageProvenance::from_json(&json), Some(provenance));
    }

    #[test]
    fn absent_values_are_omitted_and_a_provider_is_required() {
        let json = ImageProvenance {
            provider: "openverse".into(),
            ..Default::default()
        }
        .to_json();
        assert_eq!(json.as_object().unwrap().len(), 1);
        assert!(ImageProvenance::from_json(&serde_json::json!({"license": "CC0"})).is_none());
    }
}
