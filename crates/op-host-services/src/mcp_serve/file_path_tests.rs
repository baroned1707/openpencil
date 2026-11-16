//! A per-call `filePath` must never edit, on disk, a document the editor has
//! open — the write would bypass the editor and be overwritten by its next
//! Save — while any other file keeps being served as before.

use super::*;
use crate::mcp_serve::tool_profile::McpAccessProfile;

fn scratch_dir(test_name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "openpencil-file-path-open-{test_name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

fn write_doc(path: &Path, name: &str) {
    std::fs::write(
        path,
        format!(
            r##"{{"version":"1.0.0","children":[{{"id":"n1","type":"rectangle","name":"{name}","x":0,"y":0,"width":100,"height":60,"fill":[{{"type":"solid","color":"#FFFFFF"}}]}}]}}"##
        ),
    )
    .expect("write doc");
}

/// An `add_page` call (a write tool) retargeted at `target`.
fn add_page_call(target: &Path) -> String {
    let target = serde_json::to_string(&target.to_string_lossy()).expect("path json");
    format!(
        r#"{{"jsonrpc":"2.0","id":41,"method":"tools/call","params":{{"name":"add_page","arguments":{{"filePath":{target},"name":"FromAgent"}}}}}}"#
    )
}

fn serve(open_paths: &[PathBuf], line: &str) -> Result<Option<String>, McpServeError> {
    process_message_for_file_path_arg(None, open_paths, line, McpAccessProfile::UNRESTRICTED)
}

fn assert_refused_as_open_document(response: &str) {
    assert!(response.contains(r#""isError":true"#), "{response}");
    assert!(response.contains("omit filePath"), "{response}");
    assert!(response.contains(r#""id":41"#), "{response}");
}

#[test]
fn file_path_matching_open_document_is_rejected_without_writing() {
    let dir = scratch_dir("reject");
    let open = dir.join("open.op");
    write_doc(&open, "Original");
    let before = std::fs::read(&open).expect("read before");

    let response = serve(std::slice::from_ref(&open), &add_page_call(&open))
        .expect("a refusal is a tool result, not a transport error")
        .expect("a response");

    assert_refused_as_open_document(&response);
    assert_eq!(
        std::fs::read(&open).expect("read after"),
        before,
        "the open document must not be touched on disk"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn file_path_for_a_different_file_still_works() {
    let dir = scratch_dir("other");
    let open = dir.join("open.op");
    let other = dir.join("other.op");
    write_doc(&open, "Open");
    write_doc(&other, "Other");
    let open_before = std::fs::read(&open).expect("read open");

    let response = serve(std::slice::from_ref(&open), &add_page_call(&other))
        .expect("dispatch")
        .expect("a response");

    assert!(response.contains(r#"\"wrote\":\"true\""#), "{response}");
    let other_text = std::fs::read_to_string(&other).expect("other doc");
    assert!(other_text.contains("FromAgent"), "{other_text}");
    assert_eq!(std::fs::read(&open).expect("read open"), open_before);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn file_path_matching_through_symlink_or_dotdot_is_rejected() {
    let dir = scratch_dir("aliases");
    let open = dir.join("open.op");
    write_doc(&open, "Original");
    std::fs::create_dir_all(dir.join("sub")).expect("sub dir");
    let before = std::fs::read(&open).expect("read before");
    let open_paths = [open.clone()];

    // `..` segment through a real directory.
    let dotdot = dir.join("sub").join("..").join("open.op");
    let response = serve(&open_paths, &add_page_call(&dotdot))
        .expect("dispatch")
        .expect("a response");
    assert_refused_as_open_document(&response);

    // Symlink to the open file, in both directions: the agent names the link
    // while the editor reports the real path, and the other way round.
    #[cfg(unix)]
    {
        let link = dir.join("link.op");
        std::os::unix::fs::symlink(&open, &link).expect("symlink");
        let response = serve(&open_paths, &add_page_call(&link))
            .expect("dispatch")
            .expect("a response");
        assert_refused_as_open_document(&response);
        let response = serve(std::slice::from_ref(&link), &add_page_call(&open))
            .expect("dispatch")
            .expect("a response");
        assert_refused_as_open_document(&response);
    }

    assert_eq!(
        std::fs::read(&open).expect("read after"),
        before,
        "no alias may reach the open document on disk"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn file_path_to_a_nonexistent_target_is_not_treated_as_open() {
    let dir = scratch_dir("missing");
    let open = dir.join("open.op");
    write_doc(&open, "Open");
    let missing = dir.join("missing.op");

    // Same outcome as without any open document: the load fails, and it is
    // not the open-document refusal.
    let with_open = serve(std::slice::from_ref(&open), &add_page_call(&missing));
    let without_open = serve(&[], &add_page_call(&missing));

    assert_eq!(with_open, without_open);
    assert!(!missing.exists(), "a refused load must not create the file");
    let _ = std::fs::remove_dir_all(dir);
}
