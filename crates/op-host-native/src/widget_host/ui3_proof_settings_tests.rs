//! Proof scenes for the Figma UI3 restyle — settings wave.
//!
//! One `#[test]` per settings surface; each calls `ui3_proof::render_scene`
//! once per theme so `.proof-of-work/` gets a dark and a light PNG named
//! `60-settings-<scene>-<theme>.png`.

use crate::widget_host::ui3_proof::{editor_host, render_scene};
use crate::widget_host::WidgetHostNative;
use op_editor_core::agent_settings::{
    AgentSettingsTab, BuiltinAgentField, BuiltinAgentPresetMenuTarget, ImageGenField,
    ImageSearchField, ProviderConnectPhase, SettingsFocus,
};
use op_editor_core::ThemeMode;

const THEMES: [ThemeMode; 2] = [ThemeMode::Dark, ThemeMode::Light];

/// Host with the settings modal open on `tab`, in the given theme.
fn settings_host(theme: ThemeMode, tab: AgentSettingsTab) -> WidgetHostNative {
    let mut host = editor_host(theme, None);
    let ui = &mut host.editor_state_mut().editor_ui;
    ui.agent_settings_open = true;
    ui.agent_settings.tab = tab;
    host
}

/// Render the scene in both themes; `setup` mutates the host after the
/// modal is open. The modal paints over the scrim, so the probe samples a
/// pixel inside the modal body.
fn render_both(name: &str, tab: AgentSettingsTab, setup: impl Fn(&mut WidgetHostNative)) {
    for theme in THEMES {
        let mut host = settings_host(theme, tab);
        setup(&mut host);
        let pixel = render_scene(&mut host, &format!("60-settings-{name}"), (720, 450));
        assert_ne!(pixel, [0, 0, 0], "{name} {theme:?} painted nothing");
    }
}

fn connect_provider(host: &mut WidgetHostNative, index: usize) {
    let settings = &mut host.editor_state_mut().editor_ui.agent_settings;
    settings.connected[index] = true;
    settings.provider_connection[index].phase = ProviderConnectPhase::Connected;
    settings.provider_connection[index].info = Some("Connected via Pro".to_string());
}

#[test]
fn settings_agents_connected_hovered() {
    render_both("agents-connected", AgentSettingsTab::Agents, |host| {
        let settings = &mut host.editor_state_mut().editor_ui.agent_settings;
        settings.add_builtin_agent_with_defaults("Anthropic", "sk-ant-demo", "claude-sonnet");
        settings.hover_provider = 1;
        connect_provider(host, 0);
        connect_provider(host, 1);
    });
}

#[test]
fn settings_agents_empty() {
    render_both("agents-empty", AgentSettingsTab::Agents, |_| {});
}

#[test]
fn settings_mcp_tab() {
    render_both("mcp", AgentSettingsTab::Mcp, |host| {
        let settings = &mut host.editor_state_mut().editor_ui.agent_settings;
        settings.mcp_cli_enabled[0] = true;
        settings.mcp_cli_enabled[2] = true;
    });
}

#[test]
fn settings_images_advanced() {
    render_both("images-advanced", AgentSettingsTab::Images, |host| {
        let settings = &mut host.editor_state_mut().editor_ui.agent_settings;
        settings.images_advanced_open = true;
        settings.openverse_client_id = "demo-client".to_string();
    });
}

#[test]
fn settings_system_tab() {
    render_both("system", AgentSettingsTab::System, |_| {});
}

#[test]
fn settings_fonts_tab() {
    render_both("fonts", AgentSettingsTab::Fonts, |_| {});
}

#[test]
fn settings_builtin_provider_form() {
    render_both("builtin-form", AgentSettingsTab::Agents, |host| {
        let settings = &mut host.editor_state_mut().editor_ui.agent_settings;
        settings.add_builtin_agent_with_defaults("Anthropic", "sk-ant-demo", "claude-sonnet");
        settings.focus = Some(SettingsFocus::BuiltinAgent {
            index: 0,
            field: BuiltinAgentField::ApiKey,
        });
    });
}

#[test]
fn settings_account_signed_out() {
    render_both("account-out", AgentSettingsTab::Account, |host| {
        host.editor_state_mut().editor_ui.account_ui_available = true;
    });
}

#[test]
fn settings_account_signed_in() {
    render_both("account-in", AgentSettingsTab::Account, |host| {
        let ui = &mut host.editor_state_mut().editor_ui;
        ui.account_ui_available = true;
        ui.account = op_editor_core::AccountState::SignedIn {
            display_name: "Ada Lovelace".to_string(),
            username: "ada".to_string(),
        };
    });
}

#[test]
fn settings_nav_hover() {
    render_both("nav-hover", AgentSettingsTab::Mcp, |host| {
        host.editor_state_mut().editor_ui.agent_settings.hover_nav = Some(AgentSettingsTab::Images);
    });
}

#[test]
fn settings_images_field_focused() {
    render_both("images-focus", AgentSettingsTab::Images, |host| {
        let settings = &mut host.editor_state_mut().editor_ui.agent_settings;
        settings.images_advanced_open = true;
        settings.focus = Some(SettingsFocus::ImageSearch(ImageSearchField::ClientId));
    });
}

#[test]
fn settings_mcp_port_focused() {
    render_both("mcp-port-focus", AgentSettingsTab::Mcp, |host| {
        host.editor_state_mut().editor_ui.agent_settings.focus = Some(SettingsFocus::McpPort);
    });
}

#[test]
fn settings_images_profile_editing() {
    render_both("images-profile", AgentSettingsTab::Images, |host| {
        let settings = &mut host.editor_state_mut().editor_ui.agent_settings;
        settings.add_image_gen_profile();
        settings.add_image_gen_profile();
        settings.focus = Some(SettingsFocus::ImageGenProfile {
            index: 0,
            field: ImageGenField::ApiKey,
        });
    });
}

#[test]
fn settings_agents_acp_and_preset_menu() {
    render_both("agents-acp-menu", AgentSettingsTab::Agents, |host| {
        let settings = &mut host.editor_state_mut().editor_ui.agent_settings;
        settings.add_acp_agent_preset("gemini-cli");
        settings.add_builtin_agent_with_defaults("Anthropic", "sk-ant-demo", "claude-sonnet");
        settings.focus = Some(SettingsFocus::BuiltinAgent {
            index: 0,
            field: BuiltinAgentField::ApiKey,
        });
        settings.builtin_preset_menu_open = Some(BuiltinAgentPresetMenuTarget::Agent(0));
    });
}

#[test]
fn settings_agents_acp_rows_hovered() {
    render_both("agents-acp", AgentSettingsTab::Agents, |host| {
        let settings = &mut host.editor_state_mut().editor_ui.agent_settings;
        settings.add_acp_agent_preset("gemini-cli");
        settings.add_acp_agent_preset("kimi");
        settings.add_builtin_agent_with_defaults("Anthropic", "sk-ant-demo", "claude-sonnet");
        settings.hover_acp_agent = 1;
        settings.hover_builtin_agent = usize::MAX;
    });
}
