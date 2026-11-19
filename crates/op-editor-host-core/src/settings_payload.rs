//! Shared settings-persistence payload shapes + conversions.
//!
//! Single source of truth for the serde payload structs and the
//! `*_to_payload` / `*_from_payload` conversions that both settings
//! stores use: the desktop/daemon `settings.json`
//! (`op-host-services::settings_io`) and the browser localStorage
//! snapshots (`op-host-web::web_settings`). Field names and formats are
//! wire format — settings files on disk / in localStorage must
//! round-trip unchanged, so any new field lands here once and both
//! hosts pick it up together instead of silently dropping it on one
//! side.
//!
//! This module is transport-free and wasm32-clean (op-editor-core +
//! serde only).

use op_editor_core::{
    normalize_builtin_models, ApiKey, BuiltinAgentConfig, BuiltinAgentKind, BuiltinAgentPresetKey,
    ImageGenProfile, ImageGenProvider, SessionDocument, ThemeMode,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RecentFilePayload {
    pub path: String,
    pub modified_at: u64,
}

/// One document that was open when the app last saved its settings.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct OpenDocumentPayload {
    pub path: String,
    /// Which tab was on screen. Absent in older writers, so it defaults off.
    #[serde(default)]
    pub active: bool,
}

pub fn open_documents_to_payload(documents: &[SessionDocument]) -> Vec<OpenDocumentPayload> {
    documents
        .iter()
        .map(|document| OpenDocumentPayload {
            path: document.path.clone(),
            active: document.active,
        })
        .collect()
}

/// Restore the open-document list. Empty paths are dropped and at most one
/// entry may stay active (the first), so a hand-edited file cannot open two
/// documents as "the" active one.
pub fn open_documents_from_payload(payload: Vec<OpenDocumentPayload>) -> Vec<SessionDocument> {
    let mut seen_active = false;
    payload
        .into_iter()
        .filter(|entry| !entry.path.is_empty())
        .map(|entry| {
            let active = entry.active && !seen_active;
            seen_active |= active;
            SessionDocument {
                path: entry.path,
                active,
            }
        })
        .collect()
}

/// Most project folders kept across launches.
pub const MAX_OPEN_PROJECTS: usize = 50;

pub fn open_projects_to_payload(projects: &[String]) -> Vec<String> {
    open_projects_from_payload(projects.to_vec())
}

