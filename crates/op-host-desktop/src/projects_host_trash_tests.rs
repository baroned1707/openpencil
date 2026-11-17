//! Moving a design to the Trash from the Projects dock, end to end on a headless
//! app with a fake Trash: the dock's confirmed path goes through the same pump
//! the frame runs, and the tests look at the disk, the fake's log, the toast and
//! the published tree. The real Trash is never touched.

use super::super::tests::{panel, settle, temp_dir, toast_key};
use super::super::*;
use super::*;
use crate::recovery_host::test_support::{app_with_recovery, edit_active, temp_dir as store_dir};
use op_host_services::doc_io::save_to_path;
use std::sync::Mutex;

/// A Trash that moves the file into a folder of its own and records every call.
/// It offers no delete, like the trait.
struct FakeTrash {
    bin: PathBuf,
    calls: Mutex<Vec<PathBuf>>,
    fail: bool,
    supported: bool,
}

impl FakeTrash {
    fn new(bin: &Path) -> Arc<Self> {
        Arc::new(Self {
            bin: bin.to_path_buf(),
            calls: Mutex::new(Vec::new()),
            fail: false,
            supported: true,
        })
    }

    fn failing(bin: &Path) -> Arc<Self> {
        Arc::new(Self {
            fail: true,
            ..Arc::try_unwrap(Self::new(bin)).ok().unwrap()
        })
    }

    fn unsupported(bin: &Path) -> Arc<Self> {
        Arc::new(Self {
            supported: false,
            ..Arc::try_unwrap(Self::new(bin)).ok().unwrap()
        })
    }

    fn calls(&self) -> Vec<PathBuf> {
        self.calls.lock().unwrap().clone()
    }
}

impl TrashBackend for FakeTrash {
    fn supported(&self) -> bool {
        self.supported
    }

    fn trash(&self, path: &Path) -> io::Result<()> {
        self.calls.lock().unwrap().push(path.to_path_buf());
        if self.fail {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "no Trash here",
            ));
        }
        std::fs::create_dir_all(&self.bin)?;
        std::fs::rename(path, self.bin.join(path.file_name().unwrap()))
    }
}

struct World {
    app: DesktopApp,
    root: PathBuf,
    bin: PathBuf,
    store: PathBuf,
}

impl World {
    /// An app with `names` (under `designs/`) written; none is open yet.
    fn new(tag: &str, names: &[&str]) -> Self {
        let root = temp_dir(tag);
        let store = store_dir(&format!("{tag}-store"));
        let bin = root.with_file_name(format!(
            "{}-bin",
            root.file_name().unwrap().to_string_lossy()
        ));
        let mut app = app_with_recovery(&store, "Ltrash", std::process::id());
        for name in names {
            let path = root.join("designs").join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            save_to_path(&op_editor_core::EditorState::starter(), &path).unwrap();
        }
        // Make the project known to the dock: open one design then close it.
        let first = root.join("designs").join(names[0]);
        assert!(app.open_document_tab(first));
        settle(&mut app);
        let mut world = Self {
            app,
            root,
            bin,
            store,
        };
        world.use_backend(FakeTrash::new(&world.bin.clone()));
        world
    }

    fn use_backend(&mut self, backend: Arc<FakeTrash>) {
        self.app.projects_host.set_trash_backend(backend);
        self.app.projects_host.published_key = None;
        settle(&mut self.app);
    }

    fn path(&self, name: &str) -> PathBuf {
        self.root.join("designs").join(name)
    }

    /// What the user does: ask on the row, confirm, and the frame executes it.
    fn trash_via_dock(&mut self, path: &Path) {
        let panel = &mut self.app.host.editor_state_mut().editor_ui.projects_panel;
        assert!(
            panel.begin_trash_confirm(&path.to_string_lossy()),
            "removable row"
        );
        assert!(panel.confirm_trash());
        self.app.pump_projects();
        settle(&mut self.app);
    }

    fn forge(&mut self, path: &Path) {
        self.app
            .host
            .editor_state_mut()
            .editor_ui
            .projects_panel
            .trash_commit = Some(path.to_string_lossy().into_owned());
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
        let _ = std::fs::remove_dir_all(self.bin);
        let _ = std::fs::remove_dir_all(self.store);
    }
}

