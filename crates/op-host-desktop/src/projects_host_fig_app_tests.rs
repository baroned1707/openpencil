//! `.fig` rows in the dock, end to end on a headless app: what the scan lists,
//! what a click and "Convert all" do, which tabs open, and how a conversion's
//! progress reaches the published tree. The converter is a fake that writes a
//! real document, so a converted file can also be opened.

use super::fig::{ConversionRun, ConvertOutcome, FigConverter, FigLimits};
use super::tests::{panel, settle};
use super::*;
use op_editor_core::{FigStatus, ProjectFileKind};
use op_host_services::doc_io::save_to_path;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Mutex;

static SEQ: AtomicU32 = AtomicU32::new(0);

fn project_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "openpencil-fig-app-{tag}-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

fn write_fig(root: &Path, relative: &str) -> PathBuf {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, b"fig").unwrap();
    path
}

fn write_op(root: &Path, relative: &str) -> PathBuf {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    save_to_path(&op_editor_core::EditorState::starter(), &path).unwrap();
    path
}

/// Writes a real document when released; remembers every `(input, output)`.
struct Writer {
    started: Arc<Mutex<Vec<(PathBuf, PathBuf)>>>,
    release: Arc<AtomicBool>,
}

struct WriterRun {
    output: PathBuf,
    release: Arc<AtomicBool>,
}

impl FigConverter for Writer {
    fn start(&self, input: &Path, output: &Path) -> std::io::Result<Box<dyn ConversionRun>> {
        self.started
            .lock()
            .unwrap()
            .push((input.to_path_buf(), output.to_path_buf()));
        Ok(Box::new(WriterRun {
            output: output.to_path_buf(),
            release: Arc::clone(&self.release),
        }))
    }
}

impl ConversionRun for WriterRun {
    fn poll(&mut self) -> Option<ConvertOutcome> {
        if !self.release.load(Ordering::SeqCst) {
            return None;
        }
        save_to_path(&op_editor_core::EditorState::starter(), &self.output).unwrap();
        Some(ConvertOutcome::Done)
    }

    fn kill(&mut self) {}
}

struct Harness {
    app: DesktopApp,
    root: PathBuf,
    started: Arc<Mutex<Vec<(PathBuf, PathBuf)>>>,
    release: Arc<AtomicBool>,
}

impl Harness {
    /// An app with `root` opened and a fake converter that waits for `release`.
    fn new(root: &Path, released: bool) -> Self {
        let started = Arc::new(Mutex::new(Vec::new()));
        let release = Arc::new(AtomicBool::new(released));
        let mut app = DesktopApp::new(None);
        app.projects_host.set_fig_converter(
            Arc::new(Writer {
                started: Arc::clone(&started),
                release: Arc::clone(&release),
            }),
            FigLimits::default(),
        );
        assert!(app.open_project(root));
        settle(&mut app);
        Self {
            app,
            root: root.to_path_buf(),
            started,
            release,
        }
    }

    fn request(&mut self, request: ProjectRequest) {
        self.app
            .host
            .editor_state_mut()
            .editor_ui
            .projects_panel
            .pending_request = Some(request);
        self.app.pump_projects();
    }

