//! Renaming a design from the Projects dock, end to end on a headless app:
//! the dock's confirmed name goes through the same pump the frame runs, and the
//! tests look at the disk, the tabs, the recents, the recovery store and the
//! published tree.

use super::super::tests::{panel, settle, temp_dir, toast_key};
use super::*;
use crate::recovery_host::test_support::{
    app_with_recovery, edit_active, entries_in, temp_dir as store_dir,
};
use crate::recovery_store::SnapshotReason;
use op_editor_core::{ProjectRequest, RenameConfirm};
use op_host_services::doc_io::save_to_path;

fn write_op(root: &Path, relative: &str) -> PathBuf {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    save_to_path(&op_editor_core::EditorState::starter(), &path).unwrap();
    path
}

struct World {
    app: DesktopApp,
    root: PathBuf,
    paths: Vec<PathBuf>,
    store: PathBuf,
}

impl World {
    /// An app with `names` (under `designs/`) opened as tabs, the last on screen.
    fn new(tag: &str, names: &[&str]) -> Self {
        let root = temp_dir(tag);
        let store = store_dir(&format!("{tag}-store"));
        let mut app = app_with_recovery(&store, "Lrename", std::process::id());
        let paths: Vec<PathBuf> = names
            .iter()
            .map(|name| write_op(&root, &format!("designs/{name}")))
            .collect();
        for path in &paths {
            assert!(app.open_document_tab(path.clone()));
        }
        settle(&mut app);
        Self {
            app,
            root,
            paths,
            store,
        }
    }

    /// What the user does: start the rename on the row, type `stem`, press Enter,
    /// and the frame executes it.
    fn rename_via_dock(&mut self, path: &Path, stem: &str) -> RenameConfirm {
        let panel = &mut self.app.host.editor_state_mut().editor_ui.projects_panel;
        assert!(
            panel.begin_rename(&path.to_string_lossy(), 0),
            "renamable row"
        );
        panel.rename.as_mut().unwrap().input.set_text(stem);
        let outcome = panel.confirm_rename();
        self.app.pump_projects();
        settle(&mut self.app);
        outcome
    }

    /// A request the widget never made: straight into the commit slot.
    fn forge(&mut self, path: &Path, stem: &str) {
        self.app
            .host
            .editor_state_mut()
            .editor_ui
            .projects_panel
            .rename_commit = Some(ProjectRenameRequest {
            path: path.to_string_lossy().into_owned(),
            new_stem: stem.to_string(),
        });
        self.app.pump_projects();
        settle(&mut self.app);
    }

    fn labels(&self) -> Vec<String> {
        panel(&self.app)
            .iter()
            .flat_map(|project| project.files.iter().map(|file| file.label.clone()))
            .collect()
    }

    fn cleanup(self) {
        let _ = std::fs::remove_dir_all(self.root);
        let _ = std::fs::remove_dir_all(self.store);
    }
}

fn editing(app: &DesktopApp) -> bool {
    app.host
        .editor_state()
        .editor_ui
        .projects_panel
        .rename
        .is_some()
}

#[test]
fn rename_keeps_extension_and_moves_the_file() {
    let mut world = World::new("move", &["a.op", "closed.op"]);
    let closed = world.paths[1].clone();
    // `closed.op` is open (every fixture is); close the file's tab first so the
    // row is a plain file on disk.
    assert!(world.app.close_document_tab(1));
    settle(&mut world.app);
    let before = std::fs::read(&closed).unwrap();

    assert_eq!(
        world.rename_via_dock(&closed, "  hero  "),
        RenameConfirm::Submitted
    );

    let renamed = closed.with_file_name("hero.op");
    assert!(renamed.is_file());
    assert!(!closed.exists());
    assert_eq!(std::fs::read(&renamed).unwrap(), before, "same bytes");
    assert!(!editing(&world.app), "the field closed");
    assert_eq!(toast_key(&world.app), None);
    assert!(world.labels().contains(&"hero.op".to_string()));
    assert!(!world.labels().contains(&"closed.op".to_string()));
    world.cleanup();
}

