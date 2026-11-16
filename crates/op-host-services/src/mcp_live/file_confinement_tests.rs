//! Path confinement through the real `serve_connection` router: what an
//! agent's `save_document` / `open_document` / export call can touch on disk
//! once the live server is the one answering it.

use std::path::{Path, PathBuf};

use super::test_support::*;
use super::*;

/// `<scratch>/proj` (a git project) with `designs/a.op` (open as `d1`) and
/// `designs/b.op` (open as `d2`); `<scratch>/outside` belongs to no project.
struct Workspace {
    scratch: PathBuf,
}

impl Workspace {
    fn new(tag: &str) -> Self {
        let scratch = std::env::temp_dir().join(format!(
            "openpencil-file-confinement-{tag}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&scratch);
        std::fs::create_dir_all(scratch.join("proj/.git")).expect("git marker");
        std::fs::create_dir_all(scratch.join("proj/designs")).expect("designs");
        std::fs::create_dir_all(scratch.join("outside")).expect("outside");
        std::fs::write(scratch.join("proj/designs/a.op"), "{}").expect("a.op");
        std::fs::write(scratch.join("proj/designs/b.op"), "{}").expect("b.op");
        Self { scratch }
    }

    fn proj(&self, relative: &str) -> PathBuf {
        self.scratch.join("proj").join(relative)
    }

    fn outside(&self, relative: &str) -> PathBuf {
        self.scratch.join("outside").join(relative)
    }

    /// Two tabs, `a.op` on screen and `b.op` parked.
    fn resolver(&self) -> TestResolver {
        let mut resolver = TestResolver::with_ids(&[(1, true), (2, false)]);
        resolver.document_mut(1).path = Some(self.proj("designs/a.op"));
        resolver.document_mut(2).path = Some(self.proj("designs/b.op"));
        resolver
    }

    /// One tool call addressed to `d1`.
    fn call(&self, tool: &str, arguments: &str) -> String {
        let body = tool_call(1, tool, &format!("{},{arguments}", document_arg(1)));
        post(
            "/mcp",
            "",
            &body,
            &LiveDocuments::default(),
            self.resolver(),
            None,
        )
        .response
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.scratch);
    }
}

fn quoted(path: &Path) -> String {
    serde_json::to_string(&path.to_string_lossy()).expect("path json")
}

fn save_call(workspace: &Workspace, target: &Path) -> String {
    workspace.call(
        "save_document",
        &format!(r#""filePath":{}"#, quoted(target)),
    )
}

#[test]
fn save_document_outside_the_allowed_roots_is_refused_and_nothing_is_written() {
    let workspace = Workspace::new("outside");
    let target = workspace.outside("stolen.op");

    let response = save_call(&workspace, &target);

    assert!(is_error(&response), "{response}");
    let text = error_text(&response);
    assert!(text.contains("outside"), "{text}");
    let root = std::fs::canonicalize(workspace.proj("")).expect("root");
    assert!(
        text.contains(&root.display().to_string()),
        "names the root: {text}"
    );
    assert!(!target.exists(), "nothing was written");
    let leftovers: Vec<_> = std::fs::read_dir(workspace.outside(""))
        .expect("list")
        .collect();
    assert!(leftovers.is_empty(), "no temp file either: {leftovers:?}");
}

#[test]
fn save_document_inside_a_project_root_is_atomic() {
    let workspace = Workspace::new("atomic");
    let target = workspace.proj("designs/copy.op");
    std::fs::write(&target, "old contents").expect("seed");
    #[cfg(unix)]
    let before = std::os::unix::fs::MetadataExt::ino(&std::fs::metadata(&target).expect("meta"));

    let response = save_call(&workspace, &target);

    assert!(!is_error(&response), "{response}");
    let saved: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&target).expect("saved")).expect("json");
    assert!(saved.is_object());
    // Written by renaming a finished temp file over the target, never by
    // truncating the target in place: the file is a new inode and no temp
    // sibling is left.
    #[cfg(unix)]
    assert_ne!(
        std::os::unix::fs::MetadataExt::ino(&std::fs::metadata(&target).expect("meta")),
        before,
        "the target was replaced by rename, not rewritten in place"
    );
    let names: Vec<String> = std::fs::read_dir(workspace.proj("designs"))
        .expect("list")
        .map(|entry| {
            entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    assert!(
        names.iter().all(|name| !name.ends_with(".tmp")),
        "{names:?}"
    );
}

#[test]
fn save_document_onto_a_file_open_in_another_tab_is_refused() {
    let workspace = Workspace::new("other-tab");
    let other_tab = workspace.proj("designs/b.op");

    // The call addresses d1 (a.op); b.op is open as d2.
    let response = save_call(&workspace, &other_tab);

    assert!(is_error(&response), "{response}");
    assert!(
        error_text(&response).contains("open in an editor tab"),
        "{response}"
    );
    assert_eq!(std::fs::read_to_string(&other_tab).expect("b.op"), "{}");
    // The document's own file is a tab's file too.
    let own = save_call(&workspace, &workspace.proj("designs/a.op"));
    assert!(is_error(&own), "{own}");
    assert_eq!(
        std::fs::read_to_string(workspace.proj("designs/a.op")).expect("a.op"),
        "{}"
    );
}

#[test]
fn file_path_dotdot_and_symlink_escapes_are_refused() {
    let workspace = Workspace::new("escapes");
    let stolen = workspace.outside("stolen.op");
    let dotdot = workspace.proj("designs/../../outside/stolen.op");

    let by_dotdot = save_call(&workspace, &dotdot);
    assert!(is_error(&by_dotdot), "{by_dotdot}");
    assert!(error_text(&by_dotdot).contains("outside"), "{by_dotdot}");
    assert!(!stolen.exists());

    // `filePath` on any other tool is confined the same way.
    let open_dotdot = workspace.call(
        "open_document",
        &format!(r#""filePath":{}"#, quoted(&dotdot)),
    );
    assert!(is_error(&open_dotdot), "{open_dotdot}");
    assert!(!stolen.exists());

    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(workspace.outside(""), workspace.proj("designs/escape"))
            .expect("link out");
        let through_link = workspace.proj("designs/escape/stolen.op");
        let by_symlink = save_call(&workspace, &through_link);
        assert!(is_error(&by_symlink), "{by_symlink}");
        assert!(!stolen.exists(), "the write did not follow the link out");
    }
}

#[test]
fn create_dir_all_never_happens_outside_a_root() {
    let workspace = Workspace::new("create-dir");
    let outside_new = workspace.outside("fresh/deeper/new.op");

    // `open_document` creates a missing file (and its folders) ...
    let refused = workspace.call(
        "open_document",
        &format!(r#""filePath":{}"#, quoted(&outside_new)),
    );
    assert!(is_error(&refused), "{refused}");
    assert!(
        !workspace.outside("fresh").exists(),
        "no folder was created outside"
    );

    // ... and so does a frame export into a missing folder.
    let export = workspace.call(
        "export_frames",
        &format!(
            r#""outputDir":{}"#,
            quoted(&workspace.outside("frames/out"))
        ),
    );
    assert!(is_error(&export), "{export}");
    assert!(!workspace.outside("frames").exists());

    // Inside the project the same call is served and may create its folders.
    let inside = workspace.proj("designs/fresh/deeper/new.op");
    let served = workspace.call(
        "open_document",
        &format!(r#""filePath":{}"#, quoted(&inside)),
    );
    assert!(!is_error(&served), "{served}");
    assert!(inside.exists());
}

#[test]
fn headless_modes_keep_unrestricted_paths() {
    // The headless `--mcp` / `--mcp-http` servers never go through
    // `mcp_live`, so they are the user's own explicit command line and may
    // name any path: no project root, no confinement.
    let workspace = Workspace::new("headless");
    let stray = workspace.outside("anywhere/new.op");
    let line = tool_call(
        1,
        "open_document",
        &format!(r#""filePath":{}"#, quoted(&stray)),
    );

    let response = crate::mcp_serve::file_path::process_message_for_file_path_arg(
        None,
        &[],
        &line,
        crate::mcp_serve::tool_profile::McpAccessProfile::UNRESTRICTED,
    )
    .expect("served")
    .expect("a response");
    assert!(!response.contains(r#""isError":true"#), "{response}");
    assert!(stray.exists(), "headless open_document created the file");

    let target = workspace.outside("saved.op");
    let mut state = EditorState::new();
    let line = tool_call(
        2,
        "save_document",
        &format!(r#""filePath":{}"#, quoted(&target)),
    );
    let response = crate::mcp_serve::process_message_with_applier_profiled(
        &mut state,
        &line,
        crate::mcp_serve::tool_profile::McpAccessProfile::UNRESTRICTED,
        |_, _, _| true,
    )
    .expect("served")
    .expect("a response");
    assert!(!response.contains(r#""isError":true"#), "{response}");
    assert!(
        target.exists(),
        "headless save_document wrote outside any project"
    );
}

#[test]
fn a_canvas_lookalike_creates_nothing_under_the_editors_working_directory() {
    // `live://canvasX/...` is a relative path to the tools: they would create
    // `<cwd>/live:/canvasX/...` and a later `..` could climb out of it.
    let workspace = Workspace::new("lookalike");
    let relative_root = Path::new("live:");
    assert!(
        !relative_root.exists(),
        "precondition: nothing named live: here"
    );

    let opened = workspace.call("open_document", r#""filePath":"live://canvasX/a/b.op""#);
    assert!(is_error(&opened), "{opened}");
    let escaped = save_call(&workspace, Path::new("live://canvasX/../../../escape.op"));
    assert!(is_error(&escaped), "{escaped}");
    let source = workspace.call(
        "save_document",
        &format!(
            r#""filePath":{},"sourceFilePath":"live://canvasX/x.op""#,
            quoted(&workspace.proj("designs/out.op"))
        ),
    );
    assert!(is_error(&source), "{source}");

    assert!(
        !relative_root.exists(),
        "no folder was created under the cwd"
    );
    assert!(!workspace.proj("designs/out.op").exists());
}
