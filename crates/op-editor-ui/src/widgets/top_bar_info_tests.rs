//! The info bar's layout invariants: the drop ladder, no overlap at the three
//! reference widths, elision, and that every item is a real hit target.

use super::{BreadcrumbCell, Item, DROP_LADDER};
use crate::widgets::top_bar::*;
use crate::widgets::top_bar_title::TopBarTitleLayout;
use crate::{Point2D, Rect};
use op_editor_core::editor_ui_state::EditorUiState;
use op_editor_core::{ActiveFileInfo, DocumentId, PageSummary, RunningElsewhere, SaveState};

/// The estimator the hit-test falls back to: ASCII 0.68 em, the rest 1 em.
pub(super) fn width_of(text: &str, size: f32) -> f32 {
    text.chars()
        .map(|c| if c.is_ascii() { size * 0.68 } else { size })
        .sum()
}

pub(super) fn bar_rect(width: f32) -> Rect {
    Rect::xywh(0.0, 0.0, width, TOP_BAR_HEIGHT)
}

pub(super) fn info(
    project: &str,
    folder: &str,
    name: &str,
    save: SaveState,
    branch: Option<&str>,
) -> TopBar {
    let mut bar = TopBar::new(name);
    bar.info = ActiveFileInfo {
        project: project.to_string(),
        folder: folder.to_string(),
        file_name: name.to_string(),
        save,
        git_branch: branch.map(str::to_string),
        ..ActiveFileInfo::default()
    };
    bar.label_edited = "— Edited";
    bar.agent_count = 0;
    bar.mcp_count = 0;
    bar
}

/// `bar` plus the two chips of commit 9: "3 other unsaved" and a run in
/// another document.
pub(super) fn with_chips(mut bar: TopBar, other_unsaved: usize, running: Option<&str>) -> TopBar {
    bar.info.other_unsaved = other_unsaved;
    bar.text_other_unsaved = format!("{other_unsaved} other unsaved");
    if let Some(name) = running {
        bar.info.running_elsewhere = RunningElsewhere {
            count: 1,
            first: Some((DocumentId(7), name.to_string())),
        };
        bar.text_running = format!("Running in {name}");
    }
    bar
}

/// `bar` on page `active` (1-based) of `count`: the page chip's input.
pub(super) fn with_pages(mut bar: TopBar, count: usize, active: usize) -> TopBar {
    bar.info.pages = Some(PageSummary {
        names: (1..=count).map(|n| format!("Page {n}")).collect(),
        active: active - 1,
    });
    bar
}

pub(super) fn full_bar() -> TopBar {
    info(
        "storefront",
        "landing/v2",
        "hero-section.op",
        SaveState::Edited,
        Some("main"),
    )
}

pub(super) fn layout(bar: &TopBar, width: f32) -> TopBarTitleLayout {
    bar.title_layout(bar_rect(width), width_of)
}

/// Every rect the info bar claims, left to right.
pub(super) fn item_rects(layout: &TopBarTitleLayout) -> Vec<Rect> {
    let mut rects = Vec::new();
    rects.extend(layout.info.breadcrumb.as_ref().map(|cell| cell.rect));
    rects.extend(layout.info.save.as_ref().map(|cell| cell.rect));
    rects.extend(layout.info.page.as_ref().map(|cell| cell.rect));
    rects.extend(layout.info.agent.as_ref().map(|cell| cell.rect));
    rects.extend(layout.info.unsaved.as_ref().map(|cell| cell.rect));
    rects.extend(layout.git_rect);
    rects
}

pub(super) fn breadcrumb_text(layout: &TopBarTitleLayout) -> Vec<String> {
    layout
        .info
        .breadcrumb
        .as_ref()
        .map(|cell: &BreadcrumbCell| cell.parts.iter().map(|part| part.text.clone()).collect())
        .unwrap_or_default()
}

pub(super) fn has_part(layout: &TopBarTitleLayout, text: &str) -> bool {
    breadcrumb_text(layout).iter().any(|part| part == text)
}

pub(super) fn right(rect: Rect) -> f32 {
    rect.origin.x + rect.size.x
}

#[test]
fn a_wide_slot_shows_the_whole_breadcrumb_the_label_and_the_branch() {
    let bar = full_bar();
    let layout = layout(&bar, 1600.0);
    assert_eq!(
        breadcrumb_text(&layout),
        ["storefront", "›", "landing/v2", "›", "hero-section.op"]
    );
    let save = layout.info.save.as_ref().expect("dirty marker");
    assert_eq!(save.label, Some("— Edited"));
    let git = layout.git_rect.expect("git button");
    assert!(
        git.size.x > ICON_BUTTON,
        "the branch name is beside the icon"
    );
}

