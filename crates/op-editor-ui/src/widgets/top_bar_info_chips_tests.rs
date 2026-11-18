//! The info bar's chips and links: other-unsaved, agent activity, the page chip,
//! the clickable breadcrumb, and the Git anchor beside them.

use super::tests::{
    assert_clear_of_chrome, bar_rect, crowded, full_bar, info, layout, right, with_chips,
    with_pages,
};
use crate::widgets::top_bar::*;
use crate::Point2D;
use op_editor_core::editor_ui_state::EditorUiState;
use op_editor_core::{DocumentId, SaveState};

#[test]
fn the_chips_sit_in_order_and_are_real_hit_targets() {
    let bar = with_chips(full_bar(), 2, Some("landing-page.op"));
    let rect = bar_rect(2000.0);
    let layout = layout(&bar, 2000.0);
    let agent = layout.info.agent.as_ref().expect("agent item").rect;
    let unsaved = layout.info.unsaved.as_ref().expect("unsaved chip").rect;
    let save = layout.info.save.as_ref().unwrap().rect;
    assert!(right(save) <= agent.origin.x && right(agent) <= unsaved.origin.x);
    assert!(right(unsaved) <= layout.git_rect.unwrap().origin.x);
    assert_eq!(
        layout.info.agent.as_ref().unwrap().label.as_deref(),
        Some("Running in landing-page.op")
    );
    assert_eq!(
        layout.info.unsaved.as_ref().unwrap().label.as_deref(),
        Some("2 other unsaved")
    );
    for (rect_of, hit, button) in [
        (
            agent,
            TopBarHit::AgentActivity,
            op_editor_core::TopBarButton::AgentActivity,
        ),
        (
            unsaved,
            TopBarHit::OtherUnsaved,
            op_editor_core::TopBarButton::OtherUnsaved,
        ),
    ] {
        let centre = Point2D::new(
            rect_of.origin.x + rect_of.size.x / 2.0,
            TOP_BAR_HEIGHT / 2.0,
        );
        assert_eq!(bar.hit_test(rect, centre), Some(hit));
        assert_eq!(bar.button_rect(rect, button), Some(rect_of));
    }
}

#[test]
fn idle_and_all_saved_show_neither_chip() {
    let bar = full_bar();
    let layout = layout(&bar, 2000.0);
    assert!(layout.info.agent.is_none() && layout.info.unsaved.is_none());
    let rect = bar_rect(2000.0);
    assert_eq!(
        bar.button_rect(rect, op_editor_core::TopBarButton::AgentActivity),
        None
    );
    assert_eq!(
        bar.button_rect(rect, op_editor_core::TopBarButton::OtherUnsaved),
        None
    );
}

#[test]
fn a_narrow_chip_keeps_its_count_and_the_agent_its_dot() {
    let bar = with_chips(full_bar(), 12, Some("landing-page.op"));
    // The first width at which the unsaved chip has lost its words but not the chip.
    let narrow = (300..2400)
        .map(|width| layout(&bar, width as f32))
        .filter(|layout| layout.slot.size.x > 0.0)
        .find(|layout| {
            layout
                .info
                .unsaved
                .as_ref()
                .is_some_and(|cell| cell.label.as_deref() == Some("12"))
        })
        .expect("a width where only the count remains");
    let agent = narrow.info.agent.expect("agent dot stays");
    let unsaved = narrow.info.unsaved.unwrap();
    assert!(unsaved.rect.size.x < 40.0, "{:?}", unsaved.rect);
    // Either it still has its words or it is down to the dot, never both gone.
    assert!(agent.rect.size.x > 0.0);
}

#[test]
fn a_long_running_label_is_middle_elided() {
    let name = "a-really-long-design-file-name-that-would-push-the-whole-bar-around.op";
    let bar = with_chips(full_bar(), 0, Some(name));
    let layout = layout(&bar, 2400.0);
    let label = layout.info.agent.unwrap().label.expect("room for the text");
    assert!(label.contains('…'), "{label}");
    assert!(label.starts_with("Running in"), "{label}");
    assert!(label.ends_with(".op"), "{label}");
}

#[test]
fn nothing_overlaps_with_both_chips_at_640_960_and_1440() {
    for width in [640.0, 960.0, 1440.0] {
        let bar = with_chips(full_bar(), 14, Some("a-long-file-name-in-another-tab.op"));
        assert_clear_of_chrome(&bar, width, &format!("chips @ {width}"));
        assert_clear_of_chrome(&crowded(bar), width, &format!("chips crowded @ {width}"));
    }
}

