//! Proof scenes for the Figma UI3 restyle — panels wave.
//!
//! One `#[test]` per surface; each renders the scene once per theme so
//! `.proof-of-work/` gets a dark and a light PNG (`40-panel-<surface>`).

use super::super::ui3_proof::{editor_host, render_scene, PROOF_H, PROOF_W};
use super::super::WidgetHostNative;
use op_editor_core::ThemeMode;
use op_editor_ui::Rect;

/// Render `surface` in both themes. `setup` opens the panel on a host that
/// already shows the sample document; `panel_rect` names the panel's screen
/// rect (`None` for surfaces without one). When it returns a rect the test
/// probes a pixel inside it and asserts the panel really painted over the
/// canvas rather than silently never opening.
fn render_both(
    surface: &str,
    setup: impl Fn(&mut WidgetHostNative),
    panel_rect: impl Fn(&WidgetHostNative) -> Option<Rect>,
) {
    for theme in [ThemeMode::Dark, ThemeMode::Light] {
        let mut host = editor_host(theme, None);
        setup(&mut host);
        let rect = panel_rect(&host);
        let probe = match rect {
            Some(rect) => (
                (rect.origin.x + 5.0) as i32,
                (rect.origin.y + rect.size.y / 2.0) as i32,
            ),
            None => ((PROOF_W / 2.0) as i32, (PROOF_H / 2.0) as i32),
        };
        let rgb = render_scene(&mut host, &format!("40-panel-{surface}"), probe);
        if rect.is_some() {
            let canvas = match theme {
                ThemeMode::Dark => op_editor_ui::Theme::dark(),
                ThemeMode::Light => op_editor_ui::Theme::light(),
            }
            .canvas_surface;
            let canvas = [
                (canvas.r * 255.0).round() as u8,
                (canvas.g * 255.0).round() as u8,
                (canvas.b * 255.0).round() as u8,
            ];
            assert!(
                rgb != canvas,
                "{surface} {theme:?}: the panel did not paint over the canvas"
            );
        }
        assert!(
            rgb != [0, 0, 0],
            "{surface} {theme:?}: render produced black"
        );
    }
}

/// Three 16:9 boards side by side — the shape a generated deck has.
const DECK: &str = r##"{"version":"1.0.0","children":[
  {"type":"frame","id":"slide-1","name":"Cover","x":0,"y":0,"width":1920,"height":1080,
   "fill":[{"type":"solid","color":"#ffffff"}],"children":[]},
  {"type":"frame","id":"slide-2","name":"Agenda","x":2100,"y":0,"width":1920,"height":1080,
   "fill":[{"type":"solid","color":"#eeeeee"}],"children":[]},
  {"type":"frame","id":"slide-3","name":"Closing","x":4200,"y":0,"width":1920,"height":1080,
   "fill":[{"type":"solid","color":"#dddddd"}],"children":[]}
]}"##;

fn install_deck(host: &mut WidgetHostNative) {
    let document = jian_ops_schema::load_str(DECK).expect("deck parses").value;
    let theme = host.editor_state().editor_ui.theme_mode;
    let mut state = op_editor_core::EditorState::from_document(document);
    state.editor_ui.home.hide();
    state.editor_ui.theme_mode = theme;
    state.editor_ui.scenario = Some(op_editor_core::scene_template_catalog::TemplateScene::Slides);
    host.install_imported_state(state);
    host.editor_state_mut().editor_ui.home.hide();
    host.editor_state_mut().editor_ui.theme_mode = theme;
}

fn open_git_ready(host: &mut WidgetHostNative) {
    let git = &mut host.editor_state_mut().editor_ui.git_panel;
    git.open = true;
    git.loading = false;
    git.in_repo = true;
    git.has_saved_file = true;
    git.branch = Some("main".into());
    git.branches = vec!["main".into(), "feature/ui3".into()];
    git.dirty_count = 2;
    git.ahead = 1;
    git.remote_host = Some("github.com".into());
    git.commit_focused = true;
    git.changed_files = vec![
        op_editor_core::GitFileEntry {
            path: "dashboard.op".into(),
            staged: true,
            status: 'M',
        },
        op_editor_core::GitFileEntry {
            path: "assets/logo.op".into(),
            staged: false,
            status: 'A',
        },
    ];
    git.recent_commits = vec![
        op_editor_core::GitCommitSummary {
            short_hash: "a1b2c3d".into(),
            summary: "Tighten the card spacing".into(),
            author: "Mai".into(),
            time_label: "2 hours ago".into(),
            is_initial: false,
        },
        op_editor_core::GitCommitSummary {
            short_hash: "e4f5a6b".into(),
            summary: "Initial design".into(),
            author: "Mai".into(),
            time_label: "yesterday".into(),
            is_initial: true,
        },
    ];
}

#[test]
fn git_panel_ready() {
    render_both("git-ready", open_git_ready, |host| {
        host.git_panel_rect(PROOF_W, PROOF_H)
    });
}

#[test]
fn git_panel_empty() {
    render_both(
        "git-empty",
        |host| {
            let git = &mut host.editor_state_mut().editor_ui.git_panel;
            git.open = true;
            git.loading = false;
            git.in_repo = false;
            git.has_saved_file = true;
            // Hover the first card so the accent lift is on the page.
            git.empty_hovered_card = Some(0);
        },
        |host| host.git_panel_rect(PROOF_W, PROOF_H),
    );
}

