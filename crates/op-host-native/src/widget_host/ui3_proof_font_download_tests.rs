//! Proof scenes for the missing-font downloader: the modal with rows in each
//! download state, the footer "Download all" with its hover, the workspace
//! notice, and the Settings > Fonts switch.
//!
//! Writes `.proof-of-work/96-<scene>-<theme>.png` when `OP_PROOF_DIR` is set.

use super::super::ui3_proof::{editor_host, render_scene};
use crate::widget_host::WidgetHostNative;
use op_editor_core::agent_settings::AgentSettingsTab;
use op_editor_core::font_download::{FontDownloadFailure, FontDownloadStatus};
use op_editor_core::missing_fonts::{MissingFontEntry, MissingFontsHover, MissingFontsPrompt};
use op_editor_core::ThemeMode;
use op_editor_ui::widgets::{MissingFontsHit, MissingFontsPanel};
use op_editor_ui::Point2D;

const BOTH: [ThemeMode; 2] = [ThemeMode::Dark, ThemeMode::Light];

fn entry(family: &str, run_count: u32) -> MissingFontEntry {
    MissingFontEntry {
        family: family.into(),
        run_count,
        mismatch_note: None,
        resolved: false,
    }
}

/// A host whose document has these missing families (desktop capabilities on).
fn host_with_rows(theme: ThemeMode, families: &[(&str, u32)]) -> WidgetHostNative {
    let mut host = editor_host(theme, None);
    let ui = &mut host.editor_state_mut().editor_ui;
    ui.font_downloads.supported = true;
    ui.font_import_supported = true;
    ui.system_fonts_loaded = true;
    ui.missing_fonts_prompt = Some(MissingFontsPrompt {
        entries: families
            .iter()
            .map(|(family, uses)| entry(family, *uses))
            .collect(),
    });
    host
}

#[test]
fn modal_rows_show_downloading_downloaded_and_failed_proof() {
    for mode in BOTH {
        let mut host = host_with_rows(
            mode,
            &[("Pacifico", 14), ("Lobster Two", 6), ("Helvetica Neue", 3)],
        );
        let ui = &mut host.editor_state_mut().editor_ui;
        ui.missing_fonts_modal_open = true;
        ui.font_downloads
            .set_status("Pacifico", FontDownloadStatus::Downloading);
        ui.font_downloads
            .set_status("Lobster Two", FontDownloadStatus::Downloaded);
        ui.font_downloads.set_status(
            "Helvetica Neue",
            FontDownloadStatus::Failed(FontDownloadFailure::UnknownFont),
        );
        let pixel = render_scene(&mut host, "96-missing-fonts-states", (720, 450));
        assert_ne!(pixel, [0, 0, 0], "{mode:?} painted nothing");
    }
}

#[test]
fn modal_download_all_hover_proof() {
    for mode in BOTH {
        let mut host = host_with_rows(mode, &[("Pacifico", 14), ("Lobster Two", 6), ("Caveat", 3)]);
        host.editor_state_mut().editor_ui.missing_fonts_modal_open = true;
        // Hover the footer button through the real hit test, like a cursor move.
        let (width, height) = (
            super::super::ui3_proof::PROOF_W,
            super::super::ui3_proof::PROOF_H,
        );
        let state = host.editor_state();
        let panel = MissingFontsPanel::for_editor(state).expect("modal");
        let panel_rect = panel.rect(width, height);
        let button = panel.download_all_rect(panel_rect).expect("Download all");
        let point = Point2D::new(
            button.origin.x + button.size.x / 2.0,
            button.origin.y + button.size.y / 2.0,
        );
        let viewport = op_editor_ui::Rect::xywh(0.0, 0.0, width, height);
        assert_eq!(
            panel.hit_test(panel_rect, viewport, point),
            MissingFontsHit::DownloadAll
        );
        host.editor_state_mut().editor_ui.missing_fonts_hover =
            Some(MissingFontsHover::DownloadAll);
        let pixel = render_scene(&mut host, "96-missing-fonts-download-all-hover", (720, 450));
        assert_ne!(pixel, [0, 0, 0], "{mode:?} painted nothing");
    }
}

#[test]
fn workspace_notice_offers_download_all_proof() {
    for mode in BOTH {
        let mut host = host_with_rows(mode, &[("Pacifico", 14), ("Lobster Two", 6), ("Caveat", 3)]);
        let ui = &mut host.editor_state_mut().editor_ui;
        ui.workspace.visible = true;
        ui.missing_fonts_modal_open = false;
        render_scene(&mut host, "96-missing-fonts-notice", (720, 450));
        for family in ["Pacifico", "Lobster Two", "Caveat"] {
            host.editor_state_mut()
                .editor_ui
                .font_downloads
                .set_status(family, FontDownloadStatus::Downloading);
        }
        render_scene(&mut host, "96-missing-fonts-notice-downloading", (720, 450));
    }
}

#[test]
fn settings_fonts_tab_with_the_auto_download_switch_proof() {
    for mode in BOTH {
        let mut host = host_with_rows(mode, &[("Pacifico", 14), ("Helvetica Neue", 3)]);
        let ui = &mut host.editor_state_mut().editor_ui;
        ui.agent_settings_open = true;
        ui.agent_settings.tab = AgentSettingsTab::Fonts;
        ui.agent_settings.auto_download_missing_fonts = true;
        ui.font_downloads.set_status(
            "Helvetica Neue",
            FontDownloadStatus::Failed(FontDownloadFailure::UnknownFont),
        );
        let pixel = render_scene(&mut host, "96-settings-fonts-auto-on", (720, 450));
        assert_ne!(pixel, [0, 0, 0], "{mode:?} painted nothing");
        host.editor_state_mut()
            .editor_ui
            .agent_settings
            .auto_download_missing_fonts = false;
        render_scene(&mut host, "96-settings-fonts-auto-off", (720, 450));
    }
}

/// Long translations at the real font's widths: the switch row, its privacy
/// hint and the notice button must not collide or clip in these locales.
#[test]
fn long_locales_keep_the_switch_row_and_notice_readable_proof() {
    for (locale, tag) in [
        (op_editor_core::Locale::Ru, "ru"),
        (op_editor_core::Locale::De, "de"),
        (op_editor_core::Locale::Vi, "vi"),
        (op_editor_core::Locale::Ja, "ja"),
    ] {
        let mut host = host_with_rows(
            ThemeMode::Dark,
            &[("Pacifico", 14), ("Lobster Two", 6), ("Caveat", 3)],
        );
        let ui = &mut host.editor_state_mut().editor_ui;
        ui.locale = locale;
        ui.agent_settings_open = true;
        ui.agent_settings.tab = AgentSettingsTab::Fonts;
        render_scene(&mut host, &format!("96-settings-fonts-{tag}"), (720, 450));
        let ui = &mut host.editor_state_mut().editor_ui;
        ui.agent_settings_open = false;
        ui.workspace.visible = true;
        render_scene(
            &mut host,
            &format!("96-missing-fonts-notice-{tag}"),
            (720, 450),
        );
    }
}
