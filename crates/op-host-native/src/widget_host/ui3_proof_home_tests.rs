//! Proof scenes for the Figma UI3 restyle — home wave.
//!
//! One `#[test]` per surface; each renders a dark and a light PNG into
//! `OP_PROOF_DIR` (`50-home-<scene>-<theme>.png`). The harness's
//! `render_scene` is fixed at 1440x900, so scenes at other sizes (phone,
//! tablet) go through the local `render_sized` helper.

use super::super::WidgetHostNative;
use op_editor_core::size_class::{size_class, EditorSizeClass};
use op_editor_core::{EditorState, HomeFamily, TaskDraft, ThemeMode, Tool, WorkspacePhase};

const THEMES: [ThemeMode; 2] = [ThemeMode::Dark, ThemeMode::Light];

/// Paint `host` at `(w, h)` and write `<OP_PROOF_DIR>/<name>-<theme>.png`.
/// Returns the pixel at `probe` so a test can still assert on the render.
fn render_sized(
    host: &mut WidgetHostNative,
    name: &str,
    (w, h): (f32, f32),
    probe: (i32, i32),
) -> [u8; 3] {
    let mut backend = crate::backend::NativeBackend::with_dpi(1.0);
    let mut surface = skia_safe::surfaces::raster_n32_premul((w as i32, h as i32)).unwrap();
    // Frame one stamps the entrance animation; the clock then jumps past
    // it so the proof shows the settled surface, not a mid-fade frame.
    for pass in 0..3 {
        if pass == 1 {
            host.set_now_ms(60_000);
        }
        let mut frame = crate::backend::NativeFrameBackend::new(&mut backend, surface.canvas());
        host.paint(&mut frame, w, h);
    }
    if let Ok(dir) = std::env::var("OP_PROOF_DIR") {
        let suffix = match host.editor_state().editor_ui.theme_mode {
            ThemeMode::Dark => "dark",
            ThemeMode::Light => "light",
        };
        let bytes = surface
            .image_snapshot()
            .encode(None, skia_safe::EncodedImageFormat::PNG, 100)
            .expect("png encodes");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            std::path::Path::new(&dir).join(format!("{name}-{suffix}.png")),
            bytes.as_bytes(),
        )
        .unwrap();
    }
    let c = surface.peek_pixels().unwrap().get_color(probe);
    [c.r(), c.g(), c.b()]
}

/// `count` boards of `w`x`h` side by side, each with a little content so
/// the reader and workspace have something to show.
fn boards_document(count: usize, w: u32, h: u32) -> jian_ops_schema::PenDocument {
    let children: Vec<String> = (0..count)
        .map(|i| {
            format!(
                r##"{{ "type": "frame", "id": "b{i}", "name": "Page {n}", "x": {x}, "y": 0,
                     "width": {w}, "height": {h},
                     "fill": [{{ "type": "solid", "color": "#FFFFFF" }}], "children": [
                       {{ "type": "rectangle", "id": "r{i}", "x": 24, "y": 24,
                          "width": {rw}, "height": {rh}, "cornerRadius": 16,
                          "fill": [{{ "type": "solid", "color": "#0D99FF" }}] }},
                       {{ "type": "rectangle", "id": "q{i}", "x": 24, "y": {qy},
                          "width": {rw}, "height": {rh}, "cornerRadius": 16,
                          "fill": [{{ "type": "solid", "color": "#F0F0F0" }}] }} ] }}"##,
                n = i + 1,
                x = i as u32 * (w + 100),
                rw = w - 48,
                rh = h / 3,
                qy = h / 3 + 48,
            )
        })
        .collect();
    let source = format!(
        r#"{{ "version": "1.0.0", "children": [{}] }}"#,
        children.join(",")
    );
    jian_ops_schema::load_str(&source).expect("fixture").value
}

/// A Home host (the editor's default `home.visible` state) at `family`.
fn home_host(theme: ThemeMode, family: HomeFamily, class: EditorSizeClass) -> WidgetHostNative {
    let mut state = EditorState::starter();
    state.editor_ui.theme_mode = theme;
    state.editor_ui.size_class = class;
    state.editor_ui.touch = class == EditorSizeClass::Compact;
    state.editor_ui.home.visible = true;
    state.editor_ui.home.task = family;
    let mut host = WidgetHostNative::new();
    assert!(host.replace_editor_state(state));
    host.set_now_ms(5_000);
    host
}

#[test]
fn home_app_screens_renders_in_both_themes() {
    for theme in THEMES {
        let mut host = home_host(theme, HomeFamily::AppUi, EditorSizeClass::Expanded);
        let rgb = render_sized(&mut host, "50-home-apps", (1440.0, 900.0), (700, 450));
        assert!(rgb != [0, 0, 0], "{theme:?}: home painted something");
    }
}

#[test]
fn home_websites_renders_in_both_themes() {
    for theme in THEMES {
        let mut host = home_host(theme, HomeFamily::Web, EditorSizeClass::Expanded);
        render_sized(&mut host, "50-home-websites", (1440.0, 900.0), (700, 450));
    }
}

#[test]
fn home_presentations_renders_in_both_themes() {
    for theme in THEMES {
        let mut host = home_host(theme, HomeFamily::Presentation, EditorSizeClass::Expanded);
        render_sized(
            &mut host,
            "50-home-presentations",
            (1440.0, 900.0),
            (700, 450),
        );
    }
}

