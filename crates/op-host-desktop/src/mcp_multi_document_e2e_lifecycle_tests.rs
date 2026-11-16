//! Document lifecycle against the live MCP server over real HTTP: a document
//! replaced between a call's snapshot and its apply, retried writes,
//! `create_document`, `filePath` of an open document, saving after an agent
//! edit, and the server going away mid call.

use super::multi_document_e2e_support::*;
use crate::document_registry::DocumentRegistry;
use crate::mcp_document_resolver::DesktopDocumentResolver;
use crate::DesktopApp;
use op_editor_core::{DocumentId, EditorState};
use op_host_native::WidgetHostNative;
use op_host_services::mcp_live::{DocumentListing, DocumentResolver, ResolveError};
use serde_json::{json, Value};
use std::time::{Duration, Instant};

/// How the interposer replaces the document under a call.
#[derive(Clone, Copy)]
enum Replacement {
    /// `EditorState::replace_document` through the lens: bumps the document
    /// generation (what a whole-document MCP sync does).
    Generation,
    /// `WidgetHostNative::replace_editor_state`: bumps the host epoch (what
    /// Open / New / import do). Only for the document on screen.
    HostEpoch,
}

/// The desktop's resolver, except that the first time `target` is lent (a
/// call's snapshot) the document is replaced right after the lend and before
/// the snapshot is acknowledged, so the apply that follows is deterministically
/// stale. No sleeps, no race with the connection thread.
struct ReplacingResolver<'a> {
    host: &'a mut WidgetHostNative,
    documents: &'a mut DocumentRegistry,
    listings: Vec<DocumentListing>,
    target: DocumentId,
    replacement: Replacement,
    replaced: &'a mut bool,
}

impl DocumentResolver for ReplacingResolver<'_> {
    fn documents(&self) -> Vec<DocumentListing> {
        self.listings.clone()
    }

    fn with_document(
        &mut self,
        id: DocumentId,
        f: &mut dyn FnMut(&mut EditorState, u64),
    ) -> Result<(), ResolveError> {
        let mut real = DesktopDocumentResolver::new(
            &mut *self.host,
            &mut *self.documents,
            self.listings.clone(),
        );
        let result = real.with_document(id, f);
        if id == self.target && !*self.replaced && result.is_ok() {
            *self.replaced = true;
            match self.replacement {
                Replacement::Generation => {
                    DesktopDocumentResolver::new(
                        &mut *self.host,
                        &mut *self.documents,
                        self.listings.clone(),
                    )
                    .with_document(id, &mut |state, _| {
                        state.replace_document(EditorState::starter().doc)
                    })
                    .expect("replace through the lens");
                }
                Replacement::HostEpoch => {
                    assert!(self.host.replace_editor_state(EditorState::starter()));
                }
            }
        }
        result
    }
}

/// Answer the client while replacing `target`'s document under its first call.
fn call_with_replacement(
    fx: &mut Fixture,
    path: &str,
    arguments: Value,
    target: DocumentId,
    replacement: Replacement,
) -> Reply {
    let path = path.to_string();
    let mut replaced = false;
    let mut replies = fx.drive_with(
        vec![
            Box::new(move |port| post_tool(port, &path, "update_node", arguments))
                as Job<Result<Reply, String>>,
        ],
        |app: &mut DesktopApp| {
            let listings = app.mcp_document_listings();
            let Some(server) = app.mcp_server.as_mut() else {
                return;
            };
            let mut resolver = ReplacingResolver {
                host: &mut app.host,
                documents: &mut app.documents,
                listings,
                target,
                replacement,
                replaced: &mut replaced,
            };
            server.pump(&mut resolver);
        },
    );
    assert!(replaced, "the interposer never saw the call");
    replies.remove(0).expect("HTTP exchange")
}

#[test]
fn stale_epoch_apply_is_refused_after_the_document_was_replaced_in_the_slot() {
    // Parked `a`: replaced through the lens between snapshot and apply.
    let mut fx = Fixture::new("stale-generation");
    let a = fx.a;
    let reply = call_with_replacement(
        &mut fx,
        &Fixture::route(a),
        move_frame(77, 77),
        a,
        Replacement::Generation,
    );
    assert!(reply.is_error(), "a stale apply must be refused");
    assert!(
        reply.text().contains("changed while this call"),
        "{}",
        reply.text()
    );
    assert_eq!(
        fx.frame_x(fx.a),
        0.0,
        "the stale edit did not land in the new document"
    );

    // The advice in the refusal works: the retry sees the new document.
    let retry = fx.call(&Fixture::route(fx.a), "update_node", move_frame(78, 78));
    assert!(!retry.is_error(), "{}", retry.text());
    assert_eq!(fx.frame_x(fx.a), 78.0);
}

