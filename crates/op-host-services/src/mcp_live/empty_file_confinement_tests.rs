//! The live server initializes an empty `.op` file named by `filePath` only
//! where the path policy already allows an edit; everywhere else it refuses
//! before any byte is written.

use std::path::{Path, PathBuf};

use super::test_support::*;
use super::*;

struct Workspace {
    scratch: PathBuf,
}

impl Workspace {
    fn new(tag: &str) -> Self {
        let scratch = std::env::temp_dir().join(format!(
            "openpencil-empty-confinement-{tag}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&scratch);
        std::fs::create_dir_all(scratch.join("proj/.git")).expect("git marker");
        std::fs::create_dir_all(scratch.join("proj/designs")).expect("designs");
        std::fs::create_dir_all(scratch.join("proj/.hidden")).expect("hidden");
        std::fs::create_dir_all(scratch.join("outside")).expect("outside");
        std::fs::write(scratch.join("proj/designs/a.op"), "{}").expect("a.op");
        Self { scratch }
    }

    fn open_document(&self, target: &Path) -> String {
        let mut resolver = TestResolver::with_ids(&[(1, true)]);
        resolver.document_mut(1).path = Some(self.scratch.join("proj/designs/a.op"));
        let quoted = serde_json::to_string(&target.to_string_lossy()).expect("path json");
        let body = tool_call(
            1,
            "open_document",
            &format!(r#"{},"filePath":{quoted}"#, document_arg(1)),
        );
        post("/mcp", "", &body, &LiveDocuments::default(), resolver, None).response
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.scratch);
    }
}

#[test]
fn mcp_open_document_initializes_an_empty_file_inside_an_allowed_root_and_refuses_outside() {
    let workspace = Workspace::new("roots");
    let inside = workspace.scratch.join("proj/designs/new.op");
    let outside = workspace.scratch.join("outside/new.op");
    let hidden = workspace.scratch.join("proj/.hidden/new.op");
    for path in [&inside, &outside, &hidden] {
        std::fs::write(path, "").expect("empty file");
    }

    let response = workspace.open_document(&inside);
    assert!(!is_error(&response), "{response}");
    assert!(
        !crate::doc_io::bytes_are_blank(&std::fs::read(&inside).expect("read")),
        "formatted inside the root"
    );

    for refused in [&outside, &hidden] {
        let response = workspace.open_document(refused);
        assert!(is_error(&response), "{}: {response}", refused.display());
        assert_eq!(
            std::fs::read(refused).expect("read"),
            b"",
            "{} stays empty",
            refused.display()
        );
    }
}