#[test]
fn drop_ladder_gives_things_up_in_the_documented_order() {
    // The order is the contract: folder, project, save label, git label, the
    // page name, the other-unsaved text, the agent text, the page chip, the
    // other-unsaved chip, the git button, and only then the name.
    assert_eq!(
        DROP_LADDER,
        [
            Item::Folder,
            Item::Project,
            Item::SaveLabel,
            Item::GitLabel,
            Item::PageLabel,
            Item::UnsavedLabel,
            Item::AgentLabel,
            Item::PageChip,
            Item::UnsavedChip,
            Item::GitButton
        ]
    );

    let bar = with_pages(with_chips(full_bar(), 3, Some("landing-page.op")), 5, 2);
    let mut seen_gone = Vec::new();
    // Narrow the window one pixel at a time and note when each thing first goes.
    for width in (300..=2600).rev() {
        let layout = layout(&bar, width as f32);
        let git_width = layout.git_rect.map_or(0.0, |rect| rect.size.x);
        let unsaved_text = layout
            .info
            .unsaved
            .as_ref()
            .and_then(|cell| cell.label.as_deref())
            .is_some_and(|label| label.contains("other"));
        let agent_text = layout
            .info
            .agent
            .as_ref()
            .is_some_and(|cell| cell.label.is_some());
        let page_name = layout
            .info
            .page
            .as_ref()
            .and_then(|cell| cell.label.as_deref())
            .is_some_and(|label| label.starts_with("Page "));
        let state = [
            ("folder", !has_part(&layout, "landing/v2")),
            ("project", !has_part(&layout, "storefront")),
            (
                "save label",
                layout
                    .info
                    .save
                    .as_ref()
                    .is_none_or(|save| save.label.is_none()),
            ),
            ("git label", git_width <= ICON_BUTTON),
            ("page name", !page_name),
            ("unsaved text", !unsaved_text),
            ("agent text", !agent_text),
            ("page chip", layout.info.page.is_none()),
            ("unsaved chip", layout.info.unsaved.is_none()),
            ("git button", layout.git_rect.is_none()),
        ];
        for (name, gone) in state {
            if gone && !seen_gone.contains(&name) {
                seen_gone.push(name);
            }
        }
        // The agent dot never drops while the bar has any room at all.
        if layout.slot.size.x > 80.0 {
            assert!(
                layout.info.agent.is_some(),
                "{width}: the agent dot dropped"
            );
        }
    }
    assert_eq!(
        seen_gone,
        [
            "folder",
            "project",
            "save label",
            "git label",
            "page name",
            "unsaved text",
            "agent text",
            "page chip",
            "unsaved chip",
            "git button"
        ]
    );
}

#[test]
fn a_dropped_item_never_comes_back_as_the_window_narrows() {
    let bar = full_bar();
    let mut previous: Option<(usize, bool, bool, bool)> = None;
    for width in (300..=1600).rev().step_by(3) {
        let layout = layout(&bar, width as f32);
        let richness = (
            breadcrumb_text(&layout).len(),
            layout
                .info
                .save
                .as_ref()
                .is_some_and(|save| save.label.is_some()),
            layout
                .git_rect
                .is_some_and(|rect| rect.size.x > ICON_BUTTON),
            layout.git_rect.is_some(),
        );
        if let Some(before) = previous {
            // The breadcrumb may lose parts and then shorten the name, never
            // gain parts; the flags only ever go from true to false.
            assert!(
                richness.0 <= before.0,
                "{width}: {richness:?} vs {before:?}"
            );
            assert!(!richness.1 || before.1, "{width}: save label came back");
            assert!(!richness.2 || before.2, "{width}: git label came back");
            assert!(!richness.3 || before.3, "{width}: git button came back");
        }
        previous = Some(richness);
    }
}

#[test]
fn the_file_name_and_the_unsaved_mark_never_drop() {
    let bar = info(
        "storefront",
        "landing",
        "a-rather-long-file-name-for-the-hero-section.op",
        SaveState::Edited,
        Some("feature/very-long-branch-name"),
    );
    // Down to the narrowest slot that still has room for anything at all.
    for width in 700..=1600 {
        let layout = layout(&bar, width as f32);
        if layout.slot.size.x < 40.0 {
            continue;
        }
        let name = layout
            .info
            .breadcrumb
            .as_ref()
            .and_then(|cell| cell.parts.iter().find(|part| part.is_file_name))
            .unwrap_or_else(|| panic!("{width}: the name dropped"));
        assert!(!name.text.is_empty(), "{width}");
        assert!(
            layout.info.save.is_some(),
            "{width}: the unsaved mark dropped"
        );
    }
}