#[test]
fn rename_never_overwrites_an_existing_file() {
    let mut world = World::new("overwrite", &["a.op", "b.op"]);
    let (a, b) = (world.paths[0].clone(), world.paths[1].clone());
    let b_bytes = std::fs::read(&b).unwrap();
    // The listing already knows `b.op`: the widget refuses and keeps the field.
    assert_eq!(world.rename_via_dock(&a, "B"), RenameConfirm::Rejected);
    assert!(editing(&world.app));
    // A forged request gets the same answer from the host.
    world
        .app
        .host
        .editor_state_mut()
        .editor_ui
        .projects_panel
        .rename = None;
    world.forge(&a, "b");
    assert_eq!(std::fs::read(&b).unwrap(), b_bytes, "b.op untouched");
    assert!(a.exists(), "a.op still there");
    assert_eq!(
        world.app.documents.slot_paths()[0].as_deref(),
        Some(a.as_path())
    );
    world.cleanup();
}

#[test]
fn a_name_taken_on_disk_but_not_listed_keeps_the_field_open_with_a_message() {
    let mut world = World::new("unlisted", &["a.op"]);
    let a = world.paths[0].clone();
    // A file the dock has not seen yet (created by something else).
    std::fs::write(a.with_file_name("fresh.op"), "other").unwrap();
    assert_eq!(world.rename_via_dock(&a, "fresh"), RenameConfirm::Submitted);
    let edit = world
        .app
        .host
        .editor_state()
        .editor_ui
        .projects_panel
        .rename
        .clone()
        .expect("the field stays open");
    assert_eq!(edit.error, Some(RenameNameError::Taken));
    assert_eq!(
        std::fs::read_to_string(a.with_file_name("fresh.op")).unwrap(),
        "other"
    );
    assert!(a.exists());
    world.cleanup();
}

#[test]
fn open_tab_follows_the_new_path_and_keeps_unsaved_edits() {
    let mut world = World::new("follow", &["a.op", "b.op"]);
    let b = world.paths[1].clone();
    edit_active(&mut world.app, "unsaved-b");
    let revision = world.app.host.editor_state().document_revision();
    let epoch = world.app.host.document_epoch();

    world.rename_via_dock(&b, "renamed-b");

    let new_b = b.with_file_name("renamed-b.op");
    assert_eq!(world.app.current_path.as_deref(), Some(new_b.as_path()));
    assert_eq!(
        world.app.documents.slot_paths()[1].as_deref(),
        Some(new_b.as_path())
    );
    assert_eq!(world.app.documents.len(), 2, "no tab opened or closed");
    let state = world.app.host.editor_state();
    assert_eq!(
        state.doc.name.as_deref(),
        Some("unsaved-b"),
        "the edit is kept"
    );
    assert!(state.is_dirty(), "and still unsaved");
    assert_eq!(state.document_revision(), revision);
    assert_eq!(world.app.host.document_epoch(), epoch, "same document");
    let tabs = world.app.documents.tab_views(true, "");
    assert_eq!(tabs[1].label, "renamed-b.op");
    assert!(tabs[1].dirty);
    world.cleanup();
}

#[test]
fn dirty_tab_rename_does_not_reload_from_disk() {
    let mut world = World::new("noreload", &["a.op"]);
    let a = world.paths[0].clone();
    edit_active(&mut world.app, "only-in-memory");
    world.rename_via_dock(&a, "b");
    let new_a = a.with_file_name("b.op");
    assert_eq!(
        world.app.host.editor_state().doc.name.as_deref(),
        Some("only-in-memory")
    );
    let on_disk = std::fs::read_to_string(&new_a).unwrap();
    assert!(
        !on_disk.contains("only-in-memory"),
        "the rename moved the file, it did not save the edit"
    );
    world.cleanup();
}

