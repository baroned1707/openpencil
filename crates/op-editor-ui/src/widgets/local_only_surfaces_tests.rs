//! Local-only invariant: while no account / collaboration backend is
//! available (the state `op-host-desktop` leaves the editor in for every
//! default build), the title bar and the Home surface offer no way in to an
//! account or to collaboration. The gates under test are
//! `EditorUiState::account_ui_available` and `CollabAvailability::Unavailable`.

use super::home_surface::HomeSurface;
use super::top_bar::{TopBar, TopBarHit};
use super::TOP_BAR_HEIGHT;
use crate::{Point2D, Rect};
use op_editor_core::{CollabAvailability, CollabConnectionPhase, EditorState, HomeHit};

fn local_only_state() -> EditorState {
    let state = EditorState::new();
    assert!(!state.editor_ui.account_ui_available);
    assert_eq!(
        state.editor_ui.collab.availability,
        CollabAvailability::Unavailable
    );
    assert_eq!(state.editor_ui.collab.phase, CollabConnectionPhase::Idle);
    state
}

#[test]
fn title_bar_has_no_account_or_collaboration_affordance_when_unavailable() {
    let state = local_only_state();
    let bar = TopBar::for_editor_ui(&state.editor_ui);
    assert!(!bar.account_button_visible, "no avatar / sign-in button");
    assert!(!bar.collab.visible, "no collaborate chip");
    assert!(!bar.collab.enabled);

    let rect = Rect {
        origin: Point2D::new(0.0, 0.0),
        size: Point2D::new(1400.0, TOP_BAR_HEIGHT),
    };
    for x in 0..1400 {
        let hit = bar.hit_test(rect, Point2D::new(x as f32, TOP_BAR_HEIGHT / 2.0));
        assert_ne!(hit, Some(TopBarHit::Account), "account hit at x={x}");
        assert_ne!(hit, Some(TopBarHit::Collaboration), "collab hit at x={x}");
    }
}

#[test]
fn home_surface_has_no_account_entry_when_unavailable() {
    let mut state = local_only_state();
    state.editor_ui.home.visible = true;
    let home = HomeSurface::for_editor(&state).expect("home is visible");
    let (width, height) = (1440.0, 900.0);
    let layout = home.layout(width, height);
    let account = layout.account;
    let centre = Point2D::new(
        account.origin.x + account.size.x / 2.0,
        account.origin.y + account.size.y / 2.0,
    );
    assert_ne!(home.hit_test(width, height, centre), Some(HomeHit::Account));

    for y in (0..900).step_by(4) {
        for x in (0..1440).step_by(4) {
            let hit = home.hit_test(width, height, Point2D::new(x as f32, y as f32));
            assert_ne!(hit, Some(HomeHit::Account), "account hit at ({x}, {y})");
        }
    }
}
