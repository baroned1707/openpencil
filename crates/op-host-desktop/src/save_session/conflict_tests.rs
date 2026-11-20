//! A user save never replaces a file the editor cannot vouch for without the
//! user's say. Everything runs headless: the dialog answers are injected.

use super::super::conflict::SaveConflict;
use super::*;
use crate::message_dialog::Choice;
use crate::recovery_host::test_support::*;
use op_editor_core::EditorState;
use op_host_services::disk_fingerprint::DiskFingerprint;
use op_host_services::doc_io::version_backup::{enable_version_history, versions_dir_for_document};

/// Answers the questions from a script and records what was asked.
struct FakeUi {
    answers: Vec<ConflictChoice>,
    copy: Option<PathBuf>,
    asked: Vec<SaveConflict>,
    copy_defaults: Vec<PathBuf>,
}

impl FakeUi {
    fn answering(answers: &[ConflictChoice]) -> Self {
        Self {
            answers: answers.to_vec(),
            copy: None,
            asked: Vec::new(),
            copy_defaults: Vec::new(),
        }
    }

    /// Fails the test if a prompt appears.
    fn silent() -> Self {
        Self::answering(&[])
    }
}

impl ConflictUi for FakeUi {
    fn ask(
        &mut self,
        _: op_editor_core::Locale,
        _: &str,
        conflict: SaveConflict,
    ) -> ConflictChoice {
        self.asked.push(conflict);
        assert!(!self.answers.is_empty(), "unexpected prompt: {conflict:?}");
        self.answers.remove(0)
    }

    fn pick_copy_path(&mut self, _: &EditorState, default: &Path) -> Option<PathBuf> {
        self.copy_defaults.push(default.to_path_buf());
        Some(self.copy.clone().unwrap_or_else(|| default.to_path_buf()))
    }
}

const EXTERNAL: &[u8] = b"{\"edited\":\"somewhere else\",\"padding\":\"xxxxxxxxxxxxxxxxxxxx\"}";

/// An app with `name` saved in a fresh work dir and open on screen.
fn app_with_file(tag: &str, name: &str) -> (DesktopApp, PathBuf, PathBuf) {
    let work = temp_dir(tag);
    let path = write_document(&work, name);
    let mut app = DesktopApp::new(None);
    assert!(app.open_document_tab(path.clone()));
    (app, path, work)
}

/// Someone else rewrites the file (a later mtime guaranteed).
fn rewrite_externally(path: &Path, bytes: &[u8]) {
    std::thread::sleep(std::time::Duration::from_millis(25));
    std::fs::write(path, bytes).unwrap();
}

fn saved_name(path: &Path) -> Option<String> {
    op_host_services::doc_io::load_editor_state(path, op_editor_core::Locale::EnUs)
        .unwrap()
        .doc
        .name
}

/// Cmd+S, then wait for the worker.
fn save(app: &mut DesktopApp, ui: &mut FakeUi) -> (bool, bool) {
    let queued = app.request_background_save_with(ui);
    (queued, app.finish_background_saves())
}