/// Mutual order and bounds: nothing the bar claims may leave its slot, and the
/// slot itself sits between the Home button and the agent chip.
pub(super) fn assert_clear_of_chrome(bar: &TopBar, width: f32, label: &str) {
    let rect = bar_rect(width);
    let layout = layout(bar, width);
    let slot_left = layout.slot.origin.x;
    let slot_right = right(layout.slot);
    let home = bar.home_button_rect(rect);
    let chip_text_w = width_of(&bar.chip_text(), 11.0);
    let chip = bar.agent_chip_rect(rect, chip_text_w);
    assert!(slot_left >= right(home), "{label}: slot starts under Home");
    if layout.slot.size.x == 0.0 {
        // No room at all (a crowded right side on a small window): the bar
        // claims nothing, so there is nothing to overlap.
        assert!(
            item_rects(&layout).is_empty(),
            "{label}: items in a closed slot"
        );
        return;
    }
    assert!(
        slot_right <= chip.origin.x,
        "{label}: slot reaches the chip"
    );

    let mut cursor = slot_left;
    for item in item_rects(&layout) {
        assert!(
            item.origin.x >= cursor - 0.01,
            "{label}: items overlap {item:?}"
        );
        assert!(
            right(item) <= slot_right + 0.01,
            "{label}: item leaves the slot"
        );
        assert!(item.origin.x >= right(home), "{label}: item under Home");
        assert!(
            right(item) <= chip.origin.x,
            "{label}: item under the agent chip"
        );
        cursor = right(item);
    }
    if let Some(git) = layout.git_rect {
        // The Git button stays a clean target: its centre hits Git, not a neighbour.
        let centre = Point2D::new(git.origin.x + git.size.x / 2.0, TOP_BAR_HEIGHT / 2.0);
        assert_eq!(
            bar.hit_test(rect, centre),
            Some(TopBarHit::ToggleGitPanel),
            "{label}: git centre"
        );
    }
}

pub(super) fn crowded(mut bar: TopBar) -> TopBar {
    bar.agent_count = 6;
    bar.connected = [true; 8];
    bar.mcp_count = 8;
    bar.account_button_visible = true;
    bar.collab.visible = true;
    bar.collab.label = "Connected".to_string();
    bar
}

