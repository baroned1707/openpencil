//! Host wiring of the info bar: every item is hovered, tooltipped and pressed
//! through the real cursor and press paths, the page menu switches pages with a
//! re-fit, and the Git popover stays anchored on its button while the page chip
//! is shown.
//!
//! Windows-gated like the other tests that solve layout.

#![cfg(all(test, not(target_os = "windows")))]

use crate::widget_host::WidgetHostNative;
use op_editor_core::{
    DocumentId, DocumentTabRequest, DocumentTabStatus, DocumentTabView, ProjectFileKind,
    ProjectFileView, ProjectRequest, ProjectView, TopBarButton,
};
use op_editor_ui::widgets::top_bar_tooltip::TOOLTIP_DWELL_MS;
use op_editor_ui::widgets::{TopBar, TOP_BAR_HEIGHT};
use op_editor_ui::{Point2D, Rect};

const VW: f32 = 1_800.0;
const VH: f32 = 900.0;

fn tab(
    id: u64,
    label: &str,
    dirty: bool,
    status: DocumentTabStatus,
    active: bool,
) -> DocumentTabView {
    DocumentTabView {
        id: DocumentId(id),
        label: label.to_string(),
        project: "shop".to_string(),
        dirty,
        status,
        active,
    }
}

/// A host on `hero.op` of project `shop`, with another unsaved document, an
/// agent running in a third, three pages and a Git branch.
fn host() -> WidgetHostNative {
    let mut host = WidgetHostNative::new();
    host.last_viewport_w = VW;
    host.last_viewport_h = VH;
    while host.editor_state().page_count() < 3 {
        host.editor_state_mut().add_page().expect("page added");
    }
    host.editor_state_mut().ui.active_page_index = 0;
    let ui = &mut host.editor_state_mut().editor_ui;
    ui.home.hide();
    ui.document_tabs_supported = true;
    ui.file_name_display = Some("hero.op".to_string());
    ui.document_dirty = true;
    ui.git_panel.branch = Some("main".to_string());
    ui.document_tabs = vec![
        tab(1, "hero.op", true, DocumentTabStatus::Idle, true),
        tab(2, "cart.op", true, DocumentTabStatus::Idle, false),
        tab(3, "pricing.op", false, DocumentTabStatus::Running, false),
    ];
    ui.projects_panel.projects = vec![ProjectView {
        root: "/work/shop".to_string(),
        name: "shop".to_string(),
        files: vec![ProjectFileView {
            path: "/work/shop/designs/hero.op".to_string(),
            label: "hero.op".to_string(),
            open_id: Some(DocumentId(1)),
            dirty: true,
            status: DocumentTabStatus::Idle,
            active: true,
            folder: "designs".to_string(),
            kind: ProjectFileKind::Design,
            missing: false,
            removable: false,
        }],
        ..ProjectView::default()
    }];
    host.mark_dirty();
    host
}

fn bar_rect() -> Rect {
    Rect::xywh(0.0, 0.0, VW, TOP_BAR_HEIGHT)
}

fn centre_of(host: &WidgetHostNative, button: TopBarButton) -> Point2D {
    // Measured the way paint measures: the estimator is wider than the font,
    // so items after the first would sit elsewhere.
    let bar = TopBar::for_editor_state(host.editor_state());
    let rect = host
        .topbar_button_rect(&bar, bar_rect(), button)
        .unwrap_or_else(|| panic!("{button:?} is not on the bar"));
    Point2D::new(
        rect.origin.x + rect.size.x / 2.0,
        rect.origin.y + rect.size.y / 2.0,
    )
}

const INFO_BUTTONS: [TopBarButton; 5] = [
    TopBarButton::Breadcrumb,
    TopBarButton::SaveState,
    TopBarButton::PageChip,
    TopBarButton::AgentActivity,
    TopBarButton::OtherUnsaved,
];

#[test]
fn every_info_item_hovers_and_tooltips_through_the_real_cursor_path() {
    for button in INFO_BUTTONS {
        let mut host = host();
        host.set_now_ms(1_000);
        let at = centre_of(&host, button);
        host.apply_cursor_move(at.x, at.y);
        assert_eq!(
            host.editor_state().editor_ui.topbar_button_hover,
            Some(button),
            "{button:?}: the hover lands on the item the bar paints"
        );
        assert!(
            !host.top_bar_tooltip_showing(),
            "{button:?}: not before the dwell"
        );
        host.set_now_ms(1_000 + TOOLTIP_DWELL_MS);
        assert!(
            host.top_bar_tooltip_showing(),
            "{button:?}: tooltip after the dwell"
        );
    }
}

#[test]
fn the_breadcrumb_press_requests_a_reveal_and_nothing_else() {
    let mut host = host();
    let at = centre_of(&host, TopBarButton::Breadcrumb);
    assert!(host.apply_press(at.x, at.y, VW, VH));
    let ui = &host.editor_state().editor_ui;
    assert_eq!(
        ui.projects_panel.pending_request,
        Some(ProjectRequest::RevealFile(
            "/work/shop/designs/hero.op".to_string()
        ))
    );
    assert!(ui.projects_panel.rename.is_none(), "reveal only, no rename");
}