/// The kept versions of `path`, oldest first.
fn kept_versions(path: &Path) -> Vec<Vec<u8>> {
    let dir = versions_dir_for_document(path).expect("settings dir");
    let mut names: Vec<_> = std::fs::read_dir(dir)
        .map(|listing| {
            listing
                .flatten()
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .filter(|name| name.ends_with(".op"))
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
        .iter()
        .map(|name| std::fs::read(versions_dir_for_document(path).unwrap().join(name)).unwrap())
        .collect()
}

#[test]
fn classify_covers_every_row_of_the_prompt_table() {
    let work = temp_dir("classify");
    let path = work.join("a.op");
    // A file that is gone is recreated whatever the record says.
    assert_eq!(classify(None, &path), None);
    assert_eq!(classify(Some(DiskFingerprint::Stale), &path), None);
    std::fs::write(&path, b"one").unwrap();
    let known = DiskFingerprint::capture(&path).unwrap();
    assert_eq!(classify(Some(known), &path), None, "record matches");
    assert_eq!(classify(None, &path), Some(SaveConflict::Unconfirmed));
    assert_eq!(
        classify(Some(DiskFingerprint::Stale), &path),
        Some(SaveConflict::Unconfirmed)
    );
    assert_eq!(
        classify(Some(DiskFingerprint::Missing), &path),
        Some(SaveConflict::ChangedOnDisk),
        "a file appeared where none was"
    );
    rewrite_externally(&path, b"two!");
    assert_eq!(
        classify(Some(known), &path),
        Some(SaveConflict::ChangedOnDisk)
    );
    let _ = std::fs::remove_dir_all(work);
}

#[test]
fn the_copy_name_is_unused_and_never_overwrites() {
    let work = temp_dir("copy-name");
    let path = work.join("design.op");
    assert_eq!(unique_copy_path(&path), work.join("design-copy.op"));
    std::fs::write(work.join("design-copy.op"), b"taken").unwrap();
    assert_eq!(unique_copy_path(&path), work.join("design-copy-2.op"));
    std::fs::write(work.join("design-copy-2.op"), b"taken").unwrap();
    assert_eq!(unique_copy_path(&path), work.join("design-copy-3.op"));
    let _ = std::fs::remove_dir_all(work);
}

#[test]
fn cmd_s_with_an_unchanged_file_saves_without_a_prompt() {
    let (mut app, path, work) = app_with_file("silent", "a.op");
    edit_active(&mut app, "edited");
    let mut ui = FakeUi::silent();
    assert_eq!(save(&mut app, &mut ui), (true, true));
    assert!(ui.asked.is_empty());
    assert_eq!(saved_name(&path).as_deref(), Some("edited"));
    let _ = std::fs::remove_dir_all(work);
}

#[test]
fn cmd_s_after_an_external_change_prompts_and_cancel_leaves_the_disk_bytes() {
    let (mut app, path, work) = app_with_file("cancel", "a.op");
    edit_active(&mut app, "mine");
    rewrite_externally(&path, EXTERNAL);
    let mut ui = FakeUi::answering(&[ConflictChoice::Cancel]);
    assert_eq!(save(&mut app, &mut ui), (false, true));
    assert_eq!(ui.asked, vec![SaveConflict::ChangedOnDisk]);
    assert_eq!(std::fs::read(&path).unwrap(), EXTERNAL, "nothing written");
    assert!(app.document_is_dirty(), "the edit is still unsaved");
    let _ = std::fs::remove_dir_all(work);
}

#[test]
fn overwrite_writes_and_the_backup_holds_the_external_version() {
    enable_version_history();
    let (mut app, path, work) = app_with_file("overwrite", "a.op");
    edit_active(&mut app, "mine");
    rewrite_externally(&path, EXTERNAL);
    let mut ui = FakeUi::answering(&[ConflictChoice::Overwrite]);
    assert_eq!(save(&mut app, &mut ui), (true, true));
    assert_eq!(saved_name(&path).as_deref(), Some("mine"));
    assert_eq!(
        kept_versions(&path).last().map(Vec::as_slice),
        Some(EXTERNAL),
        "the version that was on disk is recoverable"
    );
    assert!(!app.document_is_dirty());
    // The write recorded the new baseline: the next save is silent again.
    edit_active(&mut app, "mine, later");
    let mut silent = FakeUi::silent();
    assert_eq!(save(&mut app, &mut silent), (true, true));
    assert!(silent.asked.is_empty());
    let _ = std::fs::remove_dir_all(work);
}

#[test]
fn save_a_copy_writes_name_copy_and_leaves_the_original() {
    let (mut app, path, work) = app_with_file("copy", "a.op");
    edit_active(&mut app, "mine");
    rewrite_externally(&path, EXTERNAL);
    let mut ui = FakeUi::answering(&[ConflictChoice::SaveCopy]);
    assert_eq!(save(&mut app, &mut ui), (true, true));
    let copy = work.join("a-copy.op");
    assert_eq!(ui.copy_defaults, vec![copy.clone()]);
    assert_eq!(
        saved_name(&copy).as_deref(),
        Some("mine"),
        "what is on screen"
    );
    assert_eq!(
        std::fs::read(&path).unwrap(),
        EXTERNAL,
        "original untouched"
    );
    assert_eq!(app.current_path.as_deref(), Some(copy.as_path()));
    // The offered name is never one that exists.
    let (mut app, path, work2) = app_with_file("copy-taken", "a.op");
    std::fs::write(work2.join("a-copy.op"), b"taken").unwrap();
    edit_active(&mut app, "mine");
    rewrite_externally(&path, EXTERNAL);
    let mut ui = FakeUi::answering(&[ConflictChoice::SaveCopy]);
    let _ = save(&mut app, &mut ui);
    assert_eq!(ui.copy_defaults, vec![work2.join("a-copy-2.op")]);
    assert_eq!(std::fs::read(work2.join("a-copy.op")).unwrap(), b"taken");
    let _ = std::fs::remove_dir_all(work);
    let _ = std::fs::remove_dir_all(work2);
}

#[test]
fn an_unknown_baseline_prompts_and_a_missing_file_does_not() {
    let (mut app, path, work) = app_with_file("unknown", "a.op");
    edit_active(&mut app, "mine");
    app.disk_baselines = Default::default();
    let mut ui = FakeUi::answering(&[ConflictChoice::Cancel]);
    assert_eq!(save(&mut app, &mut ui), (false, true));
    assert_eq!(ui.asked, vec![SaveConflict::Unconfirmed]);

    std::fs::remove_file(&path).unwrap();
    let mut silent = FakeUi::silent();
    assert_eq!(save(&mut app, &mut silent), (true, true));
    assert!(silent.asked.is_empty(), "a missing file is just recreated");
    assert_eq!(saved_name(&path).as_deref(), Some("mine"));
    let _ = std::fs::remove_dir_all(work);
}

#[test]
fn the_close_and_quit_save_path_is_guarded_the_same_way() {
    let (mut app, path, work) = app_with_file("close", "a.op");
    edit_active(&mut app, "mine");
    rewrite_externally(&path, EXTERNAL);
    // The user answers Save to the unsaved-changes prompt; the guard then asks,
    // and with no dialog available (`default_ui` refuses) the close is aborted.
    let mut ask = |_: &str, _: &str| Some(Choice::Yes);
    assert_eq!(
        app.close_decision_for_active_document(&mut ask),
        crate::app_state::CloseDecision::Abort
    );
    assert_eq!(std::fs::read(&path).unwrap(), EXTERNAL);
    assert!(app.document_is_dirty());
    assert!(!app.confirm_close_with(|_, _| Some(Choice::Yes)));
    assert_eq!(std::fs::read(&path).unwrap(), EXTERNAL);
    let _ = std::fs::remove_dir_all(work);
}

#[test]
fn save_as_onto_an_existing_file_nobody_recorded_prompts() {
    enable_version_history();
    let (mut app, _own, work) = app_with_file("save-as-existing", "a.op");
    let target = work.join("b.op");
    std::fs::write(&target, EXTERNAL).unwrap();
    edit_active(&mut app, "mine");

    let mut cancel = FakeUi::answering(&[ConflictChoice::Cancel]);
    assert!(!app.save_as_to_picked_path_with(target.clone(), false, &mut cancel));
    assert_eq!(cancel.asked, vec![SaveConflict::Unconfirmed]);
    assert_eq!(std::fs::read(&target).unwrap(), EXTERNAL);

    let mut overwrite = FakeUi::answering(&[ConflictChoice::Overwrite]);
    assert!(app.save_as_to_picked_path_with(target.clone(), false, &mut overwrite));
    assert!(app.finish_background_saves());
    assert_eq!(saved_name(&target).as_deref(), Some("mine"));
    assert_eq!(
        kept_versions(&target).last().map(Vec::as_slice),
        Some(EXTERNAL)
    );
    let _ = std::fs::remove_dir_all(work);
}

#[test]
fn save_as_onto_a_new_path_or_the_own_unchanged_file_does_not_prompt() {
    let (mut app, own, work) = app_with_file("save-as-new", "a.op");
    edit_active(&mut app, "mine");
    let mut silent = FakeUi::silent();
    assert!(app.save_as_to_picked_path_with(work.join("new.op"), false, &mut silent));
    assert!(app.finish_background_saves());
    assert_eq!(saved_name(&work.join("new.op")).as_deref(), Some("mine"));

    // Now bound to new.op, whose record is the editor's own write.
    edit_active(&mut app, "again");
    assert!(app.save_as_to_picked_path_with(work.join("new.op"), false, &mut silent));
    assert!(app.finish_background_saves());
    assert!(silent.asked.is_empty());
    assert_eq!(saved_name(&work.join("new.op")).as_deref(), Some("again"));
    assert!(own.is_file());
    let _ = std::fs::remove_dir_all(work);
}

#[test]
fn a_second_save_while_the_first_is_running_is_not_a_conflict() {
    let (mut app, path, work) = app_with_file("inflight", "a.op");
    edit_active(&mut app, "first");
    let mut silent = FakeUi::silent();
    assert!(app.request_background_save_with(&mut silent));
    // The first write is running or queued, so the record is about to change.
    assert!(app.save_session.targets_path(&path));
    edit_active(&mut app, "second");
    assert!(app.request_background_save_with(&mut silent));
    assert!(app.finish_background_saves());
    assert!(
        silent.asked.is_empty(),
        "no false conflict from our own write"
    );
    assert_eq!(saved_name(&path).as_deref(), Some("second"));
    let _ = std::fs::remove_dir_all(work);
}

#[test]
fn every_open_path_records_the_baseline_of_the_loaded_file() {
    let work = temp_dir("load");
    let first = write_document(&work, "a.op");
    let empty = work.join("empty.op");
    std::fs::write(&empty, b"").unwrap();
    let mut app = DesktopApp::new(None);
    assert!(app.open_document_tab(first.clone()));
    assert_eq!(
        app.disk_baselines.get(&first),
        DiskFingerprint::capture(&first).ok()
    );
    assert!(matches!(
        app.disk_baselines.get(&first),
        Some(DiskFingerprint::Present { .. })
    ));
    // An empty file is initialised by the loader: the baseline is what it
    // wrote, not the zero-byte file it found.
    assert!(app.open_document_tab(empty.clone()));
    assert_eq!(
        app.disk_baselines.get(&empty),
        DiskFingerprint::capture(&empty).ok()
    );
    assert_eq!(classify(app.disk_baselines.get(&empty), &empty), None);
    // File > Open Recent goes through the same funnel.
    assert!(app.activate_document_slot(0));
    let second = write_document(&work, "b.op");
    app.host.editor_state_mut().editor_ui.recent_files = vec![op_editor_core::RecentFile {
        path: second.to_string_lossy().into_owned(),
        modified_at: 0,
    }];
    assert!(app.open_recent_document(0));
    assert_eq!(
        app.disk_baselines.get(&second),
        DiskFingerprint::capture(&second).ok()
    );
    let _ = std::fs::remove_dir_all(work);
}

#[test]
fn the_load_baseline_is_stale_when_the_file_changed_during_the_load() {
    let work = temp_dir("load-race");
    let path = work.join("a.op");
    std::fs::write(&path, b"one").unwrap();
    let before = DiskFingerprint::capture(&path).ok();
    let mut app = DesktopApp::new(None);

    app.record_baseline_after_load(&path, &path, before, false);
    assert_eq!(app.disk_baselines.get(&path), before);

    rewrite_externally(&path, b"changed while being read");
    app.record_baseline_after_load(&path, &path, before, false);
    assert_eq!(app.disk_baselines.get(&path), Some(DiskFingerprint::Stale));
    assert_eq!(
        classify(app.disk_baselines.get(&path), &path),
        Some(SaveConflict::Unconfirmed)
    );

    // The editor wrote the file itself during the load: trust what it left.
    app.record_baseline_after_load(&path, &path, before, true);
    assert_eq!(
        app.disk_baselines.get(&path),
        DiskFingerprint::capture(&path).ok()
    );
    let _ = std::fs::remove_dir_all(work);
}

#[test]
fn a_git_reload_refreshes_the_baseline_but_another_tab_keeps_its_old_one() {
    let work = temp_dir("git-reload");
    let first = write_document(&work, "a.op");
    let second = write_document(&work, "b.op");
    let mut app = DesktopApp::new(None);
    assert!(app.open_document_tab(first.clone()));
    assert!(app.open_document_tab(second.clone()));
    assert!(app.activate_document_slot(0));

    // `git checkout` rewrites both files; the editor reloads the active one.
    std::thread::sleep(std::time::Duration::from_millis(25));
    let mut from_git = EditorState::starter();
    from_git.doc.name = Some("from git".into());
    op_host_services::doc_io::save_to_path(&from_git, &first).unwrap();
    std::fs::write(&second, EXTERNAL).unwrap();
    app.reload_tracked_document();

    assert_eq!(
        app.host.editor_state().doc.name.as_deref(),
        Some("from git")
    );
    assert_eq!(
        classify(app.disk_baselines.get(&first), &first),
        None,
        "no false conflict after a branch switch"
    );
    edit_active(&mut app, "after switch");
    let mut silent = FakeUi::silent();
    assert_eq!(save(&mut app, &mut silent), (true, true));
    assert!(silent.asked.is_empty());

    // The other tab was not reloaded: its next save asks, as it must.
    assert!(app.activate_document_slot(1));
    edit_active(&mut app, "other tab");
    let mut ui = FakeUi::answering(&[ConflictChoice::Cancel]);
    assert_eq!(save(&mut app, &mut ui), (false, true));
    assert_eq!(ui.asked, vec![SaveConflict::ChangedOnDisk]);
    assert_eq!(std::fs::read(&second).unwrap(), EXTERNAL);
    let _ = std::fs::remove_dir_all(work);
}

#[test]
fn a_recovery_restore_binds_a_stale_baseline_so_the_first_save_asks() {
    let work = temp_dir("recover-work");
    let recovery = temp_dir("recover-store");
    let path = write_document(&work, "a.op");
    std::fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .and_then(|file| {
            file.set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(7200))
        })
        .expect("age the file");
    let mut previous = app_with_recovery(&recovery, "Lprev", DEAD_PID);
    assert!(previous.open_document_tab(path.clone()));
    edit_active(&mut previous, "recovered-edit");
    previous.recovery_on_exit();

    let mut app = app_with_recovery(&recovery, "Lnew", std::process::id());
    assert_eq!(app.offer_recovery_with(|_, _| Some(Choice::Yes)), 1);
    assert_eq!(app.current_path.as_deref(), Some(path.as_path()), "bound");
    assert_eq!(app.disk_baselines.get(&path), Some(DiskFingerprint::Stale));
    // The per-frame sweep must not "fix" it from the file's current state.
    app.sweep_disk_baselines();
    assert_eq!(app.disk_baselines.get(&path), Some(DiskFingerprint::Stale));

    let before = std::fs::read(&path).unwrap();
    let mut ui = FakeUi::answering(&[ConflictChoice::Cancel]);
    assert_eq!(save(&mut app, &mut ui), (false, true));
    assert_eq!(ui.asked, vec![SaveConflict::Unconfirmed]);
    assert_eq!(std::fs::read(&path).unwrap(), before);
    let _ = std::fs::remove_dir_all(work);
    let _ = std::fs::remove_dir_all(recovery);
}