#[test]
fn for_editor_ui_builds_the_chip_texts_from_the_tabs() {
    use op_editor_core::DocumentTabView;
    let tab = |id: u64, dirty: bool, running: bool, active: bool| DocumentTabView {
        id: DocumentId(id),
        label: format!("doc-{id}.op"),
        project: String::new(),
        dirty,
        status: if running {
            op_editor_core::DocumentTabStatus::Running
        } else {
            op_editor_core::DocumentTabStatus::Idle
        },
        active,
    };
    let mut ui = EditorUiState {
        document_tabs_supported: true,
        ..EditorUiState::default()
    };
    ui.document_tabs = vec![
        tab(1, true, true, true),
        tab(2, true, true, false),
        tab(3, true, false, false),
        tab(4, false, false, false),
    ];
    let bar = TopBar::for_editor_ui(&ui);
    assert_eq!(bar.info.other_unsaved, 2, "the active document is excluded");
    assert_eq!(bar.text_other_unsaved, "2 other unsaved");
    assert_eq!(bar.text_running, "Running in doc-2.op");

    ui.document_tabs[2].status = op_editor_core::DocumentTabStatus::Running;
    assert_eq!(
        TopBar::for_editor_ui(&ui).text_running,
        "Running · 2 others"
    );
    ui.document_tabs[1].status = op_editor_core::DocumentTabStatus::Idle;
    ui.document_tabs[2].status = op_editor_core::DocumentTabStatus::Idle;
    assert_eq!(
        TopBar::for_editor_ui(&ui).text_running,
        "",
        "idle shows nothing"
    );
}

#[test]
fn the_page_chip_shows_the_current_page_and_is_a_hit_target() {
    let bar = with_pages(full_bar(), 5, 2);
    let rect = bar_rect(2000.0);
    let layout = layout(&bar, 2000.0);
    let page = layout.info.page.as_ref().expect("page chip");
    assert_eq!(page.label.as_deref(), Some("Page 2"));
    let save = layout.info.save.as_ref().unwrap().rect;
    assert!(
        right(save) <= page.rect.origin.x,
        "the chip follows the save marker"
    );
    let centre = Point2D::new(
        page.rect.origin.x + page.rect.size.x / 2.0,
        TOP_BAR_HEIGHT / 2.0,
    );
    assert_eq!(bar.hit_test(rect, centre), Some(TopBarHit::PageChip));
    assert_eq!(
        bar.button_rect(rect, op_editor_core::TopBarButton::PageChip),
        Some(page.rect)
    );
}

#[test]
fn a_narrow_page_chip_shows_the_position_instead_of_the_name() {
    let bar = with_pages(full_bar(), 12, 7);
    let narrow = (300..2600)
        .map(|width| layout(&bar, width as f32))
        .filter(|layout| layout.slot.size.x > 0.0)
        .find(|layout| {
            layout
                .info
                .page
                .as_ref()
                .is_some_and(|cell| cell.label.as_deref() == Some("7/12"))
        })
        .expect("a width where the chip is down to 7/12");
    assert!(narrow.info.page.unwrap().rect.size.x < 60.0);
}

#[test]
fn without_pages_there_is_no_page_chip() {
    // `ActiveFileInfo::pages` is `None` for one page and on the web.
    let layout = layout(&full_bar(), 2000.0);
    assert!(layout.info.page.is_none());
    assert_eq!(
        full_bar().button_rect(bar_rect(2000.0), op_editor_core::TopBarButton::PageChip),
        None
    );
}

#[test]
fn nothing_overlaps_with_every_item_at_640_960_and_1440() {
    for width in [640.0, 960.0, 1440.0] {
        let bar = with_pages(
            with_chips(full_bar(), 14, Some("a-long-file-name-in-another-tab.op")),
            30,
            12,
        );
        assert_clear_of_chrome(&bar, width, &format!("every item @ {width}"));
        assert_clear_of_chrome(
            &crowded(bar),
            width,
            &format!("every item crowded @ {width}"),
        );
    }
}

#[test]
fn the_git_anchor_follows_the_button_while_the_page_chip_is_shown() {
    // The page chip widens the group, which moves the Git button: the rect the
    // popover hangs from must be the one the button is drawn at, with the chip
    // on or off.
    let rect = bar_rect(1600.0);
    for bar in [full_bar(), with_pages(full_bar(), 6, 3)] {
        let git = bar.git_button_rect(rect);
        assert!(git.size.x > 0.0);
        assert_eq!(
            bar.git_button_center_x(rect),
            Some(git.origin.x + git.size.x / 2.0)
        );
        let centre = Point2D::new(git.origin.x + git.size.x / 2.0, TOP_BAR_HEIGHT / 2.0);
        assert_eq!(bar.hit_test(rect, centre), Some(TopBarHit::ToggleGitPanel));
    }
    let plain = full_bar().git_button_rect(rect).origin.x;
    let with_chip = with_pages(full_bar(), 6, 3).git_button_rect(rect).origin.x;
    assert!(with_chip > plain, "the chip pushes Git right");
}

#[test]
fn for_editor_state_offers_the_chip_only_with_two_pages_and_the_dock() {
    let mut state = op_editor_core::EditorState::default();
    state.editor_ui.document_tabs_supported = true;
    assert!(
        TopBar::for_editor_state(&state).info.pages.is_none(),
        "one page"
    );
    while state.page_count() < 3 {
        state.add_page().expect("page added");
    }
    assert_eq!(
        TopBar::for_editor_state(&state)
            .info
            .pages
            .unwrap()
            .names
            .len(),
        3
    );
    // The web never sets the dock capability: no page chip in v1.
    state.editor_ui.document_tabs_supported = false;
    let web = TopBar::for_editor_state(&state);
    assert!(web.info.pages.is_none());
    assert!(layout(&web, 2000.0).info.page.is_none());
}