/// Restore the open-project list: blank entries and repeats are dropped (the
/// first spelling wins) and at most [`MAX_OPEN_PROJECTS`] stay, so a
/// hand-edited file cannot grow the list without bound.
pub fn open_projects_from_payload(payload: Vec<String>) -> Vec<String> {
    let mut kept: Vec<String> = Vec::new();
    for path in payload {
        if path.trim().is_empty() || kept.contains(&path) {
            continue;
        }
        kept.push(path);
        if kept.len() == MAX_OPEN_PROJECTS {
            break;
        }
    }
    kept
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct BuiltinAgentPayload {
    pub id: String,
    #[serde(default)]
    pub preset: Option<String>,
    pub display_name: String,
    pub kind: String,
    #[serde(default)]
    pub api_key: String,
    /// Legacy single-model mirror. New writers keep it equal to the first
    /// saved model so older best-effort readers retain a usable fallback.
    #[serde(default)]
    pub model: String,
    /// Canonical multi-model payload. Omitted for zero/one model so the common
    /// single-model shape stays downgrade-readable; when present it wins over
    /// `model`, including an explicitly empty list.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub models: Option<Vec<String>>,
    pub base_url: String,
    pub enabled: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ImageGenProfilePayload {
    pub id: String,
    pub name: String,
    pub provider: String,
    pub api_key: String,
    pub model: String,
    #[serde(default)]
    pub base_url: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct OpenverseOAuthPayload {
    pub client_id: String,
    pub client_secret: String,
}

/// Stock-provider keys and the Unsplash / Pexels opt-ins. A struct of its own so the
/// derived `Debug` of the whole settings payload prints no key (`ApiKey` is
/// redacted); the file itself holds the plain strings (0600).
#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
pub struct StockProvidersPayload {
    #[serde(default)]
    pub pixabay_api_key: ApiKey,
    #[serde(default)]
    pub unsplash_access_key: ApiKey,
    #[serde(default)]
    pub unsplash_enabled: bool,
    #[serde(default)]
    pub pexels_api_key: ApiKey,
    #[serde(default)]
    pub pexels_enabled: bool,
}

/// `None` while nothing is configured, so a settings file of a user who never
/// touches stock providers stays byte-identical to before.
pub fn stock_providers_to_payload(
    settings: &op_editor_core::agent_settings::AgentSettings,
) -> Option<StockProvidersPayload> {
    let payload = StockProvidersPayload {
        pixabay_api_key: settings.pixabay_api_key.trimmed().unwrap_or("").into(),
        unsplash_access_key: settings.unsplash_access_key.trimmed().unwrap_or("").into(),
        unsplash_enabled: settings.unsplash_enabled,
        pexels_api_key: settings.pexels_api_key.trimmed().unwrap_or("").into(),
        pexels_enabled: settings.pexels_enabled,
    };
    (payload != StockProvidersPayload::default()).then_some(payload)
}

pub fn apply_stock_providers_payload(
    settings: &mut op_editor_core::agent_settings::AgentSettings,
    payload: Option<StockProvidersPayload>,
) {
    let payload = payload.unwrap_or_default();
    settings.pixabay_api_key = payload.pixabay_api_key;
    settings.unsplash_access_key = payload.unsplash_access_key;
    settings.unsplash_enabled = payload.unsplash_enabled;
    settings.pexels_api_key = payload.pexels_api_key;
    settings.pexels_enabled = payload.pexels_enabled;
}

pub fn builtin_agent_to_payload(agent: &BuiltinAgentConfig) -> BuiltinAgentPayload {
    let models = normalize_builtin_models(agent.models.iter().cloned());
    let model = models.first().cloned().unwrap_or_default();
    let saved_models = (models.len() > 1).then_some(models);
    BuiltinAgentPayload {
        id: agent.id.clone(),
        preset: Some(agent.preset.as_str().into()),
        display_name: agent.display_name.clone(),
        kind: match agent.kind {
            BuiltinAgentKind::Anthropic => "anthropic",
            BuiltinAgentKind::OpenAiCompat => "openai-compat",
        }
        .into(),
        api_key: agent.api_key.clone(),
        model,
        models: saved_models,
        base_url: agent.base_url.clone(),
        enabled: agent.enabled,
    }
}

pub fn builtin_agent_from_payload(payload: BuiltinAgentPayload) -> Option<BuiltinAgentConfig> {
    let kind = match payload.kind.as_str() {
        "anthropic" => BuiltinAgentKind::Anthropic,
        "openai" | "openai-compat" | "openai_compat" => BuiltinAgentKind::OpenAiCompat,
        _ => return None,
    };
    let legacy_model = payload.model;
    let models = normalize_builtin_models(match payload.models {
        Some(models) => models,
        None if legacy_model.trim().is_empty() => Vec::new(),
        None => vec![legacy_model.clone()],
    });
    let inference_model = models.first().map(String::as_str).unwrap_or("");
    Some(BuiltinAgentConfig {
        id: payload.id,
        preset: payload
            .preset
            .as_deref()
            .and_then(BuiltinAgentPresetKey::from_str)
            .unwrap_or_else(|| {
                op_editor_core::infer_builtin_agent_preset(kind, &payload.base_url, inference_model)
            }),
        display_name: payload.display_name,
        kind,
        api_key: payload.api_key,
        models,
        base_url: payload.base_url,
        enabled: payload.enabled,
    })
}

/// Whether loading and re-serializing this payload preserves its exact model
/// representation. Strict settings stores use this before applying a snapshot
/// so normalization cannot silently erase newer or malformed data.
pub fn builtin_agent_payload_models_are_canonical(payload: &BuiltinAgentPayload) -> bool {
    let candidates = match payload.models.as_ref() {
        Some(models) => models.clone(),
        None if payload.model.trim().is_empty() => Vec::new(),
        None => vec![payload.model.clone()],
    };
    let normalized = normalize_builtin_models(candidates.iter().cloned());
    if normalized != candidates {
        return false;
    }
    let primary = normalized.first().map(String::as_str).unwrap_or("");
    if payload.model != primary {
        return false;
    }
    match payload.models.as_ref() {
        Some(models) => models.len() > 1,
        None => normalized.len() <= 1,
    }
}

pub fn image_gen_profile_to_payload(profile: &ImageGenProfile) -> ImageGenProfilePayload {
    ImageGenProfilePayload {
        id: profile.id.clone(),
        name: profile.name.clone(),
        provider: match profile.provider {
            ImageGenProvider::OpenAi => "openai",
            ImageGenProvider::Gemini => "gemini",
            ImageGenProvider::Replicate => "replicate",
            ImageGenProvider::Atlas => "atlas",
            ImageGenProvider::Custom => "custom",
            ImageGenProvider::Workbench => "workbench",
        }
        .into(),
        api_key: profile.api_key.clone(),
        model: profile.model.clone(),
        base_url: profile.base_url.clone(),
    }
}

pub fn image_gen_profile_from_payload(payload: ImageGenProfilePayload) -> Option<ImageGenProfile> {
    let provider = match payload.provider.as_str() {
        "openai" => ImageGenProvider::OpenAi,
        "gemini" => ImageGenProvider::Gemini,
        "replicate" => ImageGenProvider::Replicate,
        "atlas" => ImageGenProvider::Atlas,
        "custom" => ImageGenProvider::Custom,
        "workbench" => ImageGenProvider::Workbench,
        _ => return None,
    };
    Some(ImageGenProfile {
        id: payload.id,
        name: payload.name,
        provider,
        api_key: payload.api_key,
        model: payload.model,
        base_url: payload.base_url,
        test_status: op_editor_core::agent_settings::ImageTestStatus::Idle,
    })
}

pub fn openverse_oauth_to_payload(
    settings: &op_editor_core::agent_settings::AgentSettings,
) -> Option<OpenverseOAuthPayload> {
    let client_id = settings.openverse_client_id.trim();
    let client_secret = settings.openverse_client_secret.trim();
    if client_id.is_empty() && client_secret.is_empty() {
        None
    } else {
        Some(OpenverseOAuthPayload {
            client_id: client_id.to_string(),
            client_secret: client_secret.to_string(),
        })
    }
}

pub fn dedupe_builtin_agents(agents: Vec<BuiltinAgentConfig>) -> Vec<BuiltinAgentConfig> {
    let mut deduped: Vec<BuiltinAgentConfig> = Vec::new();
    for mut agent in agents {
        let models = std::mem::take(&mut agent.models);
        agent.set_models(models);
        if let Some(existing) = deduped.iter_mut().find(|existing| {
            same_builtin_agent_ownership_domain(existing, &agent)
                && existing.enabled == agent.enabled
                && existing.matches_add_candidate(
                    &agent.display_name,
                    &agent.api_key,
                    agent.kind,
                    &agent.base_url,
                    agent.preset,
                )
        }) {
            for model in agent.models {
                existing.add_model(model);
            }
        } else {
            deduped.push(agent);
        }
    }
    deduped
}

/// Browser-owned credentials and operator-owned credentials may intentionally
/// use the same provider transport. Their id domains carry deletion and
/// persistence ownership, so a best-effort load must never merge across that
/// boundary even when every backend field is equal.
fn same_builtin_agent_ownership_domain(a: &BuiltinAgentConfig, b: &BuiltinAgentConfig) -> bool {
    const BROWSER_BUILTIN_PREFIX: &str = "web-credential:builtin:";
    a.id.starts_with(BROWSER_BUILTIN_PREFIX) == b.id.starts_with(BROWSER_BUILTIN_PREFIX)
}

pub fn next_builtin_agent_id(agents: &[BuiltinAgentConfig]) -> u64 {
    agents
        .iter()
        .filter_map(|agent| agent.id.strip_prefix("builtin-")?.parse::<u64>().ok())
        .max()
        .unwrap_or(0)
        .saturating_add(1)
}

pub fn next_image_gen_profile_id(profiles: &[ImageGenProfile]) -> u64 {
    profiles
        .iter()
        .filter_map(|profile| profile.id.strip_prefix("igp-")?.parse::<u64>().ok())
        .max()
        .unwrap_or(0)
        .saturating_add(1)
}

/// Map a positional v1 `mcp_cli_enabled` array onto the current layout.
/// The current layout has thirteen slots: the seven-slot layout plus Gemini
/// CLI / Qwen Code / Cursor / Kimi / ZCode / DeepSeek Harness appended.
/// Older files may carry six or eight slots, both of which still held a
/// since-retired Gemini CLI slot at index 2 — those get it dropped so every
/// other CLI keeps its toggle. Every other historical length is a prefix of
/// the current layout.
pub fn migrate_mcp_cli_flags(flags: Vec<bool>) -> [bool; 13] {
    let mut migrated = [false; 13];
    match flags.len() {
        // Current layout (or a longer one written by a newer build).
        13.. => migrated.copy_from_slice(&flags[..13]),
        // Prefixes of the current layout: the CLIs added after them stay off.
        11 => migrated[..11].copy_from_slice(&flags),
        7 => migrated[..7].copy_from_slice(&flags),
        // Legacy layouts that carried the retired Gemini CLI at index 2.
        8..=10 => {
            migrated[0] = flags[0];
            migrated[1] = flags[1];
            migrated[2..7].copy_from_slice(&flags[3..8]);
        }
        3..=6 => {
            migrated[0] = flags[0];
            migrated[1] = flags[1];
            migrated[2..flags.len() - 1].copy_from_slice(&flags[3..]);
        }
        _ => {
            migrated[..flags.len()].copy_from_slice(&flags);
        }
    }
    migrated
}

pub fn theme_to_str(t: ThemeMode) -> &'static str {
    match t {
        ThemeMode::Dark => "dark",
        ThemeMode::Light => "light",
    }
}

pub fn str_to_theme(s: &str) -> ThemeMode {
    match s {
        "light" => ThemeMode::Light,
        _ => ThemeMode::Dark,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use op_editor_core::{MAX_BUILTIN_AGENT_MODELS, MAX_BUILTIN_MODEL_CHARS};

    fn payload(model: &str, models: Option<Vec<&str>>) -> BuiltinAgentPayload {
        BuiltinAgentPayload {
            id: "builtin-1".into(),
            preset: Some("custom".into()),
            display_name: "Private".into(),
            kind: "openai-compat".into(),
            api_key: "sk-test".into(),
            model: model.into(),
            models: models.map(|models| models.into_iter().map(str::to_string).collect()),
            base_url: "https://example.com/v1".into(),
            enabled: true,
        }
    }

    #[test]
    fn legacy_single_model_migrates_to_one_saved_model() {
        let agent = builtin_agent_from_payload(payload(" legacy-model ", None)).expect("agent");

        assert_eq!(agent.models, ["legacy-model"]);
        assert_eq!(agent.first_model(), Some("legacy-model"));
    }

    #[test]
    fn multi_model_payload_round_trips_with_legacy_first_mirror() {
        let agent = builtin_agent_from_payload(payload(
            "model-a",
            Some(vec!["model-a", "model-b", "model-c"]),
        ))
        .expect("agent");

        let saved = builtin_agent_to_payload(&agent);
        assert_eq!(saved.model, "model-a");
        assert_eq!(
            saved.models.as_deref(),
            Some(&["model-a".into(), "model-b".into(), "model-c".into()][..])
        );
        assert!(builtin_agent_payload_models_are_canonical(&saved));
    }

    #[test]
    fn explicit_models_take_precedence_over_conflicting_legacy_model() {
        let agent = builtin_agent_from_payload(payload(
            "legacy-ignored",
            Some(vec!["selected-a", "selected-b"]),
        ))
        .expect("agent");

        assert_eq!(agent.models, ["selected-a", "selected-b"]);
        assert!(
            !builtin_agent_payload_models_are_canonical(&payload(
                "legacy-ignored",
                Some(vec!["selected-a", "selected-b"])
            )),
            "strict stores must reject an ambiguous same-version payload"
        );
    }

    #[test]
    fn explicit_preset_is_preserved_when_first_model_matches_another_preset() {
        let mut payload = payload(
            "doubao-seed-2-0-pro-260215",
            Some(vec!["doubao-seed-2-0-pro-260215", "ark-code-latest"]),
        );
        payload.preset = Some("ark-coding".into());
        payload.kind = "anthropic".into();
        payload.base_url = "https://ark.cn-beijing.volces.com/api/coding".into();

        let agent = builtin_agent_from_payload(payload).expect("agent");

        assert_eq!(agent.preset, BuiltinAgentPresetKey::ArkCoding);
    }

    #[test]
    fn model_normalization_is_ordered_deduped_and_bounded() {
        let overlong = "x".repeat(MAX_BUILTIN_MODEL_CHARS + 1);
        let candidates = std::iter::once(" first ".to_string())
            .chain(std::iter::once("second".to_string()))
            .chain(std::iter::once("third\nfourth".to_string()))
            .chain(std::iter::once("first".to_string()))
            .chain(std::iter::once(String::new()))
            .chain(std::iter::once(overlong))
            .chain((0..MAX_BUILTIN_AGENT_MODELS + 8).map(|index| format!("model-{index}")));

        let normalized = normalize_builtin_models(candidates);

        assert_eq!(&normalized[..4], ["first", "second", "third", "fourth"]);
        assert_eq!(normalized.len(), MAX_BUILTIN_AGENT_MODELS);
        assert!(!normalized.iter().any(|model| model.is_empty()));
    }

    #[test]
    fn duplicate_backends_merge_their_saved_models() {
        let mut first_payload = payload("model-a", None);
        first_payload.enabled = true;
        let first = builtin_agent_from_payload(first_payload).expect("first");
        let mut second_payload = payload("model-b", None);
        second_payload.id = "builtin-2".into();
        second_payload.display_name = "Second label".into();
        let second = builtin_agent_from_payload(second_payload).expect("second");

        let merged = dedupe_builtin_agents(vec![first, second]);

        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].id, "builtin-1");
        assert_eq!(merged[0].models, ["model-a", "model-b"]);
        assert!(
            merged[0].enabled,
            "equal enabled state is preserved while the models merge"
        );
    }

    #[test]
    fn mixed_enabled_legacy_cards_are_not_deduplicated() {
        let mut first_payload = payload("model-a", None);
        first_payload.enabled = false;
        let first = builtin_agent_from_payload(first_payload).expect("first");
        let mut second_payload = payload("model-b", None);
        second_payload.id = "builtin-2".into();
        second_payload.enabled = true;
        let second = builtin_agent_from_payload(second_payload).expect("second");

        let deduped = dedupe_builtin_agents(vec![first, second]);

        assert_eq!(deduped.len(), 2);
        assert!(!deduped[0].enabled);
        assert!(deduped[1].enabled);
    }

    #[test]
    fn shared_transport_with_distinct_presets_is_not_deduplicated() {
        let mut first_payload = payload("doubao-seed-2-0-pro-260215", None);
        first_payload.preset = Some("doubao".into());
        first_payload.kind = "anthropic".into();
        first_payload.base_url = "https://ark.cn-beijing.volces.com/api/coding".into();
        let first = builtin_agent_from_payload(first_payload).expect("first");

        let mut second_payload = payload("ark-code-latest", None);
        second_payload.id = "builtin-2".into();
        second_payload.preset = Some("ark-coding".into());
        second_payload.kind = "anthropic".into();
        second_payload.base_url = "https://ark.cn-beijing.volces.com/api/coding".into();
        let second = builtin_agent_from_payload(second_payload).expect("second");

        let deduped = dedupe_builtin_agents(vec![first, second]);

        assert_eq!(deduped.len(), 2);
        assert_eq!(deduped[0].preset, BuiltinAgentPresetKey::Doubao);
        assert_eq!(deduped[1].preset, BuiltinAgentPresetKey::ArkCoding);
    }

    #[test]
    fn operator_and_browser_ownership_domains_are_not_deduplicated() {
        let operator = builtin_agent_from_payload(payload("model-a", None)).expect("operator");
        let mut browser = operator.clone();
        browser.id = "web-credential:builtin:browser-1".into();
        browser.set_models(["model-b"]);

        let deduped = dedupe_builtin_agents(vec![operator, browser]);

        assert_eq!(deduped.len(), 2);
        assert_eq!(deduped[0].id, "builtin-1");
        assert_eq!(deduped[0].models, ["model-a"]);
        assert_eq!(deduped[1].id, "web-credential:builtin:browser-1");
        assert_eq!(deduped[1].models, ["model-b"]);
    }
}
