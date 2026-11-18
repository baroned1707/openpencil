//! Headless `save_document` without a `filePath`: the loaded file is saved in
//! place and the answer has the live editor's shape.

use super::super::{process_message, tool_text};
use op_editor_core::EditorState;

fn scratch(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "openpencil-headless-save-in-place-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

fn save_line(arguments: &str) -> String {
    format!(
        r#"{{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{{"name":"save_document","arguments":{arguments}}}}}"#
    )
}

#[test]
fn headless_save_document_without_file_path_saves_the_loaded_file_in_place() {
    let dir = scratch("saves");
    let file = dir.join("loaded.op");
    let mut state = EditorState::starter();
    state.add_page();
    // The file on disk is stale: the in-place save rewrites it from `state`.
    std::fs::write(&file, b"{}").unwrap();

    let response = process_message(&mut state, &file, &save_line("{}"))
        .expect("dispatch")
        .expect("response");

    let reply: serde_json::Value = serde_json::from_str(&tool_text(&response)).expect("JSON");
    assert_eq!(reply["saved"], serde_json::json!(true));
    assert_eq!(reply["path"], file.to_string_lossy().as_ref());
    assert_eq!(reply["documentId"], "d1");
    assert_eq!(reply["dirtyBefore"], serde_json::json!(false));
    let written = std::fs::read(&file).unwrap();
    assert_eq!(reply["bytes"], serde_json::json!(written.len()));
    let loaded =
        crate::doc_io::load_editor_state(&file, op_editor_core::Locale::EnUs).expect("loads");
    assert_eq!(loaded.doc, state.doc);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn headless_save_document_with_a_file_path_still_copies() {
    let dir = scratch("copy");
    let file = dir.join("loaded.op");
    let copy = dir.join("copy.op");
    let mut state = EditorState::starter();
    crate::doc_io::save_to_path(&state, &file).unwrap();
    let arguments = serde_json::json!({"filePath": copy.to_string_lossy()}).to_string();

    let response = process_message(&mut state, &file, &save_line(&arguments))
        .expect("dispatch")
        .expect("response");

    assert!(!response.contains("isError"), "{response}");
    assert!(copy.is_file());
    let _ = std::fs::remove_dir_all(dir);
}

/// Invariant: in headless mode too, a `filePath` that is the loaded file
/// (however it is spelled) is the in-place save, with the in-place answer
/// shape rather than a copy's.
#[test]
fn headless_save_document_with_the_loaded_file_as_file_path_saves_in_place() {
    let dir = scratch("own-path");
    let file = dir.join("loaded.op");
    let mut state = EditorState::starter();
    state.add_page();
    std::fs::write(&file, b"{}").unwrap();
    let respelled = dir
        .join("../")
        .join(dir.file_name().unwrap())
        .join("loaded.op");

    for spelling in [file.clone(), respelled] {
        let arguments = serde_json::json!({"filePath": spelling.to_string_lossy()}).to_string();
        std::fs::write(&file, b"{}").unwrap();

        let response = process_message(&mut state, &file, &save_line(&arguments))
            .expect("dispatch")
            .expect("response");

        let reply: serde_json::Value = serde_json::from_str(&tool_text(&response)).expect("JSON");
        assert_eq!(reply["saved"], serde_json::json!(true), "{response}");
        assert_eq!(reply["path"], file.to_string_lossy().as_ref());
        assert_eq!(reply["documentId"], "d1");
        let loaded =
            crate::doc_io::load_editor_state(&file, op_editor_core::Locale::EnUs).expect("loads");
        assert_eq!(loaded.doc, state.doc);
    }
    let _ = std::fs::remove_dir_all(dir);
}
