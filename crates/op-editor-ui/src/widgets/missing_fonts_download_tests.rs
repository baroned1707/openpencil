//! Download / Retry / Download all affordances of the missing-font surfaces.

use crate::theme::Theme;
use crate::widgets::missing_fonts_notice::{self, MissingFontsNotice};
use crate::widgets::missing_fonts_panel::{MissingFontsHit, MissingFontsPanel};
use crate::widgets::test_capture_backend::CaptureBackend;
use crate::widgets::{missing_fonts_flow, PaintCx};
use crate::{Point2D, Rect};
use op_editor_core::font_download::{FontDownloadFailure, FontDownloadRequest, FontDownloadStatus};
use op_editor_core::missing_fonts::{MissingFontEntry, MissingFontsPrompt};
use op_editor_core::EditorState;

const VIEWPORT: (f32, f32) = (1200.0, 800.0);

fn state(families: &[&str], supported: bool) -> EditorState {
    let mut state = EditorState::new();
    state.editor_ui.missing_fonts_prompt = Some(MissingFontsPrompt {
        entries: families
            .iter()
            .map(|family| MissingFontEntry {
                family: (*family).into(),
                run_count: 2,
                mismatch_note: None,
                resolved: false,
            })
            .collect(),
    });
    state.editor_ui.missing_fonts_modal_open = true;
    state.editor_ui.font_downloads.supported = supported;
    state
}

fn viewport() -> Rect {
    Rect::xywh(0.0, 0.0, VIEWPORT.0, VIEWPORT.1)
}

fn center(rect: Rect) -> Point2D {
    Point2D::new(
        rect.origin.x + rect.size.x / 2.0,
        rect.origin.y + rect.size.y / 2.0,
    )
}

fn hit(
    state: &EditorState,
    point_of: impl Fn(&MissingFontsPanel, Rect) -> Option<Rect>,
) -> (MissingFontsHit, bool) {
    let panel = MissingFontsPanel::for_editor(state).unwrap();
    let panel_rect = panel.rect(VIEWPORT.0, VIEWPORT.1);
    let target = point_of(&panel, panel_rect);
    let point = target.map_or(Point2D::new(1.0, 1.0), center);
    (
        panel.hit_test(panel_rect, viewport(), point),
        target.is_some(),
    )
}

#[test]
fn download_pill_is_hit_per_row_and_never_steals_choose_font() {
    let state = state(&["Pacifico", "Lobster"], true);
    for row in 0..2 {
        let (download, found) = hit(&state, |panel, rect| panel.download_pill_rect(rect, row));
        assert!(found, "row {row} offers Download");
        assert_eq!(download, MissingFontsHit::DownloadFont(row));
        let (choose, _) = hit(&state, |panel, rect| Some(panel.row_button_rect(rect, row)));
        assert_eq!(choose, MissingFontsHit::ChooseFont(row));
    }
}

#[test]
fn unsupported_hosts_show_no_download_pill_or_download_all() {
    let state = state(&["Pacifico", "Lobster"], false);
    let (_, found) = hit(&state, |panel, rect| panel.download_pill_rect(rect, 0));
    assert!(!found);
    let (_, found) = hit(&state, |panel, rect| panel.download_all_rect(rect));
    assert!(!found);
}

#[test]
fn row_status_decides_whether_a_pill_is_offered() {
    let mut state = state(&["Pacifico"], true);
    let offered =
        |state: &EditorState| hit(state, |panel, rect| panel.download_pill_rect(rect, 0)).1;
    assert!(
        offered(&state),
        "unknown downloadability still offers Download"
    );
    for (status, expected) in [
        (FontDownloadStatus::Ready, true),
        (FontDownloadStatus::Downloading, false),
        (FontDownloadStatus::Downloaded, false),
        (
            FontDownloadStatus::Failed(FontDownloadFailure::Offline),
            true,
        ),
        (
            FontDownloadStatus::Failed(FontDownloadFailure::UnknownFont),
            false,
        ),
        (
            FontDownloadStatus::Failed(FontDownloadFailure::LicenseNotAllowed),
            false,
        ),
    ] {
        state
            .editor_ui
            .font_downloads
            .set_status("Pacifico", status);
        assert_eq!(offered(&state), expected, "{status:?}");
    }
}

