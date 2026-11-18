//! Where a stock image came from, and under which licence.
//!
//! Recorded when a search result lands on a node (`apply_result_with_provenance`)
//! so the design can later credit the creator. OpenPencil-owned: the canonical
//! `.op` schema (jian) has no field for it yet, so the record lives in the
//! per-document side table `EditorState::image_provenance` and is NOT part of
//! the saved file (see `apply_result_with_provenance`).

pub use op_editor_core::ImageProvenance;
