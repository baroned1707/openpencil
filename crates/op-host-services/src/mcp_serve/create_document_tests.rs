//! `create_document` argument validation, safe path resolution and the
//! never-overwrite write. Every case builds its own directory under the OS
//! temp dir and injects its own "home", so the machine's real home is never
//! read or written.

use super::*;
use std::sync::atomic::{AtomicU32, Ordering};

static TREE_SEQ: AtomicU32 = AtomicU32::new(0);

/// A throwaway directory (canonical, since macOS temp sits behind a symlink)
/// removed on drop.
struct Tree(PathBuf);

impl Tree {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!(
            "openpencil-create-document-{}-{}",
            std::process::id(),
            TREE_SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Self(std::fs::canonicalize(dir).unwrap())
    }

    /// A project folder inside the tree.
    fn project(&self) -> PathBuf {
        let dir = self.0.join("project");
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// The injected home: a sibling of the project, so no project is home.
    fn home(&self) -> PathBuf {
        let dir = self.0.join("home");
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn resolve(root: &Path, name: &str, tree: &Tree) -> Result<CreateTarget, CreateDocumentError> {
    resolve_target(root.to_str().unwrap(), name, Some(&tree.home()))
}

fn untitled_state() -> EditorState {
    EditorState::starter()
}

#[test]
fn create_document_writes_under_designs_and_binds_the_editor_file() {
    let tree = Tree::new();
    let project = tree.project();
    let state = untitled_state();

    let target = resolve(&project, "landing", &tree).expect("resolves");
    write_new_document(&state, &target).expect("writes");

    assert_eq!(target.file_path, project.join("designs").join("landing.op"));
    assert_eq!(target.project_root, project);
    assert_eq!(target.name, "landing");
    let reloaded =
        crate::doc_io::load_editor_state(&target.file_path, op_editor_core::Locale::EnUs)
            .expect("the written file loads back");
    assert_eq!(reloaded.doc, state.doc, "the live document was written");
}

#[test]
fn create_document_creates_the_designs_directory() {
    let tree = Tree::new();
    let project = tree.project();
    assert!(!project.join("designs").exists());

    let target = resolve(&project, "first", &tree).expect("resolves");
    assert!(
        !project.join("designs").exists(),
        "resolving only checks; it never creates"
    );
    write_new_document(&untitled_state(), &target).expect("writes");

    assert!(project.join("designs").is_dir());
    assert!(project.join("designs").join("first.op").is_file());
    let leftovers: Vec<_> = std::fs::read_dir(project.join("designs"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(leftovers.len(), 1, "no temp file may remain: {leftovers:?}");
}

#[test]
fn create_document_strips_a_trailing_op_extension() {
    let tree = Tree::new();
    let project = tree.project();

    for name in ["plan", "plan.op"] {
        let target = resolve(&project, name, &tree).expect(name);
        assert_eq!(target.name, "plan", "{name}");
        assert_eq!(target.file_path, project.join("designs").join("plan.op"));
    }
    // Only one extension is taken off, and a stem keeps its other dots.
    let target = resolve(&project, "v1.2 draft.op", &tree).expect("dotted stem");
    assert_eq!(target.file_path.file_name().unwrap(), "v1.2 draft.op");
    // Unicode letters, spaces, dashes and underscores are all kept.
    let target = resolve(&project, "Trang chủ – bản_1", &tree).expect("unicode");
    assert_eq!(target.name, "Trang chủ – bản_1");
}

#[test]
fn create_document_rejects_unsafe_names() {
    let tree = Tree::new();
    let project = tree.project();
    let long = "a".repeat(101);
    let table: [(&str, NameProblem); 9] = [
        ("", NameProblem::Empty),
        ("   ", NameProblem::Empty),
        ("../x", NameProblem::PathSeparator),
        ("a/b", NameProblem::PathSeparator),
        ("a\\b", NameProblem::PathSeparator),
        ("a..b", NameProblem::ParentReference),
        (".hidden", NameProblem::LeadingDot),
        ("x\0y", NameProblem::ControlCharacter),
        (&long, NameProblem::TooLong { max: 100 }),
    ];
    for (name, problem) in table {
        assert_eq!(
            resolve(&project, name, &tree),
            Err(CreateDocumentError::InvalidName { problem }),
            "{name:?}"
        );
    }
    for name in ["tab\there", "line\nbreak", " padded", "padded .op"] {
        assert!(
            matches!(
                resolve(&project, name, &tree),
                Err(CreateDocumentError::InvalidName { .. })
            ),
            "{name:?}"
        );
    }
    // Exactly the limit is fine, and nothing was created by the refusals.
    resolve(&project, &"a".repeat(100), &tree).expect("100 characters is allowed");
    assert!(!project.join("designs").exists());
}

#[test]
fn create_document_rejects_bad_project_roots() {
    let tree = Tree::new();
    let home = tree.home();
    let file = tree.0.join("a-file");
    std::fs::write(&file, "x").unwrap();
    let home_str = home.to_str().unwrap();

    let relative = resolve_target("some/dir", "x", Some(&home));
    assert!(matches!(
        relative,
        Err(CreateDocumentError::ProjectRootNotAbsolute { .. })
    ));
    let missing = resolve_target(tree.0.join("nope").to_str().unwrap(), "x", Some(&home));
    assert!(matches!(
        missing,
        Err(CreateDocumentError::ProjectRootUnreadable { .. })
    ));
    let not_a_directory = resolve_target(file.to_str().unwrap(), "x", Some(&home));
    assert_eq!(
        not_a_directory,
        Err(CreateDocumentError::ProjectRootNotDirectory { path: file })
    );
    let nul = resolve_target("/tmp/a\0b", "x", Some(&home));
    assert!(matches!(
        nul,
        Err(CreateDocumentError::ProjectRootUnreadable { .. })
    ));
    // The filesystem root and the (injected) home directory itself.
    let root = std::fs::canonicalize("/").unwrap();
    assert_eq!(
        resolve_target("/", "x", Some(&home)),
        Err(CreateDocumentError::ProjectRootForbidden { path: root })
    );
    assert_eq!(
        resolve_target(home_str, "x", Some(&home)),
        Err(CreateDocumentError::ProjectRootForbidden { path: home })
    );
    assert!(extract_args(&BTreeMap::new()).is_err());
    let blank_root = BTreeMap::from([("projectRoot".to_string(), "  ".to_string())]);
    assert_eq!(
        extract_args(&blank_root),
        Err(CreateDocumentError::MissingArgument("projectRoot"))
    );
    let no_name = BTreeMap::from([("projectRoot".to_string(), "/x".to_string())]);
    assert_eq!(
        extract_args(&no_name),
        Err(CreateDocumentError::MissingArgument("name"))
    );
}

#[test]
fn create_document_refuses_existing_file_without_overwriting() {
    let tree = Tree::new();
    let project = tree.project();
    let designs = project.join("designs");
    std::fs::create_dir_all(&designs).unwrap();
    let existing = designs.join("taken.op");
    std::fs::write(&existing, "precious").unwrap();

    assert_eq!(
        resolve(&project, "taken", &tree),
        Err(CreateDocumentError::FileExists {
            path: existing.clone()
        })
    );
    // Even when the check was passed earlier and the file appeared since, the
    // write itself refuses and leaves the file and the directory untouched.
    let racing = {
        let target = resolve(&project, "later", &tree).expect("free name");
        std::fs::write(&target.file_path, "appeared meanwhile").unwrap();
        target
    };
    assert_eq!(
        write_new_document(&untitled_state(), &racing),
        Err(CreateDocumentError::FileExists {
            path: racing.file_path.clone()
        })
    );
    assert_eq!(std::fs::read_to_string(&existing).unwrap(), "precious");
    assert_eq!(
        std::fs::read_to_string(&racing.file_path).unwrap(),
        "appeared meanwhile"
    );
    let mut names: Vec<_> = std::fs::read_dir(&designs)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(names, ["later.op", "taken.op"], "no temp file left behind");
}

#[cfg(unix)]
#[test]
fn create_document_refuses_a_designs_symlink_that_escapes_the_project() {
    let tree = Tree::new();
    let project = tree.project();
    let outside = tree.0.join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::os::unix::fs::symlink(&outside, project.join("designs")).unwrap();

    let refused = resolve(&project, "sneaky", &tree);
    assert!(
        matches!(
            refused,
            Err(CreateDocumentError::DesignsEscapesProject { .. })
        ),
        "{refused:?}"
    );
    assert_eq!(std::fs::read_dir(&outside).unwrap().count(), 0);

    // A dangling link is refused too, and never followed by create_dir_all.
    std::fs::remove_file(project.join("designs")).unwrap();
    std::os::unix::fs::symlink(tree.0.join("missing"), project.join("designs")).unwrap();
    assert!(matches!(
        resolve(&project, "sneaky", &tree),
        Err(CreateDocumentError::DesignsEscapesProject { .. })
    ));

    // A link to another folder INSIDE the project is allowed.
    std::fs::remove_file(project.join("designs")).unwrap();
    std::fs::create_dir_all(project.join("shared")).unwrap();
    std::os::unix::fs::symlink(project.join("shared"), project.join("designs")).unwrap();
    let target = resolve(&project, "fine", &tree).expect("inside link");
    assert_eq!(target.file_path, project.join("shared").join("fine.op"));
}

#[test]
fn create_document_refuses_a_designs_that_is_a_file() {
    let tree = Tree::new();
    let project = tree.project();
    std::fs::write(project.join("designs"), "not a folder").unwrap();

    assert!(matches!(
        resolve(&project, "x", &tree),
        Err(CreateDocumentError::DesignsNotDirectory { .. })
    ));
}

#[test]
fn a_failed_write_removes_the_designs_directory_it_created() {
    let tree = Tree::new();
    let project = tree.project();
    let elsewhere = tree.0.join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    let target = resolve(&project, "gone", &tree).expect("resolves");
    // The containment re-check after `create_dir_all` fails when the folder
    // is not inside the project it claims to belong to.
    let mismatched = CreateTarget {
        project_root: elsewhere,
        ..target
    };

    let error = write_new_document(&untitled_state(), &mismatched);

    assert!(
        matches!(
            error,
            Err(CreateDocumentError::DesignsEscapesProject { .. })
        ),
        "{error:?}"
    );
    assert!(
        !project.join("designs").exists(),
        "the empty folder is removed"
    );
}

#[test]
fn the_registry_entry_names_the_missing_editor() {
    let outcome = CreateDocument.call(&BTreeMap::new());
    let ToolOutcome::Err(code, message) = outcome else {
        panic!("expected a refusal");
    };
    assert_eq!(code, ToolErrorCode::ToolFailed);
    assert!(message.contains("live desktop editor"), "{message}");
}

#[test]
fn headless_mcp_mode_refuses_create_document() {
    let backing = PathBuf::from("/work/project/doc.op");
    let line = r#"{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"create_document","arguments":{"projectRoot":"/work/project","name":"x"}}}"#;

    let response = refuse_in_headless(line, &backing, McpAccessProfile::UNRESTRICTED)
        .expect("a headless document always has a file");

    assert!(response.contains(r#""isError":true"#), "{response}");
    assert!(response.contains(r#""id":7"#), "{response}");
    assert!(
        response.contains("already saved at /work/project/doc.op"),
        "{response}"
    );
    assert!(response.contains("save_document"), "{response}");
    // Other tools are not this module's business, and the lean profile's own
    // refusal (tool-not-in-profile) is left to the normal dispatch.
    let other = line.replace("create_document", "get_selection");
    assert!(refuse_in_headless(&other, &backing, McpAccessProfile::UNRESTRICTED).is_none());
    assert!(refuse_in_headless(line, &backing, McpAccessProfile::LEAN).is_none());
}

#[test]
fn the_lean_profile_still_refuses_create_document_as_not_in_profile() {
    let line = r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"create_document","arguments":{"projectRoot":"/work","name":"x"}}}"#;
    let mut state = EditorState::new();

    let response = crate::mcp_serve::process_message_with_applier_profiled(
        &mut state,
        line,
        McpAccessProfile::LEAN,
        |_, _, _| false,
    )
    .expect("dispatch")
    .expect("a reply");

    assert!(response.contains("tool-not-in-profile"), "{response}");
    assert!(response.contains(r#""isError":true"#), "{response}");
}

#[test]
fn headless_dispatch_answers_before_the_registry() {
    let dir = Tree::new();
    let backing = dir.0.join("doc.op");
    let mut state = EditorState::new();
    let line = r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"create_document","arguments":{"projectRoot":"/tmp","name":"x"}}}"#;

    let response = crate::mcp_serve::process_message(&mut state, &backing, line)
        .expect("dispatch")
        .expect("a reply");

    assert!(response.contains("already saved at"), "{response}");
    assert!(!backing.exists(), "a refusal writes nothing");
}