#[test]
fn the_chip_paints_pressed_while_its_menu_is_open() {
    let mut state = op_editor_core::EditorState::default();
    state.editor_ui.document_tabs_supported = true;
    while state.page_count() < 2 {
        state.add_page().expect("page added");
    }
    assert!(!TopBar::for_editor_state(&state).page_menu_open);
    state.editor_ui.projects_panel.info_bar.open_page_menu(0);
    assert!(TopBar::for_editor_state(&state).page_menu_open);
}

#[test]
fn the_breadcrumb_is_a_hit_target_when_the_dock_lists_the_file() {
    let mut bar = full_bar();
    bar.info.file_path = Some("/work/shop/landing/v2/hero-section.op".to_string());
    let rect = bar_rect(1600.0);
    let cell = layout(&bar, 1600.0).info.breadcrumb.unwrap();
    assert!(cell.clickable);
    for probe in [0.1, 0.5, 0.9] {
        let at = Point2D::new(
            cell.rect.origin.x + cell.rect.size.x * probe,
            TOP_BAR_HEIGHT / 2.0,
        );
        assert_eq!(
            bar.hit_test(rect, at),
            Some(TopBarHit::Breadcrumb),
            "{probe}"
        );
    }
    assert_eq!(
        bar.button_rect(rect, op_editor_core::TopBarButton::Breadcrumb),
        Some(cell.rect)
    );
}

#[test]
fn the_breadcrumb_stays_plain_text_without_a_dock_row() {
    // Web, an untitled document and a file no project owns have no row to show.
    let bar = full_bar();
    let rect = bar_rect(1600.0);
    let cell = layout(&bar, 1600.0).info.breadcrumb.unwrap();
    assert!(!cell.clickable);
    assert_eq!(
        bar.button_rect(rect, op_editor_core::TopBarButton::Breadcrumb),
        None
    );
}

#[test]
fn the_breadcrumb_and_the_save_marker_do_not_share_a_hit() {
    let mut bar = full_bar();
    bar.info.file_path = Some("/work/shop/hero.op".to_string());
    let rect = bar_rect(1600.0);
    let layout = layout(&bar, 1600.0);
    let save = layout.info.save.as_ref().unwrap().rect;
    let at = Point2D::new(save.origin.x + save.size.x / 2.0, TOP_BAR_HEIGHT / 2.0);
    assert_eq!(bar.hit_test(rect, at), Some(TopBarHit::SaveState));
}

#[test]
fn for_editor_ui_makes_the_breadcrumb_clickable_only_for_a_listed_file() {
    use op_editor_core::{ProjectFileKind, ProjectFileView, ProjectView};
    let mut ui = EditorUiState {
        document_tabs_supported: true,
        file_name_display: Some("hero.op".to_string()),
        ..EditorUiState::default()
    };
    ui.projects_panel.projects = vec![ProjectView {
        root: "/work/shop".to_string(),
        name: "shop".to_string(),
        files: vec![ProjectFileView {
            path: "/work/shop/designs/hero.op".to_string(),
            label: "hero.op".to_string(),
            open_id: Some(DocumentId(1)),
            dirty: false,
            status: op_editor_core::DocumentTabStatus::Idle,
            active: true,
            folder: "designs".to_string(),
            kind: ProjectFileKind::Design,
            missing: false,
            removable: true,
        }],
        ..ProjectView::default()
    }];
    let bar = TopBar::for_editor_ui(&ui);
    let cell = layout(&bar, 1600.0).info.breadcrumb.unwrap();
    assert!(cell.clickable);
    assert_eq!(
        cell.parts
            .iter()
            .map(|part| part.text.as_str())
            .collect::<Vec<_>>(),
        ["shop", "›", "designs", "›", "hero.op"]
    );
    ui.projects_panel.projects.clear();
    let plain = layout(&TopBar::for_editor_ui(&ui), 1600.0)
        .info
        .breadcrumb
        .unwrap();
    assert!(!plain.clickable);
}

#[test]
fn on_a_tiny_slot_the_name_outranks_the_agent_dot() {
    // About 60 px at a 640 px window: the dot would leave the name nothing.
    let bar = with_chips(
        info(
            "shop",
            "",
            "hero-section.op",
            SaveState::Edited,
            Some("main"),
        ),
        0,
        Some("checkout-flow.op"),
    );
    let tiny = (500..700)
        .map(|width| layout(&bar, width as f32))
        .find(|layout| layout.slot.size.x > 50.0 && layout.slot.size.x < 75.0)
        .expect("a window with a tiny slot");
    assert!(tiny.info.agent.is_none(), "the dot yields: {:?}", tiny.slot);
    let name = tiny
        .info
        .breadcrumb
        .as_ref()
        .and_then(|cell| cell.parts.iter().find(|part| part.is_file_name))
        .expect("the name stays");
    assert!(name.text.contains('…') && !name.text.is_empty());
    assert!(tiny.info.save.is_some(), "and so does the unsaved mark");
}