#[test]
fn download_all_needs_two_downloadable_rows_and_raises_one_request() {
    let mut one = state(&["Pacifico"], true);
    assert!(
        !hit(&one, |p, r| p.download_all_rect(r)).1,
        "one row: the row pill is enough"
    );
    one.editor_ui
        .missing_fonts_prompt
        .as_mut()
        .unwrap()
        .entries
        .push(MissingFontEntry {
            family: "Lobster".into(),
            run_count: 1,
            mismatch_note: None,
            resolved: false,
        });
    let mut two = one;
    let (all, found) = hit(&two, |p, r| p.download_all_rect(r));
    assert!(found);
    assert_eq!(all, MissingFontsHit::DownloadAll);

    let panel = MissingFontsPanel::for_editor(&two).unwrap();
    let panel_rect = panel.rect(VIEWPORT.0, VIEWPORT.1);
    let point = center(panel.download_all_rect(panel_rect).unwrap());
    assert!(missing_fonts_flow::press(
        &mut two,
        panel_rect,
        viewport(),
        point
    ));
    assert_eq!(
        two.editor_ui.font_downloads.pending,
        vec![FontDownloadRequest::All]
    );
    // The modal stays open while the downloads run.
    assert!(two.editor_ui.missing_fonts_modal_open);

    // Once one row is already downloading only one is left: no Download all.
    two.editor_ui
        .font_downloads
        .set_status("Pacifico", FontDownloadStatus::Downloading);
    assert!(!hit(&two, |p, r| p.download_all_rect(r)).1);
}

#[test]
fn pressing_a_row_pill_queues_that_family() {
    let mut state = state(&["Pacifico", "Lobster"], true);
    let panel = MissingFontsPanel::for_editor(&state).unwrap();
    let panel_rect = panel.rect(VIEWPORT.0, VIEWPORT.1);
    let point = center(panel.download_pill_rect(panel_rect, 1).unwrap());
    assert!(missing_fonts_flow::press(
        &mut state,
        panel_rect,
        viewport(),
        point
    ));
    assert_eq!(
        state.editor_ui.font_downloads.pending,
        vec![FontDownloadRequest::Family("Lobster".into())]
    );
}

fn painted(state: &EditorState) -> CaptureBackend {
    let mut backend = CaptureBackend::default();
    let panel = MissingFontsPanel::for_editor(state).unwrap();
    let rect = panel.rect(VIEWPORT.0, VIEWPORT.1);
    {
        use crate::widgets::Widget;
        let mut cx = PaintCx {
            backend: &mut backend,
        };
        panel.paint(&mut cx, rect);
    }
    backend
}

#[test]
fn each_status_paints_its_own_line() {
    let mut state = state(&["Pacifico", "Lobster", "Helvetica Neue"], true);
    let ui = &mut state.editor_ui.font_downloads;
    ui.set_status("Pacifico", FontDownloadStatus::Downloading);
    ui.set_status("Lobster", FontDownloadStatus::Downloaded);
    ui.set_status(
        "Helvetica Neue",
        FontDownloadStatus::Failed(FontDownloadFailure::UnknownFont),
    );
    let texts: Vec<String> = painted(&state).texts.into_iter().map(|(t, _)| t).collect();
    for expected in ["Downloading…", "Downloaded", "Not a Google Fonts family"] {
        assert!(
            texts.iter().any(|t| t == expected),
            "{expected:?} in {texts:?}"
        );
    }
    // Nothing is downloadable any more: no Download all, no pills.
    assert!(!texts.iter().any(|t| t == "Download all" || t == "Download"));
}

#[test]
fn failed_offline_row_paints_the_reason_and_a_retry_pill() {
    let mut state = state(&["Pacifico"], true);
    state.editor_ui.font_downloads.set_status(
        "Pacifico",
        FontDownloadStatus::Failed(FontDownloadFailure::Offline),
    );
    let texts: Vec<String> = painted(&state).texts.into_iter().map(|(t, _)| t).collect();
    assert!(texts.iter().any(|t| t == "Offline"), "{texts:?}");
    assert!(texts.iter().any(|t| t == "Retry"), "{texts:?}");
}

#[test]
fn notice_offers_download_all_and_reports_progress() {
    let mut state = state(&["Pacifico", "Lobster"], true);
    state.editor_ui.missing_fonts_modal_open = false;
    state.editor_ui.workspace.visible = true;
    state.editor_ui.sidebar_open = false;
    let (width, height) = (1200.0, 800.0);
    let notice = MissingFontsNotice::for_editor(&state).unwrap();
    let rect = notice.rect(width, height).unwrap();
    let button = notice.download_rect(rect).expect("download button");
    assert_eq!(
        notice.hit_test(rect, center(button)),
        missing_fonts_notice::Hit::Download
    );
    assert!(missing_fonts_notice::press(
        &mut state,
        center(button),
        width,
        height
    ));
    assert_eq!(
        state.editor_ui.font_downloads.pending,
        vec![FontDownloadRequest::All]
    );

    // While rows download the message says so and the button goes away.
    for family in ["Pacifico", "Lobster"] {
        state
            .editor_ui
            .font_downloads
            .set_status(family, FontDownloadStatus::Downloading);
    }
    let notice = MissingFontsNotice::for_editor(&state).unwrap();
    assert!(notice.download_rect(rect).is_none());
    let mut backend = CaptureBackend::default();
    notice.paint(
        &mut PaintCx {
            backend: &mut backend,
        },
        rect,
    );
    assert!(backend
        .texts
        .iter()
        .any(|(t, _)| t == "Downloading missing fonts…"));
    let _ = Theme::dark();
}

