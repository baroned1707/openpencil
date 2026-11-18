use super::*;
use crate::document_tabs::DocumentTabStatus;
use crate::project_tree::{FigStatus, ProjectFileView};

fn tab(id: u64, label: &str, project: &str) -> DocumentTabView {
    DocumentTabView {
        id: DocumentId(id),
        label: label.to_string(),
        project: project.to_string(),
        dirty: false,
        status: DocumentTabStatus::Idle,
        active: false,
    }
}

fn file(label: &str, open: Option<u64>) -> ProjectFileView {
    ProjectFileView {
        path: format!("/work/{label}"),
        label: label.to_string(),
        open_id: open.map(DocumentId),
        dirty: false,
        status: DocumentTabStatus::Idle,
        active: false,
        folder: String::new(),
        kind: ProjectFileKind::Design,
        missing: false,
        removable: false,
    }
}

fn project(name: &str, files: Vec<ProjectFileView>) -> ProjectView {
    ProjectView {
        root: format!("/work/{name}"),
        name: name.to_string(),
        files,
        ..ProjectView::default()
    }
}

fn labels(rows: &[QuickSwitcherRow]) -> Vec<&str> {
    rows.iter().map(|row| row.label.as_str()).collect()
}

#[test]
fn open_documents_come_first_then_project_files_in_dock_order() {
    let tabs = vec![tab(1, "zeta.op", "shop"), tab(2, "alpha.op", "shop")];
    let projects = vec![
        project(
            "shop",
            vec![file("zeta.op", Some(1)), file("beta.op", None)],
        ),
        project("blog", vec![file("gamma.op", None)]),
    ];
    let rows = build_rows("", &tabs, &projects);
    assert_eq!(
        labels(&rows),
        ["zeta.op", "alpha.op", "beta.op", "gamma.op"]
    );
    assert!(rows[0].open && rows[1].open && !rows[2].open);
    assert_eq!(
        rows[2].target,
        QuickSwitcherTarget::File("/work/beta.op".to_string())
    );
    assert_eq!(rows[0].target, QuickSwitcherTarget::Document(DocumentId(1)));
}

#[test]
fn an_open_file_is_listed_once_as_the_document_never_as_a_file() {
    let tabs = vec![tab(1, "a.op", "shop")];
    let projects = vec![project(
        "shop",
        vec![file("a.op", Some(1)), file("b.op", None)],
    )];
    let rows = build_rows("", &tabs, &projects);
    assert_eq!(labels(&rows), ["a.op", "b.op"]);
}

#[test]
fn figma_sources_are_never_listed() {
    let mut fig = file("brand.fig", None);
    fig.kind = ProjectFileKind::Figma(FigStatus::NotConverted);
    let projects = vec![project("shop", vec![fig, file("brand.op", None)])];
    assert_eq!(labels(&build_rows("brand", &[], &projects)), ["brand.op"]);
}

#[test]
fn the_digit_is_the_one_the_shortcut_uses() {
    let tabs: Vec<_> = (1..=11).map(|n| tab(n, &format!("d{n}.op"), "")).collect();
    let rows = build_rows("", &tabs, &[]);
    let digits: Vec<Option<u8>> = rows.iter().map(|row| row.digit).collect();
    assert_eq!(
        digits,
        [
            Some(1),
            Some(2),
            Some(3),
            Some(4),
            Some(5),
            Some(6),
            Some(7),
            Some(8),
            None,
            None,
            Some(9)
        ]
    );
    assert_eq!(
        build_rows("", &tabs[..1], &[])[0].digit,
        None,
        "one tab: no chord"
    );
}

