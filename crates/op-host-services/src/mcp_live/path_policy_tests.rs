//! The file-confinement rules, on real folders: what a path-bearing argument
//! may name once the open documents' project roots are the only allowed ones.

use std::collections::BTreeMap;

use op_editor_core::{DocumentId, DocumentTabStatus};
use op_mcp::{RequestId, ToolCall};

use super::*;

/// `<scratch>/proj` is a project (it has `.git`) holding `designs/a.op` and
/// `designs/b.op`; `<scratch>/outside` is not part of any project.
struct Tree {
    scratch: PathBuf,
}

impl Tree {
    fn new(tag: &str) -> Self {
        let scratch = std::env::temp_dir().join(format!(
            "openpencil-path-policy-{tag}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&scratch);
        std::fs::create_dir_all(scratch.join("proj/.git")).expect("git marker");
        std::fs::create_dir_all(scratch.join("proj/designs")).expect("designs");
        std::fs::create_dir_all(scratch.join("outside")).expect("outside");
        std::fs::write(scratch.join("proj/designs/a.op"), "{}").expect("a.op");
        std::fs::write(scratch.join("proj/designs/b.op"), "{}").expect("b.op");
        std::fs::write(scratch.join("outside/secret.txt"), "secret").expect("secret");
        Self { scratch }
    }

    fn proj(&self, relative: &str) -> PathBuf {
        self.scratch.join("proj").join(relative)
    }

    fn outside(&self, relative: &str) -> PathBuf {
        self.scratch.join("outside").join(relative)
    }

    /// `a.op` is open in a tab (and `b.op` in another).
    fn open(&self) -> Vec<DocumentListing> {
        vec![
            listing(1, Some(self.proj("designs/a.op"))),
            listing(2, Some(self.proj("designs/b.op"))),
        ]
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.scratch);
    }
}

fn listing(id: u64, path: Option<PathBuf>) -> DocumentListing {
    DocumentListing {
        id: DocumentId(id),
        label: "doc.op".to_string(),
        path,
        project: String::new(),
        dirty: false,
        active: id == 1,
        status: DocumentTabStatus::Idle,
        nonce: None,
    }
}

fn call(tool: &str, arguments: &[(&str, &Path)]) -> ToolCall {
    ToolCall {
        id: RequestId::Num(1),
        tool: tool.to_string(),
        arguments: arguments
            .iter()
            .map(|(key, value)| (key.to_string(), value.display().to_string()))
            .collect::<BTreeMap<_, _>>(),
    }
}

fn refusal(call: &ToolCall, open: &[DocumentListing]) -> Option<PathRefusal> {
    match vet_call(call, open, &[]) {
        Ok(()) => None,
        Err(McpLiveError::PathNotAllowed { reason, .. }) => Some(reason),
        Err(other) => panic!("unexpected error {other:?}"),
    }
}

#[test]
fn the_allowed_roots_are_the_project_roots_of_the_open_documents() {
    let tree = Tree::new("roots");
    let roots = allowed_roots(&tree.open(), &[]);
    assert_eq!(roots, vec![std::fs::canonicalize(tree.proj("")).unwrap()]);

    // An untitled document contributes nothing; the user's extra folder does.
    let extra = allowed_roots(&[listing(1, None)], &[tree.outside("")]);
    assert_eq!(
        extra,
        vec![std::fs::canonicalize(tree.outside("")).unwrap()]
    );
    // A folder that does not exist is no root.
    assert!(allowed_roots(&[], &[tree.scratch.join("missing")]).is_empty());
}

#[test]
fn a_path_inside_the_project_is_allowed_new_files_and_folders_included() {
    let tree = Tree::new("inside");
    let open = tree.open();
    for target in [
        "designs/new.op",
        "designs/deeper/still/new.op",
        "export/out.pptx",
    ] {
        let save = call("save_document", &[("filePath", &tree.proj(target))]);
        assert_eq!(refusal(&save, &open), None, "{target}");
    }
    let frames = call(
        "export_frames",
        &[("outputDir", &tree.proj("export/frames"))],
    );
    assert_eq!(refusal(&frames, &open), None);
    let import = call("import_svg", &[("svgPath", &tree.proj("assets/logo.svg"))]);
    assert_eq!(refusal(&import, &open), None);
}

#[test]
fn every_path_bearing_argument_is_confined_whatever_the_tool() {
    let tree = Tree::new("keys");
    let open = tree.open();
    let elsewhere = tree.outside("secret.txt");
    let folder = tree.outside("");
    for (tool, key) in [
        ("save_document", "filePath"),
        ("save_document", "sourceFilePath"),
        ("open_document", "filePath"),
        ("batch_design", "filePath"),
        ("get_selection", "filePath"),
        ("import_svg", "svgPath"),
        ("import_svg", "svg_path"),
        ("import_html", "htmlPath"),
        ("import_html", "html_path"),
        ("import_web_snapshot", "snapshotPath"),
        ("import_web_snapshot", "snapshot_path"),
        ("brand_extract", "imagePath"),
        ("save_theme_preset", "presetPath"),
        ("load_theme_preset", "presetPath"),
        ("export_deck", "outputPath"),
    ] {
        assert_eq!(
            refusal(&call(tool, &[(key, &elsewhere)]), &open),
            Some(PathRefusal::Outside),
            "{tool} {key}"
        );
    }
    for (tool, key) in [
        ("export_frames", "outputDir"),
        ("list_theme_presets", "directory"),
    ] {
        assert_eq!(
            refusal(&call(tool, &[(key, &folder)]), &open),
            Some(PathRefusal::Outside),
            "{tool} {key}"
        );
    }
}

#[test]
fn the_live_canvas_placeholder_and_metadata_arguments_are_not_paths() {
    let tree = Tree::new("meta");
    let open = tree.open();
    for placeholder in ["live://canvas", "live://canvas/current"] {
        let sentinel = call("batch_design", &[("filePath", Path::new(placeholder))]);
        assert_eq!(refusal(&sentinel, &open), None, "{placeholder}");
    }
    // `sourcePath` is ledger metadata that no tool ever opens.
    let ledger = call(
        "upsert_variables",
        &[("sourcePath", &tree.outside("secret.txt"))],
    );
    assert_eq!(refusal(&ledger, &open), None);
}

#[test]
fn relative_and_dotdot_paths_are_refused() {
    let tree = Tree::new("shape");
    let open = tree.open();
    let relative = call("save_document", &[("filePath", Path::new("designs/x.op"))]);
    assert_eq!(refusal(&relative, &open), Some(PathRefusal::NotAbsolute));
    // A `..` that climbs out of the root is judged by where it lands.
    let escape = tree.proj("designs/../../outside/x.op");
    let dotdot = call("save_document", &[("filePath", &escape)]);
    assert_eq!(refusal(&dotdot, &open), Some(PathRefusal::Outside));
    // One that stays inside the root is just another spelling of a path in it.
    let inside = tree.proj("designs/../designs/x.op");
    let still = call("save_document", &[("filePath", &inside)]);
    assert_eq!(refusal(&still, &open), None);
    // A `..` below the part that exists has nothing to resolve against.
    let unresolvable = tree.proj("designs/missing/../x.op");
    let beyond = call("save_document", &[("filePath", &unresolvable)]);
    assert_eq!(refusal(&beyond, &open), Some(PathRefusal::ParentSegment));
}

#[cfg(unix)]
#[test]
fn symlinks_that_leave_the_root_are_refused() {
    use std::os::unix::fs::symlink;

    let tree = Tree::new("symlink");
    let open = tree.open();
    // A folder inside the project that points outside it.
    symlink(tree.outside(""), tree.proj("designs/escape")).expect("dir link");
    // A file inside the project that points at an outside file.
    symlink(tree.outside("secret.txt"), tree.proj("designs/link.op")).expect("file link");
    // A link to nowhere: `exists()` says false, a write would follow it out.
    symlink(
        tree.outside("not-there.op"),
        tree.proj("designs/dangling.op"),
    )
    .expect("dangling");

    for target in [
        "designs/escape/new.op",
        "designs/escape/secret.txt",
        "designs/link.op",
    ] {
        let save = call("save_document", &[("filePath", &tree.proj(target))]);
        assert_eq!(
            refusal(&save, &open),
            Some(PathRefusal::Outside),
            "{target}"
        );
    }
    let dangling = call(
        "save_document",
        &[("filePath", &tree.proj("designs/dangling.op"))],
    );
    assert_eq!(
        refusal(&dangling, &open),
        Some(PathRefusal::DanglingSymlink)
    );
    let read = call("import_svg", &[("svgPath", &tree.proj("designs/link.op"))]);
    assert_eq!(refusal(&read, &open), Some(PathRefusal::Outside));
    let frames = call(
        "export_frames",
        &[("outputDir", &tree.proj("designs/escape"))],
    );
    assert_eq!(refusal(&frames, &open), Some(PathRefusal::Outside));
}

#[test]
fn writes_never_target_hidden_folders_directories_or_files_open_in_any_tab() {
    let tree = Tree::new("writes");
    let open = tree.open();
    let hidden = call(
        "save_document",
        &[("filePath", &tree.proj(".git/hooks/pre-commit"))],
    );
    assert_eq!(refusal(&hidden, &open), Some(PathRefusal::HiddenFolder));
    let hidden_dir = call("export_frames", &[("outputDir", &tree.proj(".claude/out"))]);
    assert_eq!(refusal(&hidden_dir, &open), Some(PathRefusal::HiddenFolder));

    let into_dir = call("save_document", &[("filePath", &tree.proj("designs"))]);
    assert_eq!(refusal(&into_dir, &open), Some(PathRefusal::IsDirectory));
    let file_as_dir = call(
        "export_frames",
        &[("outputDir", &tree.proj("designs/a.op"))],
    );
    assert_eq!(
        refusal(&file_as_dir, &open),
        Some(PathRefusal::NotDirectory)
    );

    // `b.op` is open in ANOTHER tab than the one the call addresses.
    let onto_b = call("save_document", &[("filePath", &tree.proj("designs/b.op"))]);
    assert_eq!(refusal(&onto_b, &open), Some(PathRefusal::OpenInEditor));
    // Reading an open file is not a write.
    let read_b = call("import_html", &[("htmlPath", &tree.proj("designs/b.op"))]);
    assert_eq!(refusal(&read_b, &open), None);
    // Once the tab is closed the file is an ordinary target.
    assert_eq!(refusal(&onto_b, &open[..1]), None);
}

#[test]
fn with_only_unsaved_documents_no_folder_is_open_and_the_refusal_says_so() {
    let tree = Tree::new("noroots");
    let save = call("save_document", &[("filePath", &tree.proj("designs/x.op"))]);
    let error = vet_call(&save, &[listing(1, None)], &[]).expect_err("refused");
    let text = error.to_string();
    assert!(text.contains("unsaved"), "{text}");
    assert!(text.contains("create_document"), "{text}");
}

#[test]
fn a_refusal_names_the_roots_the_agent_may_use() {
    let tree = Tree::new("message");
    let save = call("save_document", &[("filePath", &tree.outside("x.op"))]);
    let error = vet_call(&save, &tree.open(), &[]).expect_err("refused");
    let text = error.to_string();
    let root = std::fs::canonicalize(tree.proj("")).unwrap();
    assert!(text.contains(&root.display().to_string()), "{text}");
    assert!(text.contains("save_document"), "{text}");
    assert!(text.contains("outside"), "{text}");
}

#[test]
fn an_extra_folder_from_settings_widens_the_roots() {
    let tree = Tree::new("extra");
    let save = call("save_document", &[("filePath", &tree.outside("x.op"))]);
    assert!(vet_call(&save, &tree.open(), &[]).is_err());
    assert!(vet_call(&save, &tree.open(), &[tree.outside("")]).is_ok());
}

#[test]
fn only_the_exact_canvas_placeholder_is_exempt_and_only_as_an_edit_target() {
    let tree = Tree::new("placeholder");
    let open = tree.open();
    // Near misses are relative paths to the tools (resolved against the
    // editor's cwd), so they are refused like any relative path.
    for lookalike in [
        "live://canvasX/x.op",
        "live://canvas/other",
        "live://canvas/../../escape.op",
        "live://canvas/current/x.op",
    ] {
        for (tool, key) in [
            ("open_document", "filePath"),
            ("save_document", "filePath"),
            ("save_document", "sourceFilePath"),
            ("export_deck", "outputPath"),
        ] {
            let call = call(tool, &[(key, Path::new(lookalike))]);
            assert_eq!(
                refusal(&call, &open),
                Some(PathRefusal::NotAbsolute),
                "{tool} {key} {lookalike}"
            );
        }
    }
    // The placeholder means nothing as a place to write or a file to read.
    for (tool, key) in [
        ("save_document", "filePath"),
        ("save_document", "sourceFilePath"),
    ] {
        let call = call(tool, &[(key, Path::new("live://canvas"))]);
        assert_eq!(
            refusal(&call, &open),
            Some(PathRefusal::NotAbsolute),
            "{tool} {key}"
        );
    }
}

#[test]
fn the_path_is_vetted_as_written_not_as_trimmed() {
    let tree = Tree::new("untrimmed");
    let open = tree.open();
    // `SaveDocument` resolves the untrimmed string, so a leading space makes it
    // a RELATIVE path under the editor's cwd, whatever it looks like trimmed.
    let padded = format!(" {}", tree.proj("designs/x.op").display());
    let save = call("save_document", &[("filePath", Path::new(&padded))]);
    assert_eq!(refusal(&save, &open), Some(PathRefusal::NotAbsolute));
    let blank = call("save_document", &[("filePath", Path::new("   "))]);
    assert_eq!(refusal(&blank, &open), None, "a blank value names nothing");
}

#[test]
fn mcp_path_policy_allows_files_inside_an_opened_project_but_not_over_open_tabs() {
    let tree = Tree::new("opened-project");
    // The user opened `outside` as a project: it arrives as an extra root,
    // although no open document lives in it.
    let opened = [tree.outside("")];
    let new_file = call(
        "save_document",
        &[("filePath", &tree.outside("screens/new.op"))],
    );
    assert!(
        vet_call(&new_file, &tree.open(), &[]).is_err(),
        "closed: refused"
    );
    assert!(
        vet_call(&new_file, &tree.open(), &opened).is_ok(),
        "opened: allowed"
    );

    // A `.op` inside it that is open in a tab is still not a write target.
    std::fs::write(tree.outside("held.op"), "{}").unwrap();
    let mut open = tree.open();
    open.push(listing(3, Some(tree.outside("held.op"))));
    let onto_held = call("save_document", &[("filePath", &tree.outside("held.op"))]);
    match vet_call(&onto_held, &open, &opened) {
        Err(McpLiveError::PathNotAllowed { reason, .. }) => {
            assert_eq!(reason, PathRefusal::OpenInEditor)
        }
        other => panic!("expected an OpenInEditor refusal, got {other:?}"),
    }
    // Hidden folders stay off limits inside an opened project too.
    let hidden = call(
        "save_document",
        &[("filePath", &tree.outside(".secret/x.op"))],
    );
    assert!(vet_call(&hidden, &tree.open(), &opened).is_err());
}