    /// Pump frames until conversions and the rescan after them are over.
    fn run_until_quiet(&mut self) {
        let started = Instant::now();
        loop {
            self.app.pump_projects();
            if !self.app.projects_host.figs_pending() && self.app.projects_host.scan.is_idle() {
                self.app.pump_projects();
                return;
            }
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "never went quiet"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    fn status_of(&self, label: &str) -> Option<FigStatus> {
        panel(&self.app)[0]
            .files
            .iter()
            .find(|file| file.label == label)
            .and_then(|file| match &file.kind {
                ProjectFileKind::Figma(status) => Some(status.clone()),
                ProjectFileKind::Design => None,
            })
    }

    fn labels(&self) -> Vec<String> {
        panel(&self.app)[0]
            .files
            .iter()
            .map(|file| file.label.clone())
            .collect()
    }

    /// Whether a tab shows the document at `path`.
    fn is_open(&mut self, path: &Path) -> bool {
        self.app.sync_active_slot();
        self.app.documents.index_of_path(path).is_some()
    }

    fn tab_count(&mut self) -> usize {
        self.app.sync_active_slot();
        self.app.documents.tab_views(false, "").len()
    }

    fn started(&self) -> Vec<(PathBuf, PathBuf)> {
        self.started.lock().unwrap().clone()
    }

    fn fig_path(&self, relative: &str) -> String {
        self.root.join(relative).to_string_lossy().into_owned()
    }
}

#[test]
fn the_dock_lists_fig_files_with_their_conversion_status() {
    let root = project_dir("status");
    write_fig(&root, "new.fig");
    write_fig(&root, "done.fig");
    write_op(&root, "done.op");
    write_op(&root, "plain.op");
    let harness = Harness::new(&root, true);
    assert_eq!(
        harness.labels(),
        ["done.fig", "done.op", "new.fig", "plain.op"],
        "figs sit among the .op files of their folder"
    );
    assert_eq!(harness.status_of("new.fig"), Some(FigStatus::NotConverted));
    assert_eq!(harness.status_of("done.fig"), Some(FigStatus::Converted));
    assert_eq!(harness.status_of("plain.op"), None, "an .op is a design");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn status_flows_into_the_dock_snapshot() {
    let root = project_dir("flow");
    let fig = write_fig(&root, "a.fig");
    let mut harness = Harness::new(&root, false);
    assert_eq!(harness.status_of("a.fig"), Some(FigStatus::NotConverted));

    harness.request(ProjectRequest::ConvertFig(
        fig.to_string_lossy().into_owned(),
    ));
    harness.app.pump_projects();
    assert_eq!(
        harness.status_of("a.fig"),
        Some(FigStatus::Converting),
        "the running conversion is in the published rows"
    );
    assert_eq!(harness.started().len(), 1);

    harness.release.store(true, Ordering::SeqCst);
    harness.run_until_quiet();
    assert_eq!(harness.status_of("a.fig"), Some(FigStatus::Converted));
    assert!(
        harness.labels().contains(&"a.op".to_string()),
        "the .op is listed"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_failed_conversion_shows_its_reason_and_a_click_retries() {
    let root = project_dir("retry");
    let fig = write_fig(&root, "a.fig");
    let mut harness = Harness::new(&root, true);
    // A converter that cannot start: the failure reaches the row.
    harness.app.projects_host.set_fig_converter(
        Arc::new(super::fig_process::default_converter_for_tests()),
        FigLimits::default(),
    );
    let click = ProjectRequest::ConvertFig(fig.to_string_lossy().into_owned());
    harness.request(click.clone());
    harness.run_until_quiet();
    let Some(FigStatus::Failed(reason)) = harness.status_of("a.fig") else {
        panic!("expected a failure, got {:?}", harness.status_of("a.fig"));
    };
    assert!(reason.starts_with("cannot start"), "{reason}");
    assert!(!root.join("a.op").exists());

    // Retry with a working converter.
    harness.app.projects_host.set_fig_converter(
        Arc::new(Writer {
            started: Arc::clone(&harness.started),
            release: Arc::clone(&harness.release),
        }),
        FigLimits::default(),
    );
    harness.request(click);
    harness.run_until_quiet();
    assert_eq!(harness.status_of("a.fig"), Some(FigStatus::Converted));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn clicking_one_fig_converts_it_and_opens_only_that_result() {
    let root = project_dir("single");
    let fig = write_fig(&root, "a.fig");
    write_fig(&root, "b.fig");
    let mut harness = Harness::new(&root, true);
    harness.request(ProjectRequest::ConvertFig(
        fig.to_string_lossy().into_owned(),
    ));
    harness.run_until_quiet();
    assert!(
        harness.is_open(&root.join("a.op")),
        "the clicked file's result opens"
    );
    assert_eq!(harness.started().len(), 1, "b.fig is untouched");
    assert!(root.join("a.op").is_file());
    assert!(!root.join("b.op").exists());
    assert!(!harness.is_open(&root.join("b.fig")));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn clicking_a_converted_fig_opens_the_op_beside_it_without_converting() {
    let root = project_dir("open-sibling");
    let fig = write_fig(&root, "a.fig");
    write_op(&root, "a.op");
    let mut harness = Harness::new(&root, true);
    assert!(!harness.is_open(&root.join("a.op")));
    harness.request(ProjectRequest::ConvertFig(
        fig.to_string_lossy().into_owned(),
    ));
    harness.run_until_quiet();
    assert!(harness.is_open(&root.join("a.op")));
    assert!(harness.started().is_empty(), "nothing was converted");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn batch_convert_never_opens_tabs() {
    let root = project_dir("batch");
    write_fig(&root, "a.fig");
    write_fig(&root, "screens/b.fig");
    write_fig(&root, "screens/auth/c.fig");
    write_fig(&root, "other/d.fig");
    write_op(&root, "screens/existing.op");
    let mut harness = Harness::new(&root, true);
    let before = harness.tab_count();

    // Scoped to a folder and below: b and c, not a or d.
    harness.request(ProjectRequest::ConvertAllFigs(
        root.to_string_lossy().into_owned(),
        Some("screens".to_string()),
    ));
    harness.run_until_quiet();
    let converted: Vec<String> = harness
        .started()
        .iter()
        .map(|(input, _)| input.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(converted.len(), 2);
    assert!(converted.contains(&"b.fig".to_string()) && converted.contains(&"c.fig".to_string()));
    assert_eq!(harness.tab_count(), before, "a batch opens nothing");
    assert!(!harness.is_open(&root.join("screens/b.op")));
    assert!(!harness.is_open(&root.join("screens/auth/c.op")));

    // Then the whole project: only what is still waiting.
    harness.request(ProjectRequest::ConvertAllFigs(
        root.to_string_lossy().into_owned(),
        None,
    ));
    harness.run_until_quiet();
    assert_eq!(harness.started().len(), 4, "a and d, once each");
    assert_eq!(harness.tab_count(), before, "still no tab");
    assert!(!harness.is_open(&root.join("a.op")) && !harness.is_open(&root.join("other/d.op")));
    for stem in ["a", "screens/b", "screens/auth/c", "other/d"] {
        assert!(root.join(format!("{stem}.op")).is_file(), "{stem}.op");
    }
    assert!(harness.labels().contains(&"a.op".to_string()));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_request_for_a_file_the_dock_never_listed_converts_nothing() {
    let root = project_dir("unlisted");
    write_fig(&root, "a.fig");
    let outside = project_dir("unlisted-outside");
    let stranger = write_fig(&outside, "x.fig");
    let mut harness = Harness::new(&root, true);
    for path in [
        stranger.to_string_lossy().into_owned(),
        harness.fig_path("never-scanned.fig"),
    ] {
        write_fig(&root, "never-scanned.fig");
        harness.request(ProjectRequest::ConvertFig(path));
        harness.app.pump_projects();
    }
    harness.run_until_quiet();
    assert!(harness.started().is_empty());
    assert!(!outside.join("x.op").exists());
    let _ = std::fs::remove_dir_all(root);
    let _ = std::fs::remove_dir_all(outside);
}

#[test]
fn closing_the_project_drops_its_conversions() {
    let root = project_dir("close");
    write_fig(&root, "a.fig");
    let mut harness = Harness::new(&root, false);
    harness.request(ProjectRequest::ConvertAllFigs(
        root.to_string_lossy().into_owned(),
        None,
    ));
    harness.app.pump_projects();
    assert!(harness.app.projects_host.figs_pending());
    let root_text = root.to_string_lossy().into_owned();
    harness.request(ProjectRequest::CloseProject(root_text));
    assert!(!harness.app.projects_host.figs_pending());
    let _ = std::fs::remove_dir_all(root);
}