mod settings_fonts_tab {
    use super::*;
    use crate::widgets::agent_settings_fonts::{
        content_height, hit_test, missing_row_rect, paint_fonts_tab, FontsHit,
    };
    use crate::widgets::agent_settings_panel::AgentSettingsHit;
    use crate::widgets::agent_settings_press_flow::apply_agent_settings_hit;
    use op_editor_core::host_settings_commit::SettingsCommitScope;

    fn apply_settings_hit_for_tests(state: &mut EditorState, hit: AgentSettingsHit) {
        apply_agent_settings_hit(state, hit, SettingsCommitScope::Operator, 0);
    }

    fn content() -> Rect {
        Rect::xywh(0.0, 0.0, 600.0, 900.0)
    }

    #[test]
    fn switch_row_exists_only_where_downloading_is_supported() {
        let off = state(&["Pacifico"], false);
        let on = state(&["Pacifico"], true);
        assert!(content_height(&on.editor_ui) > content_height(&off.editor_ui));

        let find_toggle = |state: &EditorState| -> bool {
            // Sweep the top band of the tab for the switch.
            (0..120).any(|y| {
                (0..600).step_by(4).any(|x| {
                    hit_test(
                        content(),
                        content(),
                        &state.editor_ui,
                        Point2D::new(x as f32, y as f32),
                        0.0,
                    ) == FontsHit::ToggleAutoDownload
                })
            })
        };
        assert!(find_toggle(&on));
        assert!(!find_toggle(&off));

        let mut backend = CaptureBackend::default();
        paint_fonts_tab(
            &mut PaintCx {
                backend: &mut backend,
            },
            &Theme::dark(),
            &on.editor_ui,
            content(),
        );
        let texts: Vec<_> = backend.texts.iter().map(|(t, _)| t.as_str()).collect();
        assert!(
            texts.contains(&"Download missing fonts automatically"),
            "{texts:?}"
        );
        assert!(texts
            .iter()
            .any(|t| t.contains("Fontsource") && t.contains("GitHub")));
    }

    #[test]
    fn pressing_the_switch_flips_the_setting_and_a_row_pill_queues_a_download() {
        let mut state = state(&["Pacifico"], true);
        assert!(state.editor_ui.agent_settings.auto_download_missing_fonts);
        apply_settings_hit_for_tests(
            &mut state,
            AgentSettingsHit::Fonts(FontsHit::ToggleAutoDownload),
        );
        assert!(!state.editor_ui.agent_settings.auto_download_missing_fonts);
        apply_settings_hit_for_tests(
            &mut state,
            AgentSettingsHit::Fonts(FontsHit::ToggleAutoDownload),
        );
        assert!(state.editor_ui.agent_settings.auto_download_missing_fonts);

        // The row pill is hit-testable and routes to the shared request.
        let row = missing_row_rect(content(), &state.editor_ui, 0);
        let entry = state
            .editor_ui
            .missing_fonts_prompt
            .as_ref()
            .unwrap()
            .entries[0]
            .clone();
        let pill =
            crate::widgets::missing_fonts_row::row_download_rect(row, &state.editor_ui, &entry)
                .unwrap();
        let found = hit_test(content(), content(), &state.editor_ui, center(pill), 0.0);
        assert_eq!(found, FontsHit::DownloadFont(0));
        apply_settings_hit_for_tests(&mut state, AgentSettingsHit::Fonts(found));
        assert_eq!(
            state.editor_ui.font_downloads.pending,
            vec![FontDownloadRequest::Family("Pacifico".into())]
        );
    }
}

#[test]
fn the_notice_button_fits_its_label_in_every_locale() {
    for locale in op_i18n::Locale::ALL {
        let mut state = state(&["Pacifico", "Lobster"], true);
        state.editor_ui.missing_fonts_modal_open = false;
        state.editor_ui.workspace.visible = true;
        state.editor_ui.sidebar_open = false;
        state.editor_ui.locale = locale;
        let notice = MissingFontsNotice::for_editor(&state).unwrap();
        let rect = notice.rect(1200.0, 800.0).unwrap();
        let button = notice.download_rect(rect).expect("download button");
        let label = crate::widgets::editor_state_ext::translate(
            &state.editor_ui,
            "missingFonts.downloadAll",
        );
        assert!(
            crate::widgets::missing_fonts_row::fit_button_width(label, 12.0)
                <= button.size.x + 0.01,
            "{locale:?}: {label}"
        );
        // The button never overlaps Manage or runs off the notice.
        assert!(button.origin.x + button.size.x <= MissingFontsNotice::manage_rect(rect).origin.x);
        assert!(
            button.origin.x > rect.origin.x + 100.0,
            "{locale:?}: message keeps room"
        );
    }
}
