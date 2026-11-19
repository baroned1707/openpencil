//! Proof scenes for the Settings > Images "Stock photo providers" block:
//! the Pixabay, Unsplash and Pexels key rows (masked), their hints and
//! links, the Test statuses, and the opt-in Unsplash and Pexels switches off
//! and on.
//!
//! Writes `.proof-of-work/97-images-<scene>-<theme>.png` when `OP_PROOF_DIR`
//! is set. The block sits at the bottom of the tab, so the scenes scroll the
//! pane to its end.

use super::super::ui3_proof::{editor_host, render_scene};
use op_editor_core::agent_settings::{AgentSettingsTab, ImageTestStatus};
use op_editor_core::ThemeMode;

const BOTH: [ThemeMode; 2] = [ThemeMode::Dark, ThemeMode::Light];

fn settings_host(theme: ThemeMode) -> crate::widget_host::WidgetHostNative {
    let mut host = editor_host(theme, None);
    let ui = &mut host.editor_state_mut().editor_ui;
    ui.agent_settings_open = true;
    ui.agent_settings.tab = AgentSettingsTab::Images;
    // Far past the end: the pane clamps to its real bottom.
    ui.agent_settings.scroll_y.offset = 10_000.0;
    host
}

#[test]
fn stock_provider_rows_proof() {
    for mode in BOTH {
        let mut host = settings_host(mode);
        {
            let settings = &mut host.editor_state_mut().editor_ui.agent_settings;
            settings.pixabay_api_key = "px-demo-key-not-real".into();
            settings.pixabay_test_status = ImageTestStatus::Valid;
        }
        let pixel = render_scene(&mut host, "97-images-stock-pixabay-set", (720, 450));
        assert_ne!(pixel, [0, 0, 0], "{mode:?} painted nothing");

        {
            let settings = &mut host.editor_state_mut().editor_ui.agent_settings;
            settings.unsplash_access_key = "us-demo-key-not-real".into();
            settings.unsplash_enabled = true;
            settings.unsplash_test_status = ImageTestStatus::Invalid;
        }
        render_scene(&mut host, "97-images-stock-unsplash-on", (720, 450));

        {
            let settings = &mut host.editor_state_mut().editor_ui.agent_settings;
            settings.pexels_api_key = "pe-demo-key-not-real".into();
            settings.pexels_enabled = true;
            settings.pexels_test_status = ImageTestStatus::Valid;
        }
        render_scene(&mut host, "97-images-stock-pexels-on", (720, 450));
    }
}
