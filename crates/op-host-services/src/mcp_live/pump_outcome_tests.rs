//! What a pump reports to the host names the document each request was
//! about, even when one pump answers requests about several documents.

use super::test_support::*;
use super::*;

fn replace_request() -> (UiRequest, Receiver<Result<bool, ResolveError>>) {
    let document = op_pen_loader::load_canonical(
        r#"{"version":"1.0","children":[],"pages":[{"id":"p1","name":"One","children":[]}]}"#,
    )
    .expect("replacement document")
    .value;
    let (ack, answer) = mpsc::sync_channel(1);
    let request = UiRequest::ReplaceDocument {
        doc: Box::new(document),
        editor_meta: op_pen_loader::EditorMeta {
            active_page_index: 0,
            preserve_authored_geometry: false,
            scenario: None,
            work_family: None,
            pinned_style_guide: None,
            share_recipe: None,
            imported_from: None,
        },
        ticket: Ticket::unbounded(),
        ack,
    };
    (request, answer)
}

fn list_pages_request(document: u64) -> UiRequest {
    let (ack, _answer) = mpsc::sync_channel(1);
    UiRequest::ListPages {
        document: DocumentId(document),
        ticket: Ticket::unbounded(),
        ack,
    }
}

#[test]
fn a_replace_is_recorded_against_the_on_screen_document_not_the_last_one_addressed() {
    let (mut server, queue) = server_with_queue();
    let mut resolver = TestResolver::with_ids(&[(1, true), (2, false)]);
    resolver.lend_parked = true;
    let (replace, answer) = replace_request();
    queue.send(replace).expect("queue replace");
    queue.send(list_pages_request(2)).expect("queue read");

    let outcome = server.pump(&mut resolver);

    assert_eq!(answer.try_recv(), Ok(Ok(true)));
    assert_eq!(
        outcome.replaced,
        vec![DocumentId(1)],
        "the replace belongs to the document on screen, whatever was read after it"
    );
    assert!(outcome.created.is_empty());
}
