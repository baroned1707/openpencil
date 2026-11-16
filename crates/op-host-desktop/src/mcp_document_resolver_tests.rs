use super::*;
use op_editor_core::DocumentTabStatus;

fn listing(id: u64, active: bool) -> DocumentListing {
    DocumentListing {
        id: DocumentId(id),
        label: format!("doc{id}.op"),
        path: None,
        project: String::new(),
        dirty: false,
        active,
        status: DocumentTabStatus::Idle,
    }
}

#[test]
fn lookup_tells_active_from_parked_and_unknown() {
    let listings = vec![listing(3, true), listing(4, false)];
    assert_eq!(lookup(&listings, DocumentId(3)), Ok(true));
    assert_eq!(lookup(&listings, DocumentId(4)), Ok(false));
    assert_eq!(lookup(&listings, DocumentId(9)), Err(ResolveError::Unknown));
}