#[test]
fn a_query_ranks_prefix_over_word_start_over_scattered_inside_each_group() {
    let tabs = vec![
        tab(1, "pricing-table.op", ""),
        tab(2, "checkout-pay.op", ""),
        tab(3, "pay-now.op", ""),
    ];
    let projects = vec![project(
        "shop",
        vec![
            file("repay-plan.op", None),
            file("pay-later.op", None),
            file("p-a-y.op", None),
        ],
    )];
    let rows = build_rows("pay", &tabs, &projects);
    assert_eq!(
        labels(&rows),
        // Open group: prefix, then word start (pricing-table does not match).
        // File group: prefix, then the contiguous-letters scatter `repay`,
        // then the looser `p-a-y`.
        [
            "pay-now.op",
            "checkout-pay.op",
            "pay-later.op",
            "repay-plan.op",
            "p-a-y.op"
        ],
    );
    let rows = build_rows("pay", &[], &projects);
    assert_eq!(labels(&rows), ["pay-later.op", "repay-plan.op", "p-a-y.op"]);
}

#[test]
fn ties_keep_their_input_order() {
    let projects = vec![project(
        "shop",
        vec![
            file("card-b.op", None),
            file("card-a.op", None),
            file("card-c.op", None),
        ],
    )];
    assert_eq!(
        labels(&build_rows("card", &[], &projects)),
        ["card-b.op", "card-a.op", "card-c.op"],
        "same tier: not alphabetised, not shuffled"
    );
}

#[test]
fn the_list_is_capped_at_fifty_rows() {
    let files: Vec<_> = (0..120)
        .map(|n| file(&format!("f{n:03}.op"), None))
        .collect();
    let tabs: Vec<_> = (0..10).map(|n| tab(n, &format!("t{n}.op"), "")).collect();
    let rows = build_rows("", &tabs, &[project("big", files)]);
    assert_eq!(rows.len(), MAX_ROWS);
    assert!(
        rows[..10].iter().all(|row| row.open),
        "open documents are kept first"
    );
    assert_eq!(rows[10].label, "f000.op");
    assert_eq!(rows.last().unwrap().label, "f039.op");
}

#[test]
fn a_project_name_lists_its_files_after_files_named_for_it() {
    let projects = vec![
        project(
            "shop",
            vec![file("checkout.op", None), file("shop-home.op", None)],
        ),
        project("blog", vec![file("home.op", None)]),
    ];
    assert_eq!(
        labels(&build_rows("shop", &[], &projects)),
        ["shop-home.op", "checkout.op"]
    );
}

#[test]
fn nothing_matches_means_no_rows() {
    let projects = vec![project("shop", vec![file("a.op", None)])];
    assert!(build_rows("zzz", &[], &projects).is_empty());
}

#[test]
fn selection_wraps_and_the_window_follows_it() {
    let mut state = QuickSwitcherState::default();
    state.open_fresh(0);
    assert!(state.open);
    state.move_selection(-1, 20);
    assert_eq!(
        state.selected, 19,
        "up from the first row wraps to the last"
    );
    assert_eq!(state.first_visible, 20 - VISIBLE_ROWS, "the window follows");
    state.move_selection(1, 20);
    assert_eq!(
        (state.selected, state.first_visible),
        (0, 0),
        "down from the last wraps"
    );
    for _ in 0..VISIBLE_ROWS {
        state.move_selection(1, 20);
    }
    assert_eq!(state.selected, VISIBLE_ROWS);
    assert_eq!(
        state.first_visible, 1,
        "scrolls only once the selection leaves the window"
    );
    state.move_selection(0, 0);
    state.select(99, 20);
    assert_eq!(state.selected, 19, "clamped to the rows");
}

#[test]
fn wheel_scroll_is_clamped_and_leaves_the_selection() {
    let mut state = QuickSwitcherState::default();
    state.scroll_rows(100, 20);
    assert_eq!(state.first_visible, 20 - VISIBLE_ROWS);
    state.scroll_rows(-100, 20);
    assert_eq!(state.first_visible, 0);
    state.scroll_rows(3, 5);
    assert_eq!(state.first_visible, 0, "everything fits: nothing to scroll");
}

#[test]
fn close_forgets_the_query() {
    let mut state = QuickSwitcherState::default();
    state.open_fresh(0);
    state.input.insert_str("abc", 0);
    state.selected = 3;
    assert!(state.close());
    assert_eq!(state, QuickSwitcherState::default());
    assert!(!state.close());
}