#[test]
fn home_compact_renders_in_both_themes() {
    for theme in THEMES {
        let mut host = home_host(theme, HomeFamily::AppUi, EditorSizeClass::Compact);
        render_sized(&mut host, "50-home-compact", (390.0, 844.0), (190, 400));
    }
}

#[test]
fn home_tablet_renders_in_both_themes() {
    for theme in THEMES {
        let dims = (1024.0, 1366.0);
        let class = size_class(dims.0, dims.1);
        let mut host = home_host(theme, HomeFamily::AppUi, class);
        host.editor_state_mut().editor_ui.touch = true;
        render_sized(&mut host, "50-home-tablet", dims, (500, 600));
    }
}

/// Seed the recent-files list the Home works row / list reads.
fn seed_recent_files(host: &mut WidgetHostNative) {
    host.editor_state_mut().editor_ui.recent_files = [
        "/tmp/Coffee app.op",
        "/tmp/Launch deck.op",
        "/tmp/Poster.op",
    ]
    .iter()
    .enumerate()
    .map(|(i, path)| op_editor_core::RecentFile {
        path: (*path).into(),
        modified_at: 1_700_000_000 + i as u64 * 86_400,
    })
    .collect();
}

#[test]
fn works_grid_tablet_renders_in_both_themes() {
    for theme in THEMES {
        let dims = (1024.0, 1366.0);
        let mut host = home_host(theme, HomeFamily::AppUi, size_class(dims.0, dims.1));
        host.editor_state_mut().editor_ui.touch = true;
        seed_recent_files(&mut host);
        render_sized(&mut host, "50-home-works-grid-tablet", dims, (500, 1250));
    }
}

#[test]
fn recent_row_desktop_renders_in_both_themes() {
    for theme in THEMES {
        let mut host = home_host(theme, HomeFamily::AppUi, EditorSizeClass::Expanded);
        seed_recent_files(&mut host);
        host.editor_state_mut().editor_ui.home.scroll_y = 400.0;
        render_sized(
            &mut host,
            "50-home-recent-desktop",
            (1440.0, 900.0),
            (700, 450),
        );
    }
}

#[test]
fn works_list_compact_renders_in_both_themes() {
    for theme in THEMES {
        let mut host = home_host(theme, HomeFamily::AppUi, EditorSizeClass::Compact);
        {
            let ui = &mut host.editor_state_mut().editor_ui;
            ui.home.works_open = true;
            ui.recent_files = ["/tmp/Coffee app.op", "/tmp/Launch deck.op"]
                .iter()
                .map(|path| op_editor_core::RecentFile {
                    path: (*path).into(),
                    modified_at: 1_700_000_000,
                })
                .collect();
        }
        render_sized(
            &mut host,
            "50-home-works-list-compact",
            (390.0, 844.0),
            (190, 400),
        );
    }
}

/// A touch host reading a finished multi-board work in the reader.
fn reading_host(theme: ThemeMode, dims: (f32, f32), count: usize) -> WidgetHostNative {
    let (w, h) = dims;
    let mut state = EditorState::from_document(boards_document(count, 375, 812));
    state.editor_ui.theme_mode = theme;
    state.editor_ui.touch = true;
    state.editor_ui.size_class = size_class(w, h);
    state.editor_ui.sidebar_open = state.editor_ui.size_class.is_rail_layout();
    let mut host = WidgetHostNative::new();
    assert!(host.replace_editor_state(state));
    host.set_now_ms(1_000);
    host.editor_state_mut()
        .editor_ui
        .open_workspace_for_generation(
            HomeFamily::AppUi,
            "A coffee pickup app, three pages",
            TaskDraft::default(),
            0,
            1_000,
            Some(Tool::Select),
        );
    host.editor_state_mut().editor_ui.workspace.phase = WorkspacePhase::Done;
    host.frame_reader_board(w, h);
    host
}

#[test]
fn works_reader_phone_renders_in_both_themes() {
    for theme in THEMES {
        let mut host = reading_host(theme, (390.0, 844.0), 3);
        render_sized(
            &mut host,
            "50-home-works-reader",
            (390.0, 844.0),
            (190, 400),
        );
    }
}

#[test]
fn works_reader_tablet_renders_in_both_themes() {
    for theme in THEMES {
        let mut host = reading_host(theme, (1366.0, 1024.0), 3);
        render_sized(
            &mut host,
            "50-home-works-reader-tablet",
            (1366.0, 1024.0),
            (700, 500),
        );
    }
}

#[test]
fn workspace_surface_renders_in_both_themes() {
    for theme in THEMES {
        let mut state = EditorState::from_document(boards_document(3, 375, 812));
        state.editor_ui.theme_mode = theme;
        let mut host = WidgetHostNative::new();
        host.install_imported_state(state);
        host.set_now_ms(2_000);
        {
            let editor = host.editor_state_mut();
            // `install_imported_state` keeps the host's own shell UI, so the
            // theme is applied after the install.
            editor.editor_ui.theme_mode = theme;
            editor.tool = Tool::Hand;
            editor.editor_ui.open_workspace_for_generation(
                HomeFamily::AppUi,
                "A coffee pickup app, three pages",
                TaskDraft::default(),
                0,
                1_000,
                Some(Tool::Select),
            );
        }
        render_sized(&mut host, "50-home-workspace", (1440.0, 900.0), (700, 450));
    }
}