#[test]
fn parked_tab_follows_the_new_path() {
    let mut world = World::new("parked", &["a.op", "b.op"]);
    let a = world.paths[0].clone();
    assert_ne!(world.app.documents.active_index(), 0, "a.op is parked");
    world.rename_via_dock(&a, "alpha");
    let new_a = a.with_file_name("alpha.op");
    assert_eq!(
        world.app.documents.slot_paths()[0].as_deref(),
        Some(new_a.as_path())
    );
    let slot = world.app.documents.slot_mut(0).unwrap();
    assert_eq!(
        slot.parked.as_ref().unwrap().scope().file_name_display(),
        Some("alpha.op")
    );
    // Switching to it shows the new name.
    assert!(world.app.activate_document_slot(0));
    assert_eq!(world.app.current_path.as_deref(), Some(new_a.as_path()));
    world.cleanup();
}

#[test]
fn active_document_title_updates() {
    let mut world = World::new("title", &["a.op"]);
    let a = world.paths[0].clone();
    world.rename_via_dock(&a, "poster");
    let new_a = a.with_file_name("poster.op");
    assert_eq!(world.app.current_path.as_deref(), Some(new_a.as_path()));
    assert_eq!(
        world
            .app
            .host
            .editor_state()
            .editor_ui
            .file_name_display
            .as_deref(),
        Some("poster.op"),
        "the name the chrome shows"
    );
    assert_eq!(
        world.app.documents.tab_views(false, "")[0].label,
        "poster.op"
    );
    world.cleanup();
}

#[test]
fn mcp_list_documents_reports_the_new_path() {
    let mut world = World::new("mcp", &["a.op", "b.op"]);
    let a = world.paths[0].clone();
    world.rename_via_dock(&a, "alpha");
    let listings = world.app.mcp_document_listings();
    let new_a = a.with_file_name("alpha.op");
    assert_eq!(listings[0].path.as_deref(), Some(new_a.as_path()));
    assert_eq!(listings[0].label, "alpha.op");
    assert!(listings
        .iter()
        .all(|listing| listing.path.as_deref() != Some(a.as_path())));
    world.cleanup();
}

#[test]
fn recent_files_follow_the_rename() {
    let mut world = World::new("recents", &["a.op", "b.op"]);
    let a = world.paths[0].clone();
    world.rename_via_dock(&a, "alpha");
    let recent: Vec<String> = world
        .app
        .host
        .editor_state()
        .editor_ui
        .recent_files
        .iter()
        .map(|recent| recent.path.clone())
        .collect();
    let new_a = a.with_file_name("alpha.op").to_string_lossy().into_owned();
    assert!(recent.contains(&new_a), "{recent:?}");
    assert!(
        !recent.contains(&a.to_string_lossy().into_owned()),
        "{recent:?}"
    );
    world.cleanup();
}

#[test]
fn rename_of_a_dirty_tab_rewrites_its_recovery_entry() {
    let mut world = World::new("recovery", &["a.op"]);
    let a = world.paths[0].clone();
    edit_active(&mut world.app, "unsaved");
    world
        .app
        .snapshot_dirty_documents(SnapshotReason::Autosnapshot);
    world.app.flush_recovery();
    let before = entries_in(&world.store);
    assert_eq!(before.len(), 1);
    assert_eq!(
        before[0].manifest.original_path.as_deref(),
        Some(a.to_string_lossy().as_ref())
    );

    world.rename_via_dock(&a, "renamed");

    let new_a = a.with_file_name("renamed.op");
    let after = entries_in(&world.store);
    assert_eq!(after.len(), 1, "one entry, not two");
    assert_eq!(
        after[0].manifest.original_path.as_deref(),
        Some(new_a.to_string_lossy().as_ref()),
        "a crash now would restore the file under its new name"
    );
    world.cleanup();
}