#[test]
fn stale_epoch_apply_is_refused_after_the_on_screen_document_was_swapped() {
    // On-screen `c`: swapped by an Open / New style replacement.
    let mut fx = Fixture::new("stale-host-epoch");
    let c = fx.c;
    let reply = call_with_replacement(
        &mut fx,
        &Fixture::route(c),
        move_frame(66, 66),
        c,
        Replacement::HostEpoch,
    );
    assert!(reply.is_error(), "{}", reply.text());
    assert!(
        reply.text().contains("changed while this call"),
        "{}",
        reply.text()
    );
    assert_eq!(fx.frame_x(fx.c), 0.0);
    // The parked documents were never involved.
    assert!(!fx.is_dirty(fx.a) && !fx.is_dirty(fx.b));
}

fn page_count(state: &EditorState) -> usize {
    state.doc.pages.as_ref().map_or(0, Vec::len)
}

#[test]
fn operation_id_retry_does_not_apply_twice() {
    let mut fx = Fixture::new("operation-id");
    let document = Fixture::name(fx.a);
    let add = |name: &str, operation: Option<&str>| {
        let mut arguments = json!({"name": name, "documentId": document});
        if let Some(operation) = operation {
            arguments["operationId"] = json!(operation);
        }
        arguments
    };
    let pages_before = fx.peek(fx.a, page_count);

    let first = fx.call("/mcp", "add_page", add("Retried", Some("op-1")));
    assert!(!first.is_error(), "{}", first.text());
    let pages_after_first = fx.peek(fx.a, page_count);
    // A document without pages grows a default page too, so measure the first
    // write's own delta instead of assuming +1.
    let one_write = pages_after_first - pages_before;
    assert!(one_write >= 1, "the first call applied");
    assert!(!first.replayed());

    let second = fx.call("/mcp", "add_page", add("Retried", Some("op-1")));
    assert!(!second.is_error(), "{}", second.text());
    assert!(second.replayed(), "the stored response is returned");
    assert_eq!(second.text(), first.text());
    assert_eq!(fx.peek(fx.a, page_count), pages_after_first, "applied once");

    // The same id for a different call is a client bug, refused untouched.
    let conflict = fx.call("/mcp", "add_page", add("Other", Some("op-1")));
    assert!(conflict.is_error());
    assert!(
        conflict.text().contains("operationId"),
        "{}",
        conflict.text()
    );
    assert_eq!(fx.peek(fx.a, page_count), pages_after_first);

    // Control: without an operationId the same call is a new write, and the
    // log is per document, so `b` may reuse the id.
    let plain = fx.call("/mcp", "add_page", add("Retried", None));
    assert!(!plain.is_error());
    assert_eq!(
        fx.peek(fx.a, page_count),
        pages_after_first + 1,
        "a new write adds one page"
    );
    let mut on_b = add("Retried", Some("op-1"));
    on_b["documentId"] = json!(Fixture::name(fx.b));
    assert!(!fx.call("/mcp", "add_page", on_b).is_error());
    assert_eq!(fx.peek(fx.b, page_count), pages_before + one_write);
}

fn bytes_of(fx: &Fixture) -> [Vec<u8>; 3] {
    [&fx.path_a, &fx.path_b, &fx.path_c].map(|path| read_bytes(path))
}

fn designs_listing(fx: &mut Fixture) -> Vec<Value> {
    fx.call("/mcp", "list_documents", json!({}))
        .json()
        .as_array()
        .cloned()
        .expect("listing")
}

fn row_of(rows: &[Value], id: DocumentId) -> Value {
    let wanted = Fixture::name(id);
    rows.iter()
        .find(|row| row["documentId"] == wanted.as_str())
        .cloned()
        .unwrap_or_else(|| panic!("{wanted} not listed"))
}

