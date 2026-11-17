//! Font-family feedback for design writes.
//!
//! A text node whose `fontFamily` is not installed silently renders in a
//! default font, and nothing in the document says so. [`scan_font_issues`]
//! names each such family once, and says what the editor will do about it:
//! download it from Google Fonts (when it is in the catalog) or fall back.
//!
//! What counts as "installed" is a [`FontKnowledge`], built from the editor
//! state the write ran against:
//! - the 16 bundled design fonts, always;
//! - imported fonts, always;
//! - system fonts, only when the host finished enumerating them
//!   (`system_fonts_loaded`, the live editor). A headless `--mcp` process never
//!   enumerates them, so there the answer for a non-bundled family is unknown
//!   and only names found in NO list at all are reported, worded as such.
//!
//! The scan reuses the editor's own font-usage walk (`FontPlan`, component
//! override stacks) and stack rules (a stack is satisfied by any member that
//! resolves; the primary concrete family is the one named), so it agrees with
//! the missing-fonts prompt.

use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use jian_ops_schema::font_plan::FontPlan;
use jian_ops_schema::node::PenNode;
use op_editor_core::font_catalog::{
    is_generic_or_system_font_alias, primary_concrete_font_family, split_font_family_stack,
};
use op_editor_core::missing_fonts::component_override_font_family_stacks_in;
use op_editor_core::EditorState;
use op_editor_ui::font_catalog::FALLBACK_SYSTEM_FONTS;
use op_font_fetch::Catalog;

use super::icon_names::edit_distance;

/// Design fonts shipped inside the renderer (`op-host-native`
/// `bundled_design_fonts.rs`, all SIL OFL). op-chat-agent cannot depend on the
/// native host, so the list is repeated here; the live editor additionally
/// reports its registered set through `editor_ui.bundled_font_families`.
const BUNDLED_DESIGN_FAMILIES: [&str; 16] = [
    "Roboto",
    "Inter",
    "Noto Sans SC",
    "Plus Jakarta Sans",
    "Archivo",
    "Libre Caslon Text",
    "Noto Serif SC",
    "Space Grotesk",
    "Manrope",
    "Outfit",
    "DM Sans",
    "DM Serif Display",
    "DM Mono",
    "Instrument Serif",
    "JetBrains Mono",
    "Cormorant Garamond",
];

/// At most this many families are reported per write.
const MAX_FONT_ISSUES: usize = 12;
/// How many nearest family names a line offers.
const SUGGESTION_COUNT: usize = 3;
/// A family that starts with (or is a prefix of) the missing name is never
/// ranked worse than this edit distance: `Sora Display` should offer `Sora`.
const RELATED_DISTANCE: usize = 2;
/// A family name longer than this is cut in the message (documents are
/// untrusted text).
const MAX_DISPLAY_CHARS: usize = 60;

/// Which font families exist for the editor this write ran against.
pub(super) struct FontKnowledge {
    /// Lowercased names that resolve: bundled, imported and (when known)
    /// system fonts.
    installed: HashSet<String>,
    /// Whether the host enumerated the system fonts.
    system_known: bool,
    /// The cached Google Fonts catalog; `None` when absent or empty.
    catalog: Option<Arc<Catalog>>,
}

fn key(family: &str) -> String {
    family.trim().to_ascii_lowercase()
}

impl FontKnowledge {
    /// Knowledge for the live editor state, or a headless one when the state
    /// carries no system-font snapshot.
    pub(super) fn from_state(state: &EditorState) -> Self {
        let ui = &state.editor_ui;
        let system_known = ui.system_fonts_loaded;
        let mut extra: Vec<&str> = Vec::new();
        extra.extend(ui.bundled_font_families.iter().map(String::as_str));
        extra.extend(ui.imported_font_families.iter().map(String::as_str));
        if system_known {
            extra.extend(ui.system_font_families.iter().map(String::as_str));
        }
        Self::new(
            extra,
            system_known,
            super::font_catalog_file::cached_catalog(),
        )
    }

