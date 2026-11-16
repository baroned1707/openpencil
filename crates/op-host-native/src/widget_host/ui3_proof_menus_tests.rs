//! Proof scenes for the Figma UI3 restyle — menus wave.
//!
//! Add one `#[test]` per surface; each calls `ui3_proof::render_scene` once
//! per theme so `.proof-of-work/` gets a dark and a light PNG.

use super::super::ui3_proof::{editor_host, render_scene};
use crate::widget_host::WidgetHostNative;
use op_editor_core::ThemeMode;

const BOTH: [ThemeMode; 2] = [ThemeMode::Dark, ThemeMode::Light];

/// Render `name` once per theme after `open` has configured the scene.
fn each_theme(name: &str, selected: Option<&str>, open: impl Fn(&mut WidgetHostNative)) {
    for theme in BOTH {
        let mut host = editor_host(theme, selected);
        open(&mut host);
        let rgb = render_scene(&mut host, name, (10, 10));
        assert!(rgb != [0, 0, 0], "{theme:?}: {name} painted something");
    }
}

#[test]
fn file_menu_proof() {
    each_theme("30-menu-file", Some("card"), |host| {
        let ui = &mut host.editor_state_mut().editor_ui;
        ui.file_menu_open = true;
        ui.file_menu.hover = Some(3);
    });
}

#[test]
fn import_menu_proof() {
    each_theme("30-menu-import", None, |host| {
        let ui = &mut host.editor_state_mut().editor_ui;
        ui.import_menu_open = true;
        ui.import_menu.open = true;
        ui.import_menu.hover = Some(1);
    });
}

#[test]
fn export_quick_menu_proof() {
    each_theme("30-menu-export-quick", Some("card"), |host| {
        let ui = &mut host.editor_state_mut().editor_ui;
        ui.export_quick_menu_open = true;
        ui.export_quick_menu_hover = Some(op_editor_core::ExportQuickRow::Image);
    });
}

#[test]
fn export_dialog_proof() {
    each_theme("30-menu-export-dialog", Some("card"), |host| {
        host.editor_state_mut().editor_ui.export_dialog_open = true;
        host.editor_state_mut().editor_ui.export_dialog_hover =
            Some(op_editor_core::ExportDialogButton::Export);
    });
}

#[test]
fn account_menu_proof() {
    each_theme("30-menu-account", None, |host| {
        let ui = &mut host.editor_state_mut().editor_ui;
        ui.account_ui_available = true;
        ui.account = op_editor_core::AccountState::dev_fake_signed_in();
        ui.account_menu_open = true;
    });
}

#[test]
fn locale_picker_proof() {
    each_theme("30-menu-locale", None, |host| {
        let ui = &mut host.editor_state_mut().editor_ui;
        ui.locale_picker.open = true;
        ui.locale_picker.hover = Some(2);
    });
}

#[test]
fn layer_context_menu_proof() {
    use op_editor_core::editor_ui_state::{LayerContextMenuState, LayerContextTarget};
    each_theme("30-menu-layer-context", Some("card"), |host| {
        let mut state = LayerContextMenuState {
            target: LayerContextTarget::Layer(op_editor_core::NodeId::new("card")),
            anchor_x: 120.0,
            anchor_y: 230.0,
            menu: Default::default(),
        };
        state.menu.hover = Some(1);
        host.editor_state_mut().editor_ui.layer_context_menu = Some(state);
    });
}

#[test]
fn color_picker_proof() {
    each_theme("30-menu-color-picker", Some("card"), |host| {
        assert!(host
            .editor_state_mut()
            .open_color_picker(op_editor_core::ui_draft::ColorTarget::Fill, 300.0));
    });
}

#[test]
fn save_name_dialog_proof() {
    each_theme("30-menu-save-name", None, |host| {
        host.editor_state_mut()
            .editor_ui
            .save_name_dialog
            .open_with("Dashboard", false, 0);
    });
}