#[test]
fn create_document_over_http_binds_the_untitled_tab() {
    let mut fx = Fixture::new("create");
    assert!(fx.app.open_untitled_tab());
    let untitled = fx.app.documents.active_id();
    assert!(fx.app.current_path.is_none());
    let project = fx.p2.to_string_lossy().into_owned();
    let create = |name: &str, document: DocumentId| json!({"projectRoot": project, "name": name, "documentId": Fixture::name(document)});

    let reply = fx.call("/mcp", "create_document", create("landing", untitled));

    let expected = fx.p2.join("designs").join("landing.op");
    assert!(!reply.is_error(), "{}", reply.text());
    assert_eq!(
        reply.json()["filePath"],
        expected.to_string_lossy().as_ref()
    );
    assert!(
        expected.is_file(),
        "the file is at <project>/designs/<name>.op"
    );
    assert_eq!(fx.app.current_path.as_deref(), Some(expected.as_path()));
    let row = row_of(&designs_listing(&mut fx), untitled);
    assert_eq!(row["label"], "landing.op");
    assert_eq!(row["path"], expected.to_string_lossy().as_ref());
    // The listing's `project` is the project root's name, the same the tab
    // strip and the Projects dock use.
    assert_eq!(
        row["project"],
        fx.p2.file_name().unwrap().to_string_lossy().as_ref()
    );
    assert_eq!(row["dirty"], false);

    // Saved already: a second create on the same document is refused.
    let again = fx.call("/mcp", "create_document", create("second", untitled));
    assert!(
        again.is_error() && again.text().contains("already saved"),
        "{}",
        again.text()
    );
    assert!(!fx.p2.join("designs").join("second.op").exists());

    // A name that exists is refused and the file is left alone.
    assert!(fx.app.open_untitled_tab());
    let second_untitled = fx.app.documents.active_id();
    let written = read_bytes(&expected);
    let taken = fx.call(
        "/mcp",
        "create_document",
        create("landing", second_untitled),
    );
    assert!(taken.is_error(), "{}", taken.text());
    assert!(
        taken.text().contains("never overwritten"),
        "{}",
        taken.text()
    );
    assert_eq!(
        read_bytes(&expected),
        written,
        "the existing file is untouched"
    );
    assert!(fx.app.current_path.is_none(), "a refusal binds nothing");

    // A parked untitled document is bound in its own slot, not the screen's.
    let c_index = fx.app.documents.index_of(fx.c).unwrap();
    assert!(fx.app.activate_document_slot(c_index));
    let parked = fx.call(
        "/mcp",
        "create_document",
        create("parked-one", second_untitled),
    );
    assert!(!parked.is_error(), "{}", parked.text());
    let parked_file = fx.p2.join("designs").join("parked-one.op");
    assert!(parked_file.is_file());
    assert_eq!(fx.app.current_path.as_deref(), Some(fx.path_c.as_path()));
    let row = row_of(&designs_listing(&mut fx), second_untitled);
    assert_eq!(row["path"], parked_file.to_string_lossy().as_ref());
    assert_eq!(row["label"], "parked-one.op");
}

#[test]
fn filepath_of_an_open_document_is_refused_and_disk_is_untouched() {
    let mut fx = Fixture::new("filepath");
    let disk_before = bytes_of(&fx);
    let memory_before = [fx.a, fx.b, fx.c].map(|id| fx.fingerprint(id));
    let mut spellings = vec![
        fx.path_a.to_string_lossy().into_owned(),
        format!("{}/./a.op", fx.p1.display()),
        format!("{}/../p1/a.op", fx.p1.display()),
    ];
    #[cfg(unix)]
    {
        let link = fx.root.join("link-to-p1");
        std::os::unix::fs::symlink(&fx.p1, &link).expect("symlink");
        spellings.push(format!("{}/a.op", link.display()));
    }

    for spelling in &spellings {
        let mut arguments = move_frame(999, 999);
        arguments["filePath"] = json!(spelling);
        let reply = fx.call("/mcp", "update_node", arguments);
        assert!(
            reply.is_error(),
            "{spelling} was accepted: {}",
            reply.text()
        );
        assert!(
            reply.text().contains("omit filePath"),
            "{spelling}: {}",
            reply.text()
        );
    }

    assert_eq!(
        bytes_of(&fx),
        disk_before,
        "no refused spelling touched the disk"
    );
    assert_eq!(
        [fx.a, fx.b, fx.c].map(|id| fx.fingerprint(id)),
        memory_before
    );

    // Control: a file the editor does NOT have open is still edited on disk,
    // and the open documents stay as they were.
    let closed = fx.p1.join("closed.op");
    op_host_services::doc_io::save_to_path(&EditorState::starter(), &closed).unwrap();
    let mut arguments = move_frame(5, 6);
    arguments["filePath"] = json!(closed.to_string_lossy());
    let edited = fx.call("/mcp", "update_node", arguments);
    assert!(!edited.is_error(), "{}", edited.text());
    let reloaded =
        op_host_services::doc_io::load_editor_state(&closed, op_editor_core::Locale::EnUs)
            .expect("reload closed file");
    assert_eq!(frame_x_of(&reloaded), 5.0);
    assert_eq!(
        [fx.a, fx.b, fx.c].map(|id| fx.fingerprint(id)),
        memory_before
    );
}

