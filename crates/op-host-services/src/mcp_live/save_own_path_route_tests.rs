//! `save_document { filePath }` where `filePath` is the addressed document's
//! OWN file, through the real router and a real pump. Nested in
//! `save_in_place_route_tests` for its workspace and helpers.
//!
//! The rule under test: the own file saves in place (every in-place guard
//! stands), any other path is an ordinary copy and keeps the path policy's
//! verdict, byte for byte.

use super::*;
use op_editor_core::CollabConnectionPhase;

fn quoted(path: &std::path::Path) -> String {
    serde_json::to_string(&path.to_string_lossy()).unwrap()
}

fn file_path_arg(path: &std::path::Path) -> String {
    format!(r#","filePath":{}"#, quoted(path))
}

/// `d1` (bound to `a.op`, dirty, baseline recorded) plus `d2` (bound to
/// `b.op`, parked but reachable): two tabs in one project.
fn two_tabs(workspace: &Workspace) -> (TestResolver, PathBuf) {
    let mut resolver = workspace.resolver();
    let other = workspace.scratch.join("proj/designs/b.op");
    let seed = EditorState::starter();
    crate::doc_io::save_to_path(&seed, &other).expect("seed b");
    resolver.documents.push(TestDocument {
        id: DocumentId(2),
        label: "b.op".to_string(),
        path: Some(other.clone()),
        state: seed,
        host_epoch: 0,
        active: false,
        status: op_editor_core::DocumentTabStatus::Idle,
    });
    resolver.lend_parked = true;
    resolver.track_baselines();
    (resolver, other)
}

fn assert_saved_in_place(posted: &mut Posted, workspace: &Workspace, before: &[u8]) {
    assert!(!is_error(&posted.response), "{}", posted.response);
    let reply = result_json(&posted.response);
    assert_eq!(reply["saved"], serde_json::json!(true));
    assert_eq!(reply["path"], workspace.file().to_string_lossy().as_ref());
    assert_eq!(reply["dirtyBefore"], serde_json::json!(true));
    let written = std::fs::read(workspace.file()).unwrap();
    assert_ne!(written, before, "the file now holds the agent's edit");
    assert_eq!(reply["bytes"], serde_json::json!(written.len()));
    assert!(!posted.ui.resolver.document_mut(1).state.is_dirty());
    assert_eq!(posted.ui.saved.len(), 1);
}

/// Invariant: the natural call, `filePath` = the open file, saves it in place
/// like the call without a `filePath`.
#[test]
fn file_path_equal_to_own_file_saves_in_place() {
    let workspace = Workspace::new("own-path");
    let mut resolver = workspace.resolver();
    resolver.document_mut(1).state.add_page();
    let before = std::fs::read(workspace.file()).unwrap();
    let body = save_body(&file_path_arg(&workspace.file()));

    let mut posted = post_save(&body, resolver, &LiveDocuments::default());

    assert_saved_in_place(&mut posted, &workspace, &before);
    let loaded =
        crate::doc_io::load_editor_state(&workspace.file(), op_editor_core::Locale::EnUs).unwrap();
    assert_eq!(loaded.doc, posted.ui.resolver.document_mut(1).state.doc);
    assert_eq!(
        std::fs::read_dir(workspace.file().parent().unwrap())
            .unwrap()
            .count(),
        1,
        "no copy and no leftover temp file next to the document"
    );
}

/// Invariant: the path is judged by what file it names, not how it is spelled
/// (the path policy's own equality: `..`, symlinks, the canonical form of a
/// symlinked temp dir, and the on-disk case on a case-insensitive volume).
#[test]
fn own_path_in_other_spelling_saves_in_place() {
    let workspace = Workspace::new("spellings");
    drop(workspace.resolver()); // seeds the file
    let designs = workspace.file().parent().unwrap().to_path_buf();
    let mut spellings = vec![
        workspace.scratch.join("proj/designs/../designs/a.op"),
        std::fs::canonicalize(workspace.file()).unwrap(),
    ];
    #[cfg(unix)]
    {
        let link = workspace.scratch.join("link");
        std::os::unix::fs::symlink(&designs, &link).expect("symlink");
        spellings.push(link.join("a.op"));
    }
    let shouting = designs.join("A.OP");
    // On a case-insensitive volume `A.OP` IS the file.
    if shouting.exists() {
        spellings.push(shouting);
    }
    for spelling in spellings {
        let mut resolver = workspace.resolver();
        resolver.document_mut(1).state.add_page();
        let before = std::fs::read(workspace.file()).unwrap();

        let mut posted = post_save(
            &save_body(&file_path_arg(&spelling)),
            resolver,
            &LiveDocuments::default(),
        );

        assert_saved_in_place(&mut posted, &workspace, &before);
    }
}

/// Invariant: a path another tab holds is still refused with the path
/// policy's message, never saved, and the other tab's file is untouched.
#[test]
fn file_path_of_another_open_tab_is_still_refused_with_its_message() {
    let workspace = Workspace::new("other-tab");
    let (resolver, other) = two_tabs(&workspace);
    let before_other = std::fs::read(&other).unwrap();
    let before_own = std::fs::read(workspace.file()).unwrap();

    let mut posted = post_save(
        &save_body(&file_path_arg(&other)),
        resolver,
        &LiveDocuments::default(),
    );

    assert!(is_error(&posted.response), "{}", posted.response);
    let text = error_text(&posted.response);
    assert!(
        text.contains(&path_policy::PathRefusal::OpenInEditor.to_string()),
        "{text}"
    );
    assert!(text.contains("open in an editor tab"), "{text}");
    assert_eq!(std::fs::read(&other).unwrap(), before_other);
    assert_eq!(std::fs::read(workspace.file()).unwrap(), before_own);
    assert!(posted.ui.saved.is_empty());
    assert!(posted.ui.resolver.document_mut(1).state.is_dirty());
}

/// Invariant: the path is compared with the document the call is addressed
/// to: the other tab's own file saves THAT tab in place (by `documentId` and
/// by the bound route), and this tab's file is refused there.
#[test]
fn own_path_is_judged_against_the_addressed_document() {
    let workspace = Workspace::new("addressed");
    let (mut resolver, other) = two_tabs(&workspace);
    resolver.document_mut(2).state.mark_document_changed();

    let by_id = tool_call(
        7,
        "save_document",
        &format!("{}{}", document_arg(2), file_path_arg(&other)),
    );
    let mut posted = post_save(&by_id, resolver, &LiveDocuments::default());
    assert!(!is_error(&posted.response), "{}", posted.response);
    assert_eq!(
        result_json(&posted.response)["documentId"],
        full(2).as_str()
    );
    assert_eq!(posted.ui.saved.len(), 1);
    assert_eq!(posted.ui.saved[0].0, DocumentId(2));
    assert!(
        posted.ui.resolver.document_mut(1).state.is_dirty(),
        "d1 is not saved"
    );

    // Bound to d2 by the route, d1's file is somebody else's tab.
    let (resolver, _) = two_tabs(&workspace);
    let bound = tool_call(7, "save_document", &file_path_arg(&workspace.file())[1..]);
    let refused = post(
        &format!("/mcp/d/{}", full(2)),
        "",
        &bound,
        &LiveDocuments::default(),
        resolver,
        None,
    );
    assert!(is_error(&refused.response), "{}", refused.response);
    assert!(error_text(&refused.response).contains("open in an editor tab"));
    assert!(refused.ui.saved.is_empty());
}

/// Invariant: a new path is a copy: written, the bound file untouched, the
/// document still dirty.
#[test]
fn copy_to_a_new_path_still_writes_a_copy_and_does_not_touch_the_bound_file() {
    let workspace = Workspace::new("copy");
    let resolver = workspace.resolver();
    let before = std::fs::read(workspace.file()).unwrap();
    let copy = workspace.scratch.join("proj/designs/copy.op");

    let mut posted = post_save(
        &save_body(&file_path_arg(&copy)),
        resolver,
        &LiveDocuments::default(),
    );

    assert!(!is_error(&posted.response), "{}", posted.response);
    assert!(copy.is_file());
    assert_eq!(std::fs::read(workspace.file()).unwrap(), before);
    assert!(posted.ui.saved.is_empty());
    assert!(
        !posted.ui.seen.contains(&"save"),
        "a copy never reaches the save request: {:?}",
        posted.ui.seen
    );
    assert!(posted.ui.resolver.document_mut(1).state.is_dirty());
}

/// Invariant: the own-path form keeps every guard of the in-place save and
/// never falls back to the copy writer once the path is the document's.
#[test]
fn own_path_save_keeps_every_in_place_guard() {
    // The file changed on disk since it was opened.
    let workspace = Workspace::new("guard-disk");
    let resolver = workspace.resolver();
    std::thread::sleep(std::time::Duration::from_millis(20));
    std::fs::write(workspace.file(), b"someone else's work").unwrap();
    let mut posted = post_save(
        &save_body(&file_path_arg(&workspace.file())),
        resolver,
        &LiveDocuments::default(),
    );
    assert!(is_error(&posted.response), "{}", posted.response);
    assert!(error_text(&posted.response).contains("the file changed on disk since it was opened"));
    assert_eq!(
        std::fs::read(workspace.file()).unwrap(),
        b"someone else's work"
    );
    assert!(posted.ui.resolver.document_mut(1).state.is_dirty());

    // A Cmd+S write is still running.
    let workspace = Workspace::new("guard-flight");
    let mut resolver = workspace.resolver();
    resolver.save_in_flight = true;
    let before = std::fs::read(workspace.file()).unwrap();
    let mut posted = post_save(
        &save_body(&file_path_arg(&workspace.file())),
        resolver,
        &LiveDocuments::default(),
    );
    assert!(is_error(&posted.response), "{}", posted.response);
    assert!(error_text(&posted.response).contains("NOT saved"));
    assert_eq!(std::fs::read(workspace.file()).unwrap(), before);
    assert!(posted.ui.resolver.document_mut(1).state.is_dirty());

    // The editor has not recorded the file's state.
    let workspace = Workspace::new("guard-baseline");
    let mut resolver = workspace.resolver();
    resolver.baselines = Some(Default::default());
    let before = std::fs::read(workspace.file()).unwrap();
    let posted = post_save(
        &save_body(&file_path_arg(&workspace.file())),
        resolver,
        &LiveDocuments::default(),
    );
    assert!(is_error(&posted.response), "{}", posted.response);
    assert!(error_text(&posted.response).contains("has not recorded the state"));
    assert_eq!(std::fs::read(workspace.file()).unwrap(), before);

    // A collaboration session.
    let workspace = Workspace::new("guard-collab");
    let mut resolver = workspace.resolver();
    resolver.document_mut(1).state.editor_ui.collab.phase = CollabConnectionPhase::Starting;
    let before = std::fs::read(workspace.file()).unwrap();
    let posted = post_save(
        &save_body(&file_path_arg(&workspace.file())),
        resolver,
        &LiveDocuments::default(),
    );
    assert!(is_error(&posted.response), "{}", posted.response);
    assert!(error_text(&posted.response).contains("collaboration session"));
    assert_eq!(std::fs::read(workspace.file()).unwrap(), before);
    assert!(posted.ui.saved.is_empty());

    // An untitled document has no own file: the path is just a path, judged
    // by the policy as before (no open folder to allow).
    let mut state = EditorState::starter();
    state.mark_document_changed();
    let posted = post_save(
        &save_body(r#","filePath":"/somewhere/new.op""#),
        TestResolver::single(state),
        &LiveDocuments::default(),
    );
    assert!(is_error(&posted.response), "{}", posted.response);
    assert!(error_text(&posted.response).contains("no folder is open"));
    assert!(posted.ui.saved.is_empty());
}

/// Invariant: a retried own-path save with the same `operationId` replays the
/// first answer and writes nothing more.
#[test]
fn own_path_retry_with_same_operation_id_saves_once() {
    let workspace = Workspace::new("own-retry");
    let documents = LiveDocuments::default();
    let body = save_body(&format!(
        r#"{},"operationId":"save-own-1""#,
        file_path_arg(&workspace.file())
    ));

    let first = post_save(&body, workspace.resolver(), &documents);
    assert!(!is_error(&first.response), "{}", first.response);
    assert_eq!(first.ui.saved.len(), 1);
    let on_disk = std::fs::read(workspace.file()).unwrap();

    let mut resolver = first.ui.resolver;
    resolver.document_mut(1).state.mark_document_changed();
    let mut retried = post_save(&body, resolver, &documents);

    assert_eq!(
        crate::mcp_serve::tool_text(&retried.response),
        crate::mcp_serve::tool_text(&first.response)
    );
    assert_eq!(
        json_body(&retried.response)["result"]["_meta"]["replayed"],
        serde_json::json!(true)
    );
    assert!(retried.ui.saved.is_empty(), "no second write");
    assert_eq!(std::fs::read(workspace.file()).unwrap(), on_disk);
    assert!(retried.ui.resolver.document_mut(1).state.is_dirty());
}

/// Invariant: `sourceFilePath` asks for a different operation (copy THAT
/// file), so a call that carries it is never the in-place form, even with the
/// own path as `filePath`: it is refused as before and nothing is saved.
#[test]
fn source_file_path_with_own_path_is_not_in_place() {
    let workspace = Workspace::new("source");
    let resolver = workspace.resolver();
    let own = file_path_arg(&workspace.file());
    let body = save_body(&format!(
        r#"{own},"sourceFilePath":{}"#,
        quoted(&workspace.file())
    ));
    let call = op_mcp::parse_tool_call(&body).unwrap();
    assert_eq!(crate::save_in_place::own_path_claim(&call), None);
    assert!(!crate::save_in_place::is_in_place_save_call(&call));
    let before = std::fs::read(workspace.file()).unwrap();

    let mut posted = post_save(&body, resolver, &LiveDocuments::default());

    assert!(is_error(&posted.response), "{}", posted.response);
    assert!(error_text(&posted.response).contains("open in an editor tab"));
    assert!(!posted.ui.seen.contains(&"save"), "{:?}", posted.ui.seen);
    assert_eq!(std::fs::read(workspace.file()).unwrap(), before);
    assert!(posted.ui.resolver.document_mut(1).state.is_dirty());
}