#[test]
fn nothing_overlaps_home_git_or_the_agent_chip_at_640_960_and_1440() {
    // None of the 15 shipped locales is right-to-left, so there is no RTL
    // layout to test: the CJK case below stands in for wide glyphs.
    let long = "an-extremely-long-file-name-that-keeps-going-and-going-for-the-title-bar.op";
    type Case = (&'static str, fn() -> TopBar);
    let cases: [Case; 5] = [
        ("short", || info("", "", "a.op", SaveState::Saved, None)),
        ("untitled", || {
            info("", "", "Untitled", SaveState::Untitled, None)
        }),
        ("long names", || {
            info(
                "a-very-long-project-name-from-the-monorepo",
                "deeply/nested/folder/structure/of/the/design",
                "an-extremely-long-file-name-that-keeps-going-and-going-for-the-title-bar.op",
                SaveState::Edited,
                Some("feature/a-long-branch-name-here"),
            )
        }),
        ("missing", || {
            info("shop", "x", "gone.op", SaveState::Missing, Some("main"))
        }),
        ("cjk", || {
            info(
                "商店",
                "首页",
                "首页设计稿最终版本.op",
                SaveState::Edited,
                Some("主分支"),
            )
        }),
    ];
    assert!(long.len() > 60);
    for width in [640.0, 960.0, 1440.0] {
        for (name, make) in cases {
            assert_clear_of_chrome(&make(), width, &format!("{name} @ {width}"));
            // A crowded right side shrinks the slot further.
            assert_clear_of_chrome(
                &crowded(make()),
                width,
                &format!("{name} crowded @ {width}"),
            );
        }
    }
}

#[test]
fn project_and_folder_keep_both_ends_and_the_name_keeps_its_extension() {
    let bar = info(
        "monorepo-storefront-checkout-redesign",
        "marketing/campaigns/spring-sale-2027",
        "hero-section-with-a-very-long-descriptive-name-v12-final.op",
        SaveState::Saved,
        None,
    );
    let layout = layout(&bar, 2400.0);
    let parts = breadcrumb_text(&layout);
    let project = &parts[0];
    assert!(project.contains('…'), "{project}");
    assert!(project.starts_with("monorepo"), "{project}");
    assert!(project.ends_with("redesign"), "{project}");
    let folder = &parts[2];
    assert!(
        folder.starts_with("marketing") && folder.ends_with("2027"),
        "{folder}"
    );
    let name = parts.last().unwrap();
    assert!(name.ends_with(".op"), "{name}");
    assert!(name.contains('…'), "{name}");
}

#[test]
fn only_the_file_name_is_marked_as_the_emphasised_part() {
    let layout = layout(&full_bar(), 1600.0);
    let cell = layout.info.breadcrumb.as_ref().unwrap();
    let emphasised: Vec<_> = cell.parts.iter().filter(|part| part.is_file_name).collect();
    assert_eq!(emphasised.len(), 1);
    assert_eq!(emphasised[0].text, "hero-section.op");
    assert!(std::ptr::eq(emphasised[0], cell.parts.last().unwrap()));
}

#[test]
fn a_file_with_no_project_shows_the_name_alone() {
    let bar = info("", "", "Untitled", SaveState::Untitled, None);
    let layout = layout(&bar, 1600.0);
    assert_eq!(breadcrumb_text(&layout), ["Untitled"]);
}

#[test]
fn a_saved_file_keeps_no_marker_once_the_label_is_dropped() {
    let bar = info("shop", "", "hero.op", SaveState::Saved, Some("main"));
    let wide = layout(&bar, 1600.0);
    assert_eq!(
        wide.info.save.as_ref().and_then(|save| save.label),
        Some("Saved")
    );
    // Narrow enough that the save label has gone: a clean file has no dot.
    let narrow = (500..1600)
        .map(|width| layout(&bar, width as f32))
        .find(|layout| layout.info.save.is_none() && layout.info.breadcrumb.is_some())
        .expect("a width where only the label is gone");
    assert!(narrow.info.save.is_none());
}

#[test]
fn an_unsaved_file_keeps_a_dot_when_the_label_is_dropped() {
    let bar = info("shop", "", "hero.op", SaveState::Edited, Some("main"));
    let dotted = (500..1600)
        .map(|width| layout(&bar, width as f32))
        .find(|layout| {
            layout
                .info
                .save
                .as_ref()
                .is_some_and(|save| save.label.is_none())
        })
        .expect("a width where the label has become a dot");
    let save = dotted.info.save.unwrap();
    assert!(
        save.rect.size.x <= 6.5,
        "the dot, not the label: {:?}",
        save.rect
    );
}

#[test]
fn the_save_marker_is_a_hit_target_and_a_button_rect() {
    let bar = full_bar();
    let rect = bar_rect(1600.0);
    let layout = layout(&bar, 1600.0);
    let save = layout.info.save.as_ref().unwrap().rect;
    let centre = Point2D::new(save.origin.x + save.size.x / 2.0, TOP_BAR_HEIGHT / 2.0);
    assert_eq!(bar.hit_test(rect, centre), Some(TopBarHit::SaveState));
    assert_eq!(
        bar.button_rect(rect, op_editor_core::TopBarButton::SaveState),
        Some(save)
    );
}

#[test]
fn the_breadcrumb_is_plain_text_until_it_has_somewhere_to_go() {
    let bar = full_bar();
    let rect = bar_rect(1600.0);
    let cell = layout(&bar, 1600.0).info.breadcrumb.unwrap();
    let centre = Point2D::new(
        cell.rect.origin.x + cell.rect.size.x / 2.0,
        TOP_BAR_HEIGHT / 2.0,
    );
    assert_eq!(bar.hit_test(rect, centre), None);
}

#[test]
fn zero_width_slot_shows_nothing() {
    let bar = full_bar();
    let layout = layout(&bar, 260.0);
    assert_eq!(layout.slot.size.x, 0.0);
    assert!(item_rects(&layout).is_empty());
}

#[test]
fn zero_one_and_many_documents_share_one_code_path() {
    // The strip is gone: the bar describes the active file whatever the count,
    // and the open documents only matter to the other-unsaved chip.
    use op_editor_core::{DocumentId, DocumentTabStatus, DocumentTabView};
    let tab = |n: u64, active: bool| DocumentTabView {
        id: DocumentId(n),
        label: format!("doc-{n}.op"),
        project: String::new(),
        dirty: false,
        status: DocumentTabStatus::Idle,
        active,
    };
    let reference = {
        EditorUiState {
            document_tabs_supported: true,
            file_name_display: Some("doc-0.op".to_string()),
            ..EditorUiState::default()
        }
    };
    let base = layout(&TopBar::for_editor_ui(&reference), 1400.0);
    for count in [0u64, 1, 3, 12] {
        let mut ui = reference.clone();
        ui.document_tabs = (0..count).map(|n| tab(n, n == 0)).collect();
        let layout = layout(&TopBar::for_editor_ui(&ui), 1400.0);
        assert_eq!(breadcrumb_text(&layout), ["doc-0.op"], "{count} documents");
        assert_eq!(item_rects(&layout), item_rects(&base), "{count} documents");
    }
}

#[test]
fn git_popover_anchor_follows_the_painted_git_button() {
    let bar = full_bar();
    let rect = bar_rect(1600.0);
    let git = bar.git_button_rect(rect);
    assert!(git.size.x > 0.0);
    assert_eq!(
        bar.git_button_center_x(rect),
        Some(git.origin.x + git.size.x / 2.0)
    );
}