#[test]
fn login_modal_proof() {
    each_theme("30-menu-login", None, |host| {
        let ui = &mut host.editor_state_mut().editor_ui;
        ui.account_ui_available = true;
        ui.login_modal_open = true;
    });
}

#[test]
fn missing_fonts_proof() {
    use op_editor_core::missing_fonts::{MissingFontEntry, MissingFontsPrompt};
    each_theme("30-menu-missing-fonts", None, |host| {
        let entry = |family: &str, resolved: bool| MissingFontEntry {
            family: family.to_string(),
            run_count: 3,
            mismatch_note: None,
            resolved,
        };
        let ui = &mut host.editor_state_mut().editor_ui;
        ui.missing_fonts_prompt = Some(MissingFontsPrompt {
            entries: vec![
                entry("Söhne", false),
                entry("Canela Deck", false),
                entry("GT America", true),
            ],
        });
        ui.missing_fonts_modal_open = true;
    });
}

#[test]
fn html_import_diagnostics_proof() {
    use op_editor_core::html_import_diagnostics::HtmlImportDiagnostic;
    each_theme("30-menu-html-diagnostics", None, |host| {
        let diag = |code: &str| HtmlImportDiagnostic {
            code: code.to_string(),
            i18n_key: format!("htmlImport.diagnostics.{code}"),
            args: Vec::new(),
            message: format!("Imported with a fallback: {code}"),
        };
        let ui = &mut host.editor_state_mut().editor_ui;
        ui.html_import_diagnostics = vec![
            diag("layout.flex_wrap_node_limit"),
            diag("css.unsupported_filter"),
            diag("css.unsupported_blend"),
        ];
        ui.html_import_diagnostics_total = 3;
        ui.html_import_diagnostics_open = true;
        ui.html_import_diagnostics_expanded = true;
    });
}

#[test]
fn mobile_more_panel_proof() {
    use op_editor_core::size_class::{EditorSizeClass, MobileSheetKind};
    each_theme("30-menu-mobile-more", None, |host| {
        let ui = &mut host.editor_state_mut().editor_ui;
        ui.touch = true;
        ui.size_class = EditorSizeClass::Expanded;
        ui.mobile_sheet = Some(MobileSheetKind::More);
    });
}

#[test]
fn model_menu_proof() {
    use op_editor_core::agent_settings::{
        BuiltinAgentField, BuiltinAgentKind, BuiltinModelMenuTarget, SettingsFocus,
    };
    use op_editor_core::{
        BuiltinModelCatalogRefreshOutcome, BuiltinModelCatalogTarget, BuiltinModelOption,
    };
    each_theme("30-menu-model", None, |host| {
        let ui = &mut host.editor_state_mut().editor_ui;
        ui.agent_settings_open = true;
        let settings = &mut ui.agent_settings;
        let id = settings.add_builtin_agent_config(
            "Provider",
            "sk-test",
            "model-0",
            BuiltinAgentKind::OpenAiCompat,
            "https://api.example.com/v1",
        );
        let request = settings
            .begin_builtin_model_catalog_refresh(BuiltinModelCatalogTarget::Agent(id), 1)
            .expect("configured provider starts discovery");
        let expected = settings
            .builtin_model_catalog_config_for_request(&request)
            .expect("current discovery config");
        settings.take_pending_builtin_model_catalog_refresh();
        assert!(
            settings.apply_builtin_model_catalog_refresh_outcome_if_current(
                &expected,
                &request,
                BuiltinModelCatalogRefreshOutcome::Success {
                    models: (0..6)
                        .map(|i| BuiltinModelOption::new(
                            format!("model-{i}"),
                            format!("Model {i}")
                        ))
                        .collect(),
                },
            )
        );
        settings.focus = Some(SettingsFocus::BuiltinAgent {
            index: 0,
            field: BuiltinAgentField::Model,
        });
        settings.builtin_model_menu_open = Some(BuiltinModelMenuTarget::Agent(0));
        settings.builtin_model_menu_hover = Some(1);
        ui.settings_input.set_text("model-0");
    });
}
