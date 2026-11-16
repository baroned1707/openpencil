//! Proof scenes for the Figma UI3 restyle — document tab strip.
//!
//! Seeds `editor_ui.document_tabs` directly (the registry that normally fills
//! it lives in the document swap), then paints the real editor frame in both
//! themes so `.proof-of-work/70-tabs-*.png` shows the strip in context.

use super::super::ui3_proof::{editor_host, render_probes, rgb8, PROOF_W};
use crate::widget_host::WidgetHostNative;
use op_editor_core::{DocumentId, DocumentTabStatus, DocumentTabView, ThemeMode, TopBarButton};
use op_editor_ui::widgets::{TopBar, TOP_BAR_HEIGHT};
use op_editor_ui::{Point2D, Rect, Theme};

const BOTH: [ThemeMode; 2] = [ThemeMode::Dark, ThemeMode::Light];

fn tab(
    n: u64,
    label: &str,
    project: &str,
    status: DocumentTabStatus,
    dirty: bool,
    active: bool,
) -> DocumentTabView {
    DocumentTabView {
        id: DocumentId(n),
        label: label.to_string(),
        project: project.to_string(),
        dirty,
        status,
        active,
    }
}

fn seed(host: &mut WidgetHostNative, rows: Vec<DocumentTabView>) {
    let ui = &mut host.editor_state_mut().editor_ui;
    ui.document_tabs_supported = true;
    ui.document_tabs = rows;
}

/// A point inside tab `index` when it is on screen (an overflowing strip
/// shows only a window of its tabs), from the anchor hover and tooltips use.
fn inside_tab(host: &WidgetHostNative, index: usize) -> Option<(i32, i32)> {
    let bar = TopBar::for_editor_ui(&host.editor_state().editor_ui);
    let bar_rect = Rect::xywh(0.0, 0.0, PROOF_W, TOP_BAR_HEIGHT);
    let rect = bar.button_rect(bar_rect, TopBarButton::DocumentTab(index))?;
    let at = Point2D::new(rect.origin.x + 4.0, rect.origin.y + 3.0);
    Some((at.x as i32, at.y as i32))
}

fn theme_of(mode: ThemeMode) -> Theme {
    match mode {
        ThemeMode::Dark => Theme::dark(),
        ThemeMode::Light => Theme::light(),
    }
}

/// Render `name` in both themes; returns the pixel probes `(active tab,
/// inactive tab)` per theme for the caller's assertions.
fn each_theme(
    name: &str,
    rows: &[DocumentTabView],
    prepare: impl Fn(&mut WidgetHostNative),
) -> Vec<(ThemeMode, [u8; 3], [u8; 3])> {
    BOTH.iter()
        .map(|&mode| {
            let mut host = editor_host(mode, Some("card"));
            seed(&mut host, rows.to_vec());
            prepare(&mut host);
            let active = rows.iter().position(|tab| tab.active).unwrap_or(0);
            let inactive = (0..rows.len())
                .find(|&index| index != active && inside_tab(&host, index).is_some())
                .expect("an inactive tab is on screen");
            let probes = [
                inside_tab(&host, active).expect("the active tab is on screen"),
                inside_tab(&host, inactive).unwrap(),
            ];
            let px = render_probes(&mut host, name, &probes);
            (mode, px[0], px[1])
        })
        .collect()
}

#[test]
fn three_tabs_one_project_proof() {
    let rows = [
        tab(1, "dashboard.op", "", DocumentTabStatus::Idle, false, true),
        tab(
            2,
            "landing-page.op",
            "",
            DocumentTabStatus::Idle,
            false,
            false,
        ),
        tab(3, "icons.op", "", DocumentTabStatus::Idle, false, false),
    ];
    for (mode, active, inactive) in each_theme("70-tabs-three", &rows, |_| {}) {
        let theme = theme_of(mode);
        assert_eq!(active, rgb8(theme.row_selected), "{mode:?}: active fill");
        assert_eq!(
            inactive,
            rgb8(theme.background),
            "{mode:?}: inactive is flat"
        );
    }
}

#[test]
fn two_projects_with_status_and_unsaved_proof() {
    let rows = [
        tab(
            1,
            "checkout.op",
            "storefront",
            DocumentTabStatus::Running,
            false,
            true,
        ),
        tab(
            2,
            "cart.op",
            "storefront",
            DocumentTabStatus::NeedsReview,
            true,
            false,
        ),
        tab(
            3,
            "product-card.op",
            "storefront",
            DocumentTabStatus::Idle,
            false,
            false,
        ),
        tab(
            4,
            "onboarding.op",
            "mobile-app",
            DocumentTabStatus::Done,
            false,
            false,
        ),
        tab(
            5,
            "settings.op",
            "mobile-app",
            DocumentTabStatus::Error,
            false,
            false,
        ),
    ];
    each_theme("70-tabs-projects", &rows, |_| {});
}

#[test]
fn many_tabs_overflow_proof() {
    let rows: Vec<_> = (0..14)
        .map(|n| {
            tab(
                n + 1,
                &format!("screen-{:02}.op", n + 1),
                if n < 8 { "storefront" } else { "mobile-app" },
                DocumentTabStatus::Idle,
                n == 3,
                n == 9,
            )
        })
        .collect();
    each_theme("70-tabs-overflow", &rows, |_| {});
}

#[test]
fn hover_with_close_button_proof() {
    let rows = [
        tab(1, "dashboard.op", "", DocumentTabStatus::Idle, false, true),
        tab(
            2,
            "landing-page.op",
            "",
            DocumentTabStatus::NeedsReview,
            true,
            false,
        ),
        tab(3, "icons.op", "", DocumentTabStatus::Idle, false, false),
    ];
    let results = each_theme("70-tabs-hover", &rows, |host| {
        host.editor_state_mut().editor_ui.topbar_button_hover =
            Some(TopBarButton::CloseDocumentTab(1));
    });
    assert_eq!(results.len(), 2);
}