#[test]
fn collab_panel() {
    use op_editor_core::{CollabAvailability, CollabConnectionPhase, CollabPanelView};
    render_both(
        "collab",
        |host| {
            let collab = &mut host.editor_state_mut().editor_ui.collab;
            collab.availability = CollabAvailability::Ready;
            collab.phase = CollabConnectionPhase::Idle;
            collab.panel.open = true;
            collab.panel.view = CollabPanelView::Join;
            // Focused address input: the 1px ring is the only outline.
            collab.panel.join_address_focused = true;
        },
        |_| None,
    );
}

#[test]
fn variables_panel() {
    use jian_ops_schema::variable::{VariableKind, VariableScalar};
    render_both(
        "variables",
        |host| {
            let state = host.editor_state_mut();
            state.editor_ui.variables_panel_open = true;
            for (name, kind, value) in [
                (
                    "primary",
                    VariableKind::Color,
                    VariableScalar::Str("#0D99FF".into()),
                ),
                (
                    "surface",
                    VariableKind::Color,
                    VariableScalar::Str("#FFFFFF".into()),
                ),
                ("radius", VariableKind::Number, VariableScalar::Num(8.0)),
                (
                    "title",
                    VariableKind::String,
                    VariableScalar::Str("Dashboard".into()),
                ),
            ] {
                assert!(state.create_variable(name, kind, value));
            }
        },
        |host| host.variables_panel_rect(PROOF_W, PROOF_H),
    );
}

#[test]
fn slides_panel() {
    render_both(
        "slides",
        |host| {
            install_deck(host);
            host.editor_state_mut().editor_ui.slides_panel.tab =
                op_editor_core::LeftPanelTab::Slides;
            host.editor_state_mut().editor_ui.slides_panel.hover =
                Some(op_editor_core::SlidesPanelTarget::Slide(1));
        },
        |_| None,
    );
}

#[test]
fn slideshow_toolbar() {
    render_both(
        "slideshow-toolbar",
        |host| {
            install_deck(host);
            assert!(host.enter_preview((PROOF_W, PROOF_H)));
            assert!(host.preview_slideshow_active(), "the deck presents");
        },
        |_| None,
    );
}

#[test]
fn scene_template_panel() {
    render_both(
        "scene-template",
        |host| {
            let center = &mut host.editor_state_mut().editor_ui.scene_template_center;
            center.open(0, true);
            // Hover the first card so the accent lift is on the page.
            center.hover = Some(0);
        },
        |host| host.scene_template_panel_rect(PROOF_W, PROOF_H),
    );
}

#[test]
fn component_browser() {
    render_both(
        "component-browser",
        |host| {
            let ui = &mut host.editor_state_mut().editor_ui;
            ui.component_browser_open = true;
            ui.component_browser_hover = Some(op_editor_core::ComponentBrowserButton::Card(1));
        },
        |host| host.component_browser_panel_rect(PROOF_W, PROOF_H),
    );
}

#[test]
fn prompt_center() {
    render_both(
        "prompt-center",
        |host| {
            let center = &mut host.editor_state_mut().editor_ui.prompt_center;
            center.open(0);
            center.hover = Some(1);
        },
        |host| host.prompt_center_panel_rect(PROOF_W, PROOF_H),
    );
}

#[test]
fn design_md_panel() {
    render_both(
        "design-md",
        |host| host.editor_state_mut().editor_ui.design_md_panel.open = true,
        |host| host.design_md_panel_rect(PROOF_W, PROOF_H),
    );
}

#[test]
fn icon_picker() {
    render_both(
        "icon-picker",
        |host| {
            let ui = &mut host.editor_state_mut().editor_ui;
            ui.icon_picker.open = true;
            ui.icon_picker.hover = Some(2);
        },
        |host| host.icon_picker_panel_rect(PROOF_W, PROOF_H),
    );
}

#[test]
fn image_fill_popover() {
    render_both(
        "image-fill-popover",
        |host| {
            let document = jian_ops_schema::load_str(
                r##"{"version":"1.0.0","children":[
                  {"type":"rectangle","id":"photo","name":"Photo","x":40,"y":40,"width":320,"height":220,
                   "fill":[{"type":"image","url":"asset.png","mode":"fill"}]}]}"##,
            )
            .expect("image document parses")
            .value;
            let theme = host.editor_state().editor_ui.theme_mode;
            *host.editor_state_mut() = op_editor_core::EditorState::from_document(document);
            let state = host.editor_state_mut();
            state.editor_ui.home.hide();
            state.editor_ui.theme_mode = theme;
            state.set_single_selection(op_editor_core::NodeId::new("photo"));
            state.editor_ui.image_fill_popover_open = true;
        },
        |_| None,
    );
}

#[test]
fn font_picker() {
    render_both(
        "font-picker",
        |host| {
            let ui = &mut host.editor_state_mut().editor_ui;
            ui.font_picker.open = true;
            ui.font_picker_purpose = Some(op_editor_core::FontPickerPurpose::PropertyText);
            host.editor_state_mut()
                .set_single_selection(op_editor_core::NodeId::new("title"));
        },
        |_| None,
    );
}