    /// `extra_installed` joins the bundled set. When the system fonts are not
    /// known, the few ubiquitous ones the font picker lists as its stand-in
    /// count as installed so the headless path does not flag `Arial`.
    pub(super) fn new<'a>(
        extra_installed: impl IntoIterator<Item = &'a str>,
        system_known: bool,
        catalog: Option<Arc<Catalog>>,
    ) -> Self {
        let mut installed: HashSet<String> =
            BUNDLED_DESIGN_FAMILIES.iter().map(|f| key(f)).collect();
        installed.extend(extra_installed.into_iter().map(key));
        if !system_known {
            installed.extend(FALLBACK_SYSTEM_FONTS.iter().map(|f| key(f)));
        }
        Self {
            installed,
            system_known,
            catalog: catalog.filter(|catalog| !catalog.is_empty()),
        }
    }

    /// With no system fonts and no catalog, no family can be called unknown.
    fn can_judge(&self) -> bool {
        self.system_known || self.catalog.is_some()
    }

    /// A stack is satisfied by any generic alias or installed member.
    fn stack_resolves(&self, stack: &str) -> bool {
        split_font_family_stack(stack).iter().any(|family| {
            is_generic_or_system_font_alias(family) || self.installed.contains(&key(family))
        })
    }

    /// Up to [`SUGGESTION_COUNT`] bundled / catalog family names nearest to
    /// `family`, best first. Deterministic: ties break on the shorter name,
    /// then alphabetically.
    fn closest(&self, family: &str) -> Vec<String> {
        let query = key(family);
        let catalog_names = self.catalog.iter().flat_map(|catalog| catalog.families());
        let mut seen = HashSet::new();
        let mut ranked: Vec<(usize, usize, &str)> = BUNDLED_DESIGN_FAMILIES
            .iter()
            .copied()
            .chain(catalog_names)
            .filter_map(|candidate| {
                let lowered = key(candidate);
                if lowered == query || !seen.insert(lowered.clone()) {
                    return None;
                }
                Some((name_distance(&query, &lowered), candidate.len(), candidate))
            })
            .collect();
        ranked.sort_unstable();
        ranked
            .into_iter()
            .take(SUGGESTION_COUNT)
            .map(|(_, _, name)| name.to_string())
            .collect()
    }

    /// The feedback line for one unresolved family, `None` when this
    /// knowledge cannot honestly call it a problem.
    fn issue_line(&self, family: &str) -> Option<String> {
        let name = display_name(family);
        let entry = self
            .catalog
            .as_ref()
            .and_then(|catalog| catalog.lookup(family));
        if !self.system_known && entry.is_some() {
            // It may well be installed; the system fonts were not checked.
            return None;
        }
        if self.system_known && entry.is_some_and(|entry| entry.license.is_some()) {
            return Some(format!(
                "font '{name}': not installed; the editor will download it from Google Fonts \
                 when auto-download is on (otherwise it falls back to a default)"
            ));
        }
        let closest = self.closest(family);
        let closest = if closest.is_empty() {
            String::new()
        } else {
            format!("; closest: {}", closest.join(", "))
        };
        Some(match (self.system_known, entry, self.catalog.is_some()) {
            (true, Some(_), _) => format!(
                "font '{name}': not installed and its licence does not allow automatic \
                 download{closest} (the editor falls back to a default)"
            ),
            (true, None, true) => format!(
                "font '{name}': not installed and not in the Google Fonts catalog{closest} \
                 (the editor falls back to a default)"
            ),
            (true, None, false) => format!(
                "font '{name}': not installed{closest} (the editor falls back to a default)"
            ),
            (false, _, _) => format!(
                "font '{name}': unknown family (not bundled, not in the Google Fonts catalog; \
                 system fonts not checked){closest}"
            ),
        })
    }
}

fn name_distance(query: &str, candidate: &str) -> usize {
    let edit = edit_distance(query, candidate);
    let related =
        !query.is_empty() && (candidate.starts_with(query) || query.starts_with(candidate));
    if related {
        edit.min(RELATED_DISTANCE)
    } else {
        edit
    }
}

/// The family as shown in a message: no control characters, bounded length.
fn display_name(family: &str) -> String {
    family
        .chars()
        .filter(|ch| !ch.is_control())
        .take(MAX_DISPLAY_CHARS)
        .collect()
}

/// A stack the document leaves to something else (a style variable, an
/// inherited value): the family is not known here, so it is never flagged.
fn is_deferred(stack: &str) -> bool {
    split_font_family_stack(stack).iter().any(|family| {
        family.starts_with('$')
            || matches!(
                family.to_ascii_lowercase().as_str(),
                "inherit" | "initial" | "unset" | "revert"
            )
    })
}

/// One line per distinct unresolved family used by the text under `nodes`.
pub(super) fn scan_font_issues(nodes: &[PenNode], known: &FontKnowledge) -> Vec<String> {
    if !known.can_judge() {
        return Vec::new();
    }
    let plan = FontPlan::scan_subtrees(nodes);
    let mut stacks: Vec<String> = plan
        .families()
        .map(|(stack, _)| stack.to_string())
        .collect();
    stacks.extend(component_override_font_family_stacks_in(nodes));

    // Keyed by lowercase name so `sora` and `Sora` are one family; a sorted
    // map makes the line order deterministic.
    let mut unresolved: BTreeMap<String, String> = BTreeMap::new();
    for stack in &stacks {
        let stack = stack.trim();
        if stack.is_empty() || is_deferred(stack) || known.stack_resolves(stack) {
            continue;
        }
        if let Some(family) = primary_concrete_font_family(stack) {
            unresolved.entry(key(&family)).or_insert(family);
        }
    }
    unresolved
        .values()
        .filter_map(|family| known.issue_line(family))
        .take(MAX_FONT_ISSUES)
        .collect()
}