fn frame_x_of(state: &EditorState) -> f64 {
    serde_json::to_value(&state.doc.children[0]).unwrap()["x"]
        .as_f64()
        .unwrap_or(0.0)
}

#[test]
fn parked_document_survives_save_after_agent_edit() {
    let mut fx = Fixture::new("save-parked");
    let mut arguments = move_frame(55, 56);
    arguments["documentId"] = json!(Fixture::name(fx.a));
    assert!(!fx.call("/mcp", "update_node", arguments).is_error());
    assert!(fx.is_dirty(fx.a));
    let [disk_b, disk_c] = [read_bytes(&fx.path_b), read_bytes(&fx.path_c)];

    // The user switches to `a` and saves through the app's own save path.
    let index = fx.app.documents.index_of(fx.a).unwrap();
    assert!(fx.app.activate_document_slot(index));
    assert!(fx.app.request_background_save());
    assert!(fx.app.finish_background_saves());

    let reloaded =
        op_host_services::doc_io::load_editor_state(&fx.path_a, op_editor_core::Locale::EnUs)
            .expect("reload a.op");
    assert_eq!(frame_x_of(&reloaded), 55.0, "the agent's edit is on disk");
    assert!(!fx.is_dirty(fx.a), "the save acknowledgement cleaned a");
    assert_eq!(read_bytes(&fx.path_b), disk_b);
    assert_eq!(read_bytes(&fx.path_c), disk_c);
}

#[test]
fn closing_the_server_mid_call_leaves_documents_intact() {
    let mut fx = Fixture::new("server-close");
    let disk_before = bytes_of(&fx);
    let memory_before = [fx.a, fx.b, fx.c].map(|id| fx.fingerprint(id));
    let port = fx.port;
    let mut arguments = move_frame(13, 13);
    arguments["documentId"] = json!(Fixture::name(fx.a));
    // The call is in flight (waiting for the UI thread, which is not pumping)
    // when the server is stopped the way a settings toggle stops it.
    let started = Instant::now();
    let client = std::thread::spawn(move || post_tool(port, "/mcp", "update_node", arguments));
    std::thread::sleep(Duration::from_millis(300));
    let mut server = fx.app.mcp_server.take().expect("server");
    server.stop();
    drop(server);

    let outcome = client.join().expect("client thread");
    let waited = started.elapsed();
    eprintln!("server-close: the client was released after {waited:?}");

    // A client must be released (error or closed connection) and never told
    // the write succeeded.
    if let Ok(reply) = &outcome {
        assert!(
            reply.is_error(),
            "a call never applied cannot succeed: {}",
            reply.text()
        );
    }
    assert!(
        waited < Duration::from_secs(10),
        "the client waited {waited:?} for a server that was already gone"
    );
    assert_eq!(
        [fx.a, fx.b, fx.c].map(|id| fx.fingerprint(id)),
        memory_before
    );
    assert_eq!(bytes_of(&fx), disk_before);
    assert!(![fx.a, fx.b, fx.c].into_iter().any(|id| fx.is_dirty(id)));

    // Starting the server again serves the same documents.
    assert!(fx.app.reconcile_mcp_server_from_settings());
    fx.port = fx.app.mcp_server.as_ref().expect("restarted").port();
    let mut again = move_frame(14, 14);
    again["documentId"] = json!(Fixture::name(fx.a));
    assert!(!fx.call("/mcp", "update_node", again).is_error());
    assert_eq!(fx.frame_x(fx.a), 14.0);
}