#[test]
fn the_other_unsaved_chip_opens_the_switcher_on_the_other_unsaved_file() {
    let mut host = host();
    let at = centre_of(&host, TopBarButton::OtherUnsaved);
    assert!(host.apply_press(at.x, at.y, VW, VH));
    assert!(host.quick_switcher_active());
    let rows = op_editor_core::quick_switcher_keyboard::rows(host.editor_state());
    let selected = host
        .editor_state()
        .editor_ui
        .projects_panel
        .switcher
        .selected;
    assert_eq!(rows[selected].label, "cart.op");
}

#[test]
fn the_agent_item_switches_to_the_file_with_the_run() {
    let mut host = host();
    let at = centre_of(&host, TopBarButton::AgentActivity);
    assert!(host.apply_press(at.x, at.y, VW, VH));
    assert_eq!(
        host.editor_state().editor_ui.pending_document_request,
        Some(DocumentTabRequest::Switch(DocumentId(3)))
    );
}

#[test]
fn the_page_menu_opens_switches_the_page_refits_and_closes() {
    let mut host = host();
    let chip = centre_of(&host, TopBarButton::PageChip);
    assert!(host.apply_press(chip.x, chip.y, VW, VH));
    assert!(host.page_menu_open());

    let popup = host.page_menu_popup_rect().expect("menu is open");
    let row_h = popup.size.y / 3.0;
    let third = Point2D::new(popup.origin.x + 20.0, popup.origin.y + row_h * 2.5);
    host.editor_state_mut().viewport.zoom = 7.7;
    assert!(host.apply_press(third.x, third.y, VW, VH));
    assert_eq!(host.editor_state().ui.active_page_index, 2);
    assert!(!host.page_menu_open());
    assert_ne!(
        host.editor_state().viewport.zoom,
        7.7,
        "a real page switch re-fits the viewport, as the rail does"
    );
}

#[test]
fn choosing_the_active_page_keeps_the_canvas_view() {
    let mut host = host();
    let chip = centre_of(&host, TopBarButton::PageChip);
    host.apply_press(chip.x, chip.y, VW, VH);
    let popup = host.page_menu_popup_rect().expect("menu is open");
    let row_h = popup.size.y / 3.0;
    host.editor_state_mut().viewport.zoom = 7.7;
    let first = Point2D::new(popup.origin.x + 20.0, popup.origin.y + row_h * 0.5);
    assert!(host.apply_press(first.x, first.y, VW, VH));
    assert_eq!(host.editor_state().ui.active_page_index, 0);
    assert_eq!(host.editor_state().viewport.zoom, 7.7);
    assert!(!host.page_menu_open());
}

#[test]
fn a_second_press_on_the_chip_and_a_press_elsewhere_close_the_menu() {
    let mut host = host();
    let chip = centre_of(&host, TopBarButton::PageChip);
    host.apply_press(chip.x, chip.y, VW, VH);
    assert!(host.page_menu_open());
    host.apply_press(chip.x, chip.y, VW, VH);
    assert!(!host.page_menu_open(), "the chip toggles it");

    host.apply_press(chip.x, chip.y, VW, VH);
    assert!(host.page_menu_open());
    host.apply_press(60.0, 500.0, VW, VH);
    assert!(!host.page_menu_open(), "a press outside dismisses it");
    assert_eq!(
        host.editor_state().ui.active_page_index,
        0,
        "and picks nothing"
    );
}

#[test]
fn escape_closes_the_page_menu_first() {
    let mut host = host();
    let chip = centre_of(&host, TopBarButton::PageChip);
    host.apply_press(chip.x, chip.y, VW, VH);
    assert!(host.apply_escape());
    assert!(!host.page_menu_open());
}

#[test]
fn the_git_popover_stays_centred_under_its_button_with_the_page_chip_shown() {
    let mut host = host();
    host.editor_state_mut().editor_ui.sidebar_open = false;
    let panel = &mut host.editor_state_mut().editor_ui.git_panel;
    panel.open = true;
    panel.loading = false;
    panel.in_repo = false;
    panel.has_saved_file = false;
    host.editor_state_mut().editor_ui.enter_chat_tab();
    let bar = TopBar::for_editor_state(host.editor_state());
    assert!(bar.info.pages.is_some(), "fixture: the page chip is shown");
    let git_cx = host
        .topbar_git_button_center_x(&bar, bar_rect())
        .expect("git button");
    let panel_rect = host.git_panel_rect(VW, VH).expect("panel open");
    assert!(
        (panel_rect.origin.x + panel_rect.size.x / 2.0 - git_cx).abs() < 0.5,
        "popover centre follows the button the page chip pushed right"
    );
}

#[test]
fn a_page_menu_hover_highlights_the_row_under_the_cursor() {
    let mut host = host();
    let chip = centre_of(&host, TopBarButton::PageChip);
    host.apply_press(chip.x, chip.y, VW, VH);
    let popup = host.page_menu_popup_rect().unwrap();
    let row_h = popup.size.y / 3.0;
    host.apply_cursor_move(popup.origin.x + 20.0, popup.origin.y + row_h * 1.5);
    assert_eq!(
        host.editor_state()
            .editor_ui
            .projects_panel
            .info_bar
            .page_menu
            .hover,
        Some(1)
    );
}
