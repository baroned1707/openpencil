//! Change detection for the settings file: a cheap snapshot of every
//! persisted field, captured before each dispatch and compared after.
//! Split out of `settings_io.rs` (800-line ceiling); pure code motion.

use super::{selected_chat_agent_name, selected_chat_model_value};
use op_editor_core::editor_ui_state::RecentFile;
use op_editor_core::{
    AcpAgentConfig, BuiltinAgentConfig, EditorState, ImageGenProfile, SessionDocument, ThemeMode,
};
use op_i18n::Locale;

/// Cheap snapshot of every persisted field. Captured before each
/// dispatch; if it differs after, save the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fingerprint {
    recent_files: Vec<RecentFile>,
    session_documents: Vec<SessionDocument>,
    open_projects: Vec<String>,
    theme: ThemeMode,
    locale: Locale,
    port: u16,
    cli: [bool; 13],
    mcp_lean_profile: bool,
    mcp_require_token: bool,
    images_adv: bool,
    openverse_client_id: String,
    openverse_client_secret: String,
    openverse_credential_owner: Option<String>,
    auto_update_enabled: bool,
    experimental_features_enabled: bool,
    connected: [bool; 8],
    builtin_agents: Vec<BuiltinAgentConfig>,
    acp_agents: Vec<AcpAgentConfig>,
    image_gen_profiles: Vec<ImageGenProfile>,
    active_image_gen_profile_id: Option<String>,
    image_gen_enabled: bool,
    preferred_agent_team_size: u32,
    entry_surface: op_editor_core::EntrySurface,
    chat_agent: String,
    chat_model: String,
}

pub fn fingerprint(state: &EditorState) -> Fingerprint {
    let eui = &state.editor_ui;
    Fingerprint {
        recent_files: eui.recent_files.clone(),
        session_documents: eui.session_documents.clone(),
        open_projects: eui.open_projects.clone(),
        theme: eui.theme_mode,
        locale: eui.locale,
        port: eui.agent_settings.mcp_server.port,
        cli: eui.agent_settings.mcp_cli_enabled,
        mcp_lean_profile: eui.agent_settings.mcp_lean_profile,
        mcp_require_token: eui.agent_settings.mcp_require_token,
        images_adv: eui.agent_settings.images_advanced_open,
        openverse_client_id: eui.agent_settings.openverse_client_id.clone(),
        openverse_client_secret: eui.agent_settings.openverse_client_secret.clone(),
        openverse_credential_owner: eui.agent_settings.openverse_credential_owner.clone(),
        auto_update_enabled: eui.agent_settings.auto_update_enabled,
        experimental_features_enabled: eui.agent_settings.experimental_features_enabled,
        connected: eui.agent_settings.connected,
        builtin_agents: eui.agent_settings.builtin_agents.clone(),
        acp_agents: eui.agent_settings.acp_agents.clone(),
        image_gen_profiles: eui.agent_settings.image_gen_profiles.clone(),
        active_image_gen_profile_id: eui.agent_settings.active_image_gen_profile_id.clone(),
        image_gen_enabled: eui.agent_settings.image_gen_enabled,
        preferred_agent_team_size: eui.preferred_agent_team_size,
        entry_surface: eui.entry_surface,
        chat_agent: selected_chat_agent_name(eui),
        chat_model: selected_chat_model_value(state),
    }
}
