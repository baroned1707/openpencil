//! The File menu's "Open project" row: offered only where the host has a
//! Projects dock, right under "Open file", and every row after it moves down
//! by exactly one without a gap in the row map.

use super::*;
use jian_widgets::components::menu::MenuHit;

fn panel(menu: &FileMenu<'_>) -> Rect {
    Rect {
        origin: Point2D::new(100.0, 50.0),
        size: Point2D::new(MENU_WIDTH, menu.height()),
    }
}

fn desktop() -> EditorUiState {
    EditorUiState {
        document_tabs_supported: true,
        ..EditorUiState::default()
    }
}

fn row_centre(panel: Rect, row_from_top: usize, dividers_above: usize) -> Point2D {
    let divider = DIVIDER_GAP * 2.0 + 1.0;
    Point2D::new(
        panel.origin.x + 20.0,
        panel.origin.y
            + PAD_Y
            + ROW_HEIGHT * row_from_top as f32
            + divider * dividers_above as f32
            + ROW_HEIGHT / 2.0,
    )
}

#[test]
fn a_host_with_a_projects_dock_offers_open_project_under_open_file() {
    let ui = desktop();
    let menu = FileMenu::for_editor_ui(&ui, Vec::new());
    assert_eq!(menu.choice_for_row(3), Some(FileMenuChoice::OpenFile));
    assert_eq!(menu.choice_for_row(4), Some(FileMenuChoice::OpenProject));
    assert_eq!(menu.choice_for_row(5), Some(FileMenuChoice::Save));
    assert_eq!(menu.choice_for_row(6), Some(FileMenuChoice::SaveAs));
    assert_eq!(
        menu.choice_for_row(7),
        Some(FileMenuChoice::ExportImage),
        "the rows after it moved down by one"
    );
    let panel = panel(&menu);
    assert_eq!(
        menu.hit_test(panel, row_centre(panel, 4, 0)),
        Some(FileMenuChoice::OpenProject)
    );
    assert_eq!(
        menu.hit_test(panel, row_centre(panel, 5, 1)),
        Some(FileMenuChoice::Save)
    );
}

#[test]
fn a_host_without_a_dock_keeps_the_menu_exactly_as_it_was() {
    let mut touch = desktop();
    touch.touch = true;
    for ui in [EditorUiState::default(), touch] {
        let menu = FileMenu::for_editor_ui(&ui, Vec::new());
        assert_eq!(menu.choice_for_row(4), Some(FileMenuChoice::Save));
        assert_eq!(menu.choice_for_row(5), Some(FileMenuChoice::SaveAs));
        assert_eq!(menu.choice_for_row(6), Some(FileMenuChoice::ExportImage));
    }
    let plain = FileMenu::for_editor_ui(&EditorUiState::default(), Vec::new()).height();
    let docked = FileMenu::for_editor_ui(&desktop(), Vec::new()).height();
    assert_eq!(docked, plain + ROW_HEIGHT);
}

#[test]
fn the_row_map_has_no_gap_and_agrees_with_the_hit_test_for_every_row() {
    let mut ui = desktop();
    ui.scene_template_center.save_current_supported = true;
    let recent = vec![
        RecentEntry {
            name: "one.op".to_string(),
            age: "now".to_string(),
        },
        RecentEntry {
            name: "two.op".to_string(),
            age: "now".to_string(),
        },
    ];
    let menu = FileMenu::for_editor_ui(&ui, recent);
    let panel = panel(&menu);
    let mut rows = Vec::new();
    let mut y = panel.origin.y;
    while y < panel.origin.y + panel.size.y {
        if let MenuHit::Row(row) = menu.hit(panel, Point2D::new(panel.origin.x + 20.0, y)) {
            if rows.last() != Some(&row) {
                rows.push(row);
            }
        }
        y += 1.0;
    }
    let expected: Vec<usize> = (0..rows.len()).collect();
    assert_eq!(
        rows, expected,
        "rows are numbered 0..n in order, none skipped"
    );
    let projects = rows
        .iter()
        .filter(|row| menu.choice_for_row(**row) == Some(FileMenuChoice::OpenProject))
        .count();
    assert_eq!(projects, 1, "exactly one Open project row");
    for row in rows {
        assert!(
            menu.choice_for_row(row).is_some(),
            "row {row} has an action"
        );
    }
}