#[test]
fn remove_moves_to_trash_and_never_deletes() {
    let mut world = World::new("move", &["keep.op", "doomed.op"]);
    let doomed = world.path("doomed.op");
    let bytes = std::fs::read(&doomed).unwrap();
    let fake = FakeTrash::new(&world.bin);
    world.use_backend(Arc::clone(&fake));
    assert!(world.labels().contains(&"doomed.op".to_string()));

    world.trash_via_dock(&doomed);

    assert_eq!(fake.calls(), [doomed.clone()], "the Trash was asked, once");
    assert!(!doomed.exists(), "gone from its folder");
    assert_eq!(
        std::fs::read(world.bin.join("doomed.op")).unwrap(),
        bytes,
        "and recoverable, byte for byte"
    );
    assert!(
        !world.labels().contains(&"doomed.op".to_string()),
        "row patched out"
    );
    assert!(world.labels().contains(&"keep.op".to_string()));
    assert_eq!(toast_key(&world.app), None);

    // A failing Trash leaves the file where it was: there is no fallback delete.
    let failing = FakeTrash::failing(&world.bin);
    world.use_backend(Arc::clone(&failing));
    let survivor = world.path("keep.op");
    assert!(world.app.close_document_tab(0));
    settle(&mut world.app);
    world.trash_via_dock(&survivor);
    assert_eq!(failing.calls(), [survivor.clone()]);
    assert!(survivor.is_file(), "a failed move never deletes");
    assert_eq!(
        toast_key(&world.app).as_deref(),
        Some("projects.trashFailed")
    );
    assert!(
        world.labels().contains(&"keep.op".to_string()),
        "the row stays"
    );
    world.cleanup();
}

#[test]
fn remove_refused_for_open_tab_inflight_and_outside_root() {
    let mut world = World::new("refuse", &["a.op", "b.op", "c.op"]);
    let (a, b, c) = (world.path("a.op"), world.path("b.op"), world.path("c.op"));
    let fake = FakeTrash::new(&world.bin);
    world.use_backend(Arc::clone(&fake));

    // Open in a tab: close the tab first.
    world.forge(&a);
    assert!(a.is_file());
    assert_eq!(
        toast_key(&world.app).as_deref(),
        Some("projects.trashOpenTab")
    );

    // A save is being written.
    world.app.host.editor_state_mut().editor_ui.dismiss_toast();
    edit_active(&mut world.app, "to-save");
    assert!(world.app.request_background_save());
    assert!(world.app.save_session.is_active());
    assert_eq!(
        world.app.trash_design_file(&b),
        Err(TrashRefusal::SaveInProgress)
    );
    world.app.finish_background_saves();

    // Outside every shown project: a forged path (a file next door).
    let elsewhere = temp_dir("refuse-out");
    let stranger = elsewhere.join("stranger.op");
    save_to_path(&op_editor_core::EditorState::starter(), &stranger).unwrap();
    assert_eq!(
        world.app.trash_design_file(&stranger),
        Err(TrashRefusal::OutsideProjects)
    );
    // Not an .op file, and a link.
    let notes = world.root.join("designs").join("notes.txt");
    std::fs::write(&notes, "x").unwrap();
    assert_eq!(
        world.app.trash_design_file(&notes),
        Err(TrashRefusal::NotADesignFile)
    );
    #[cfg(unix)]
    {
        let link = world.root.join("designs").join("link.op");
        std::os::unix::fs::symlink(&stranger, &link).unwrap();
        assert_eq!(
            world.app.trash_design_file(&link),
            Err(TrashRefusal::NotADesignFile)
        );
        assert!(stranger.is_file(), "the link's target is untouched");
    }
    assert_eq!(
        world.app.trash_design_file(&world.path("missing.op")),
        Err(TrashRefusal::SourceGone)
    );
    assert!(fake.calls().is_empty(), "no refusal ever reached the Trash");
    assert!(a.is_file() && b.is_file() && c.is_file() && stranger.is_file());
    let _ = std::fs::remove_dir_all(elsewhere);
    world.cleanup();
}

#[test]
fn a_platform_without_a_trash_offers_nothing_and_refuses_a_forged_request() {
    let mut world = World::new("unsupported", &["a.op", "b.op"]);
    let b = world.path("b.op");
    let fake = FakeTrash::unsupported(&world.bin);
    world.use_backend(Arc::clone(&fake));
    assert!(
        panel(&world.app)
            .iter()
            .flat_map(|project| &project.files)
            .all(|file| !file.removable),
        "the dock hides the action"
    );
    world.forge(&b);
    assert!(b.is_file(), "and nothing is hard-deleted instead");
    assert!(fake.calls().is_empty());
    world.cleanup();
}

#[test]
fn only_op_designs_are_marked_removable() {
    let mut world = World::new("marks", &["a.op", "b.op"]);
    let files: Vec<_> = panel(&world.app)
        .iter()
        .flat_map(|p| p.files.iter())
        .collect();
    assert!(files.iter().all(|file| file.removable), "{files:?}");
    world.cleanup();
}