#[test]
fn listing_refreshes_without_losing_collapse_state() {
    let root = temp_dir("listing");
    write_op(&root, "top.op");
    let renamed = write_op(&root, "sub/b.op");
    write_op(&root, "sub/c.op");
    write_op(&root, "other/x.op");
    let mut app = DesktopApp::new(None);
    assert!(app.open_project(&root));
    settle(&mut app);
    for folder in ["other"] {
        app.host
            .editor_state_mut()
            .editor_ui
            .projects_panel
            .pending_request = Some(ProjectRequest::ToggleFolder(
            root.to_string_lossy().into_owned(),
            folder.to_string(),
        ));
        app.pump_projects();
    }
    settle(&mut app);
    app.host
        .editor_state_mut()
        .editor_ui
        .projects_panel
        .scroll
        .offset = 7.0;
    assert_eq!(panel(&app)[0].collapsed_folders, ["other"]);

    let panel_state = &mut app.host.editor_state_mut().editor_ui.projects_panel;
    assert!(panel_state.begin_rename(&renamed.to_string_lossy(), 0));
    panel_state.rename.as_mut().unwrap().input.set_text("bee");
    assert_eq!(panel_state.confirm_rename(), RenameConfirm::Submitted);
    // One frame: the row is already the new name, before any rescan finished.
    app.pump_projects();
    let labels: Vec<String> = panel(&app)[0]
        .files
        .iter()
        .map(|f| f.label.clone())
        .collect();
    assert!(labels.contains(&"bee.op".to_string()), "{labels:?}");
    assert!(!labels.contains(&"b.op".to_string()), "{labels:?}");
    settle(&mut app);
    let after = &panel(&app)[0];
    assert!(after.files.iter().any(|f| f.label == "bee.op"));
    assert!(!after.files.iter().any(|f| f.label == "b.op"));
    assert_eq!(after.collapsed_folders, ["other"], "folds survive");
    assert!(after.expanded);
    assert_eq!(
        app.host
            .editor_state()
            .editor_ui
            .projects_panel
            .scroll
            .offset,
        7.0,
        "the scroll position survives"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn fig_rows_and_folders_are_not_renamable() {
    let mut world = World::new("fig", &["a.op"]);
    let fig = world.root.join("brand.fig");
    std::fs::write(&fig, b"fig").unwrap();
    let folder = world.root.join("sub.op");
    std::fs::create_dir_all(&folder).unwrap();
    for target in [&fig, &folder] {
        world.forge(target, "renamed");
        assert_eq!(
            toast_key(&world.app).as_deref(),
            Some("projects.renameFailed")
        );
        world.app.host.editor_state_mut().editor_ui.editor_toast = None;
    }
    assert!(fig.is_file());
    assert!(folder.is_dir());
    assert!(!world.root.join("renamed.fig").exists());
    assert!(!world.root.join("renamed.op").exists());
    world.cleanup();
}

#[test]
fn forged_request_outside_the_project_roots_is_refused() {
    let mut world = World::new("forged", &["a.op"]);
    let elsewhere = temp_dir("forged-out");
    let outside = write_op(&elsewhere, "secret.op");
    world.forge(&outside, "taken");
    assert!(outside.is_file(), "the outside file was not renamed");
    assert!(!elsewhere.join("taken.op").exists());
    assert_eq!(
        toast_key(&world.app).as_deref(),
        Some("projects.renameFailed")
    );
    // A `..` hop from inside the project does not get out either.
    let hop = world
        .root
        .join("designs")
        .join("..")
        .join("..")
        .join(elsewhere.file_name().unwrap())
        .join("secret.op");
    world.forge(&hop, "taken");
    assert!(outside.is_file());
    let _ = std::fs::remove_dir_all(elsewhere);
    world.cleanup();
}

#[cfg(unix)]
#[test]
fn symlink_escape_is_refused() {
    let mut world = World::new("symlink", &["a.op"]);
    let elsewhere = temp_dir("symlink-out");
    let real = write_op(&elsewhere, "real.op");
    let link = world.root.join("designs").join("link.op");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    world.forge(&link, "renamed");
    assert!(real.is_file(), "the target of the link was not touched");
    assert!(
        std::fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink(),
        "the link itself was not renamed either"
    );
    let dir_link = world.root.join("hop");
    std::os::unix::fs::symlink(&elsewhere, &dir_link).unwrap();
    world.forge(&dir_link.join("real.op"), "renamed");
    assert!(real.is_file());
    assert!(!elsewhere.join("renamed.op").exists());
    let _ = std::fs::remove_dir_all(elsewhere);
    world.cleanup();
}

#[test]
fn a_rename_is_refused_while_a_save_is_being_written() {
    let mut world = World::new("saving", &["a.op"]);
    let a = world.paths[0].clone();
    edit_active(&mut world.app, "to-save");
    assert!(world.app.request_background_save());
    assert!(world.app.save_session.is_active(), "the save is in flight");
    assert_eq!(
        world.app.rename_design_file(&a, "late"),
        Err(RenameRefusal::SaveInProgress)
    );
    assert!(a.exists(), "nothing moved");
    world.app.finish_background_saves();
    assert_eq!(world.app.rename_design_file(&a, "late"), Ok(()));
    assert!(a.with_file_name("late.op").is_file());
    assert!(
        !a.exists(),
        "the finished save did not bring the old name back"
    );
    world.cleanup();
}

#[test]
fn a_file_open_in_two_tabs_is_not_renamed() {
    let mut world = World::new("twice", &["a.op", "b.op"]);
    let (a, b) = (world.paths[0].clone(), world.paths[1].clone());
    // The invariant says this cannot happen; force it and check the answer.
    world.app.documents.slot_mut(0).unwrap().local.path = Some(b.clone());
    assert_eq!(
        world.app.rename_design_file(&b, "c"),
        Err(RenameRefusal::OpenInSeveralTabs)
    );
    assert!(b.exists());
    let _ = a;
    world.cleanup();
}

#[cfg(unix)]
#[test]
fn failed_rename_leaves_row_and_tab_unchanged() {
    use std::os::unix::fs::PermissionsExt;
    let mut world = World::new("failed", &["a.op", "b.op"]);
    let b = world.paths[1].clone();
    let designs = b.parent().unwrap().to_path_buf();
    // A folder the user cannot write to: the OS refuses the rename.
    std::fs::set_permissions(&designs, std::fs::Permissions::from_mode(0o555)).unwrap();
    let denied = std::fs::File::create(designs.join("probe")).is_err();
    if denied {
        world.rename_via_dock(&b, "renamed");
        std::fs::set_permissions(&designs, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(b.exists(), "the file is where it was");
        assert!(!designs.join("renamed.op").exists());
        assert_eq!(world.app.current_path.as_deref(), Some(b.as_path()));
        assert_eq!(
            world.app.documents.slot_paths()[1].as_deref(),
            Some(b.as_path())
        );
        assert_eq!(
            toast_key(&world.app).as_deref(),
            Some("projects.renameFailed")
        );
        assert!(!editing(&world.app), "the field closed");
        assert!(
            world.labels().contains(&"b.op".to_string()),
            "the row is as it was"
        );
    } else {
        // Running with privileges that ignore permissions: nothing to prove.
        std::fs::set_permissions(&designs, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    world.cleanup();
}

#[test]
fn a_vanished_source_is_reported_and_changes_nothing() {
    let mut world = World::new("vanished", &["a.op", "b.op"]);
    let b = world.paths[1].clone();
    std::fs::remove_file(&b).unwrap();
    world.rename_via_dock(&b, "renamed");
    assert_eq!(
        world.app.current_path.as_deref(),
        Some(b.as_path()),
        "the tab is untouched"
    );
    assert_eq!(
        toast_key(&world.app).as_deref(),
        Some("projects.renameFailed")
    );
    world.cleanup();
}
