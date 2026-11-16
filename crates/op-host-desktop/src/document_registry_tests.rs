use super::*;
use op_editor_core::EditorState;

fn parked(dirty: bool) -> ParkedDocument {
    let mut state = EditorState::starter();
    if dirty {
        state.mark_document_changed();
    }
    ParkedDocument::from_state(state, Default::default())
}

fn local_with_path(path: &str) -> SlotLocalState {
    SlotLocalState {
        path: Some(PathBuf::from(path)),
        ..Default::default()
    }
}

/// A registry with three documents, the untitled `0` on screen and `b`, `c`
/// parked behind it.
fn three_documents() -> DocumentRegistry {
    let mut registry = DocumentRegistry::new();
    // Open b: slot 0 (untitled) is parked, b is on screen.
    let slot = registry.new_slot();
    registry.push_active(parked(false), SlotLocalState::default(), slot);
    // Open c: b is parked with its path, c is on screen.
    let slot = registry.new_slot();
    registry.push_active(parked(false), local_with_path("/work/b.op"), slot);
    registry.set_active_path(Some(PathBuf::from("/work/c.op")));
    // Show slot 0 again: c is parked with its path, slot 0's local comes back.
    registry.finish_switch(0, parked(false), local_with_path("/work/c.op"));
    registry
}

#[test]
fn ids_are_unique_and_never_reused_after_a_close() {
    let mut registry = three_documents();
    let closed = registry.id_at(1).unwrap();
    registry.remove(1).expect("a parked slot can be removed");
    let fresh = registry.new_slot();
    assert_ne!(fresh.id, closed);
    assert!(
        registry.index_of(closed).is_none(),
        "a stale id finds no tab"
    );
}

#[test]
fn the_active_slot_cannot_be_removed() {
    let mut registry = three_documents();
    assert!(registry.remove(registry.active_index()).is_none());
    assert_eq!(registry.len(), 3);
}

#[test]
fn removing_an_earlier_slot_keeps_the_same_document_active() {
    let mut registry = three_documents();
    registry.finish_switch(2, parked(false), SlotLocalState::default());
    let active = registry.active_id();
    registry.remove(0).unwrap();
    assert_eq!(registry.active_id(), active);
    assert_eq!(registry.active_index(), 1);
}

#[test]
fn the_same_canonical_path_finds_the_open_tab() {
    let dir = std::env::temp_dir().join(format!("op-registry-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("sub")).unwrap();
    let file = dir.join("poster.op");
    std::fs::write(&file, b"{}").unwrap();

    let mut registry = DocumentRegistry::new();
    registry.set_active_path(Some(file.clone()));
    let roundabout = dir.join("sub").join("..").join("poster.op");
    assert_eq!(registry.index_of_path(&roundabout), Some(0));
    assert_eq!(registry.index_of_path(&dir.join("other.op")), None);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn dirty_comes_from_the_parked_document_or_the_live_flag() {
    let mut registry = DocumentRegistry::new();
    let slot = registry.new_slot();
    registry.push_active(parked(true), SlotLocalState::default(), slot);
    // Slot 0 is parked with edits; slot 1 is on screen and clean.
    assert_eq!(registry.dirty_indices(false), vec![0]);
    assert_eq!(registry.dirty_indices(true), vec![0, 1]);
}

#[test]
fn a_save_ack_is_routed_by_epoch() {
    let mut registry = DocumentRegistry::new();
    let first = parked(false);
    let first_epoch = first.epoch();
    let slot = registry.new_slot();
    registry.push_active(first, SlotLocalState::default(), slot);
    let live_epoch = first_epoch + 1_000;

    assert_eq!(
        registry.owner_of_epoch(live_epoch, live_epoch),
        EpochOwner::Active
    );
    assert_eq!(
        registry.owner_of_epoch(first_epoch, live_epoch),
        EpochOwner::Parked(0)
    );
    assert_eq!(registry.owner_of_epoch(7, live_epoch), EpochOwner::Unknown);
}

#[test]
fn tab_rows_show_file_name_project_and_active_flag() {
    let mut registry = DocumentRegistry::new();
    let slot = registry.new_slot();
    registry.push_active(parked(false), local_with_path("/work/brand/logo.op"), slot);

    let rows = registry.tab_views(false, "Untitled");
    assert_eq!(rows[0].label, "logo.op");
    assert_eq!(rows[0].project, "brand");
    assert!(!rows[0].active);
    assert_eq!(rows[1].label, "Untitled");
    assert_eq!(rows[1].project, "");
    assert!(rows[1].active);
}

#[test]
fn only_documents_bound_to_a_file_are_restored_next_launch() {
    let registry = three_documents();
    let session = registry.session_documents();
    let paths: Vec<_> = session.iter().map(|d| d.path.as_str()).collect();
    assert_eq!(paths, vec!["/work/b.op", "/work/c.op"]);
    assert!(session.iter().all(|d| !d.active), "slot 0 is untitled");
}

#[test]
fn no_swap_while_run_bound() {
    assert_eq!(ActivityFlags::default().blocked_by(), None);
    let cases = [
        (
            ActivityFlags {
                chat_turn: true,
                ..Default::default()
            },
            BlockReason::ChatTurn,
        ),
        (
            ActivityFlags {
                design_run: true,
                ..Default::default()
            },
            BlockReason::DesignRun,
        ),
        (
            ActivityFlags {
                sub_agents: true,
                ..Default::default()
            },
            BlockReason::SubAgents,
        ),
        (
            ActivityFlags {
                codegen: true,
                ..Default::default()
            },
            BlockReason::Codegen,
        ),
        (
            ActivityFlags {
                design_md: true,
                ..Default::default()
            },
            BlockReason::DesignMd,
        ),
        (
            ActivityFlags {
                import: true,
                ..Default::default()
            },
            BlockReason::Import,
        ),
        (
            ActivityFlags {
                git_job: true,
                ..Default::default()
            },
            BlockReason::GitJob,
        ),
        (
            ActivityFlags {
                background_save: true,
                ..Default::default()
            },
            BlockReason::BackgroundSave,
        ),
        (
            ActivityFlags {
                collaboration: true,
                ..Default::default()
            },
            BlockReason::Collaboration,
        ),
    ];
    for (flags, expected) in cases {
        assert_eq!(flags.blocked_by(), Some(expected));
    }
}
