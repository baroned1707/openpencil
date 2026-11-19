//! Write-time repair of misspelled `iconFontName` values.
//!
//! A glyph name the canvas cannot paint draws nothing, and the model that wrote
//! it only learns so from `iconIssues`. When ONE name is clearly what was meant
//! (the same name in another case or separator style, or a single typo away
//! from exactly one glyph), this module writes that name instead and reports
//! the change as `renamed: from -> to` so the model learns the spelling.
//!
//! Boundaries, all deliberate:
//! - only `icon_font` nodes, only `iconFontName`; the family (`iconFontFamily`)
//!   is never touched, and a family the editor has no glyph list for (a brand
//!   set, an unknown family) is never renamed;
//! - only a name the painter cannot resolve (the same predicate
//!   `icon_names::unknown_glyph_issue` flags): an alias that paints today, such
//!   as `check-circle`, is left exactly as written;
//! - only nodes this write introduced or changed ([`IconNameSnapshot`] is taken
//!   before the write), so an old icon is never rewritten behind a later,
//!   unrelated write;
//! - anything ambiguous, too short to trust, or unmatched is kept and reported
//!   as before.

use std::cell::RefCell;
use std::collections::HashMap;

use jian_ops_schema::node::PenNode;
use op_editor_core::pen_node_ext::PenNodeExt;
use op_editor_core::EditorState;

use super::icon_names::{painter_knows_glyph, rename_target};

/// `iconFontName` of every `icon_font` node on the active page, by node id,
/// taken before a write.
#[derive(Debug, Default, Clone)]
pub struct IconNameSnapshot(HashMap<String, String>);

impl IconNameSnapshot {
    pub fn capture(state: &EditorState) -> Self {
        let mut names = HashMap::new();
        fn walk(nodes: &[PenNode], names: &mut HashMap<String, String>) {
            for node in nodes {
                if let PenNode::IconFont(icon) = node {
                    names.insert(icon.base.id.clone(), icon.icon_font_name.clone());
                }
                if let Some(children) = node.children() {
                    walk(children, names);
                }
            }
        }
        walk(state.active_children(), &mut names);
        Self(names)
    }

    /// Whether `id` is an icon this write added or whose name it changed.
    fn is_new_or_changed(&self, id: &str, name: &str) -> bool {
        self.0.get(id).is_none_or(|before| before != name)
    }
}

/// One glyph name that was rewritten.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IconRename {
    pub node_id: String,
    pub from: String,
    pub to: String,
}

impl IconRename {
    /// The `iconIssues` line telling the model what changed.
    pub fn note(&self) -> String {
        format!(
            "renamed: {} -> {} (icon {})",
            self.from, self.to, self.node_id
        )
    }
}

/// How many rewrites the thread-local log keeps for one write.
const MAX_RECORDED: usize = 64;

thread_local! {
    /// Rewrites made on this thread while a [`RenameRecording`] is alive.
    static RECORDED: RefCell<Option<Vec<IconRename>>> = const { RefCell::new(None) };
}

/// While alive, [`rename_unknown_icons`] on this thread also logs what it
/// renamed, so the MCP write path can report a rewrite made earlier in the same
/// call (headless transport: inside the applier, before the diagnostics run).
/// Threads without a recording (the editor's UI thread) log nothing.
pub struct RenameRecording(());

impl RenameRecording {
    pub fn start() -> Self {
        RECORDED.with(|cell| *cell.borrow_mut() = Some(Vec::new()));
        Self(())
    }

    /// The rewrites logged so far; the log restarts empty.
    pub fn take(&self) -> Vec<IconRename> {
        RECORDED.with(|cell| {
            cell.borrow_mut()
                .as_mut()
                .map(std::mem::take)
                .unwrap_or_default()
        })
    }
}

impl Drop for RenameRecording {
    fn drop(&mut self) {
        RECORDED.with(|cell| *cell.borrow_mut() = None);
    }
}

/// Rewrite the unknown glyph names of icons added or changed since `before` on
/// the active page, where exactly one glyph is clearly meant. Returns what was
/// rewritten, in document order. Mutates `iconFontName` only.
pub fn rename_unknown_icons(state: &mut EditorState, before: &IconNameSnapshot) -> Vec<IconRename> {
    let mut renames = Vec::new();
    fn walk(nodes: &mut [PenNode], before: &IconNameSnapshot, renames: &mut Vec<IconRename>) {
        for node in nodes {
            if let PenNode::IconFont(icon) = node {
                let name = icon.icon_font_name.clone();
                if before.is_new_or_changed(&icon.base.id, &name)
                    && !painter_knows_glyph(icon.icon_font_family.as_deref(), &name)
                {
                    if let Some(target) = rename_target(icon.icon_font_family.as_deref(), &name) {
                        icon.icon_font_name = target.clone();
                        renames.push(IconRename {
                            node_id: icon.base.id.clone(),
                            from: name,
                            to: target,
                        });
                    }
                }
            }
            if let Some(children) = node.children_mut() {
                walk(children, before, renames);
            }
        }
    }
    walk(state.active_children_mut(), before, &mut renames);
    if !renames.is_empty() {
        RECORDED.with(|cell| {
            if let Some(log) = cell.borrow_mut().as_mut() {
                let room = MAX_RECORDED.saturating_sub(log.len());
                log.extend(renames.iter().take(room).cloned());
            }
        });
    }
    renames
}
