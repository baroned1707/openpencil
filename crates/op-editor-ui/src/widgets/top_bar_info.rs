//! The info bar: what the TopBar's middle slot says about the active file.
//!
//! It replaced the document tab strip. The Projects dock lists and switches
//! files; this bar describes the one on screen:
//!
//! ```text
//! project › folder › file.op   — Edited   ⎇ main
//! ```
//!
//! Layout is a pure function of the slot, the file info and one text metric,
//! so paint, hit-test, the hover tooltip anchor and the Git popover anchor all
//! read the same rects. The slot between the Home button and the agent chip is
//! narrow on a small window (about 70 px at 640 px, more than 700 px at
//! 1440 px), so the bar is a ladder of rungs: it tries the richest rung first
//! and takes the first one that fits (`Item` / `DROP_LADDER` below).
//!
//! # Drop ladder (rung 0 is the richest)
//!
//! | step | what goes                                              |
//! |------|--------------------------------------------------------|
//! | 1    | the folder                                             |
//! | 2    | the project                                            |
//! | 3    | the save label (becomes a dot, or nothing when saved)  |
//! | 4    | the branch name beside the Git icon                    |
//! | 5    | the page name (the chip keeps "2/5")                   |
//! | 6    | the text of the other-unsaved chip (it keeps its count) |
//! | 7    | the text of the agent item (it keeps its dot)          |
//! | 8    | the page chip                                          |
//! | 9    | the other-unsaved chip                                 |
//! | 10   | the Git button                                         |
//! | then | the name is middle-elided to whatever room is left     |
//!
//! The file name and the unsaved mark never drop, and the agent dot stays as
//! long as the shortened name keeps at least `NAME_FLOOR` px beside it: on a
//! window so small that the slot is about 60 px, the name outranks the dot. The
//! Git button, while it is shown, sits right of the other items, never under
//! them.

use crate::widgets::top_bar::*;
use crate::widgets::top_bar_info_text::{elide_middle, SEPARATOR};
use crate::widgets::top_bar_title::{elide_filename_to_width, TopBarTitleLayout, GIT_GAP};
use crate::{Point2D, Rect};
use op_editor_core::SaveState;

/// Space between two items of the bar.
pub(super) const ITEM_GAP: f32 = 6.0;
/// Inner padding of a breadcrumb chip.
pub(super) const CHIP_PAD_X: f32 = 6.0;
/// Room each side of a breadcrumb separator.
const SEPARATOR_GAP: f32 = 4.0;
/// The widest a file name is allowed to get, however wide the window is.
const NAME_MAX_WIDTH: f32 = 360.0;
/// The widest a project or folder part is allowed to get.
const PART_MAX_WIDTH: f32 = 140.0;
pub(super) const NAME_FONT: f32 = 13.0;
pub(super) const META_FONT: f32 = 11.0;
/// Diameter of the unsaved dot an elided bar keeps.
pub(super) const UNSAVED_DOT: f32 = 6.0;
/// Diameter of the agent-activity dot and its gap to the text.
pub(super) const AGENT_DOT: f32 = 6.0;
const AGENT_DOT_GAP: f32 = 6.0;
/// The widest a page name is allowed to get in the page chip.
const PAGE_LABEL_MAX_WIDTH: f32 = 120.0;
/// The page chip's chevron and its gap to the label.
pub(super) const PAGE_CHEVRON: f32 = 12.0;
const PAGE_CHEVRON_GAP: f32 = 4.0;
/// The least room the shortened file name must keep; below it the agent dot
/// gives way to the name.
const NAME_FLOOR: f32 = 24.0;
/// The widest the agent text is allowed to get.
const AGENT_LABEL_MAX_WIDTH: f32 = 220.0;

/// One optional thing the bar can give up when the slot is too narrow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Item {
    Folder,
    Project,
    SaveLabel,
    GitLabel,
    PageLabel,
    UnsavedLabel,
    AgentLabel,
    PageChip,
    UnsavedChip,
    GitButton,
}

/// The order things are given up in, first to last (see the module docs).
/// After the last one the file name itself is shortened.
pub(super) const DROP_LADDER: [Item; 10] = [
    Item::Folder,
    Item::Project,
    Item::SaveLabel,
    Item::GitLabel,
    Item::PageLabel,
    Item::UnsavedLabel,
    Item::AgentLabel,
    Item::PageChip,
    Item::UnsavedChip,
    Item::GitButton,
];

/// One step down the ladder: the first `0` entries of [`DROP_LADDER`] are gone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Rung(pub usize);

impl Rung {
    /// The poorest rung: everything optional is gone.
    pub(super) const LAST: Rung = Rung(DROP_LADDER.len());

    fn keeps(self, item: Item) -> bool {
        !DROP_LADDER[..self.0].contains(&item)
    }
}

/// One part of the breadcrumb: a name, or a separator between names.
#[derive(Debug, Clone)]
pub(super) struct BreadcrumbPart {
    pub text: String,
    pub x: f32,
    /// The file name: painted in the foreground colour, the rest is muted.
    pub is_file_name: bool,
}

#[derive(Debug, Clone)]
pub(super) struct BreadcrumbCell {
    pub rect: Rect,
    /// The dock lists the file, so a press reveals it: the cell is a hit target
    /// with a hover wash. Without that it is plain text.
    pub clickable: bool,
    pub parts: Vec<BreadcrumbPart>,
}

/// The save-state marker: a label while there is room, else a dot.
#[derive(Debug, Clone)]
pub(super) struct SaveCell {
    pub rect: Rect,
    pub label: Option<&'static str>,
}

/// A chip with an optional text: the agent item (a dot, then the text) and the
/// other-unsaved chip (the text, or just its count).
#[derive(Debug, Clone)]
pub(super) struct ChipCell {
    pub rect: Rect,
    pub label: Option<String>,
}

/// Where each info item sits. An item that is dropped is `None`.
#[derive(Debug, Clone, Default)]
pub(super) struct InfoLayout {
    pub breadcrumb: Option<BreadcrumbCell>,
    pub save: Option<SaveCell>,
    /// The current page and a chevron: the page menu's button. Only when the
    /// document has more than one page and the host has the dock.
    pub page: Option<ChipCell>,
    /// An agent run in another document: a dot and, while there is room, text.
    pub agent: Option<ChipCell>,
    /// "N other unsaved", or just the count when narrow.
    pub unsaved: Option<ChipCell>,
}

impl InfoLayout {
    /// The hit under `point`.
    pub(super) fn hit(&self, point: Point2D) -> Option<TopBarHit> {
        let over = |rect: Option<Rect>| rect.is_some_and(|rect| rect.contains(point));
        if over(
            self.breadcrumb
                .as_ref()
                .filter(|cell| cell.clickable)
                .map(|cell| cell.rect),
        ) {
            Some(TopBarHit::Breadcrumb)
        } else if over(self.save.as_ref().map(|cell| cell.rect)) {
            Some(TopBarHit::SaveState)
        } else if over(self.page.as_ref().map(|cell| cell.rect)) {
            Some(TopBarHit::PageChip)
        } else if over(self.agent.as_ref().map(|cell| cell.rect)) {
            Some(TopBarHit::AgentActivity)
        } else if over(self.unsaved.as_ref().map(|cell| cell.rect)) {
            Some(TopBarHit::OtherUnsaved)
        } else {
            None
        }
    }
}

/// A finished attempt at one rung.
struct Built {
    info: InfoLayout,
    git_rect: Option<Rect>,
    /// Right edge of the last item.
    end_x: f32,
    /// Painted width of the file name, so the last rung knows what to shave.
    name_width: f32,
}

impl TopBar {
    /// The text of the save marker at full width.
    pub(super) fn save_label(&self) -> &'static str {
        match self.info.save {
            SaveState::Saved => self.label_saved,
            SaveState::Edited => self.label_edited,
            SaveState::Untitled => self.label_not_saved_yet,
            SaveState::Missing => self.label_not_on_disk,
        }
    }

    /// Lay the info items out inside `slot`: the richest rung that fits, and at
    /// the last rung the name shortened to the room that is left.
    pub(super) fn info_layout(
        &self,
        slot: Rect,
        mut measure: impl FnMut(&str, f32) -> f32,
    ) -> TopBarTitleLayout {
        let empty = |slot| TopBarTitleLayout {
            slot,
            git_rect: None,
            info: InfoLayout::default(),
        };
        if slot.size.x <= 0.0 {
            return empty(slot);
        }
        let slot_right = slot.origin.x + slot.size.x;
        for step in 0..Rung::LAST.0 {
            let built = self.build_info(Rung(step), slot, NAME_MAX_WIDTH, true, &mut measure);
            if built.end_x <= slot_right {
                return finish(slot, built);
            }
        }
        let last = Rung::LAST;
        let shave = |keep_agent: bool, measure: &mut dyn FnMut(&str, f32) -> f32| {
            let natural = self.build_info(last, slot, NAME_MAX_WIDTH, keep_agent, measure);
            let name_max = (natural.name_width - (natural.end_x - slot_right)).max(0.0);
            (name_max, keep_agent)
        };
        let measure: &mut dyn FnMut(&str, f32) -> f32 = &mut measure;
        let (mut name_max, mut keep_agent) = shave(true, &mut *measure);
        if name_max < NAME_FLOOR && self.has_agent_item() {
            (name_max, keep_agent) = shave(false, &mut *measure);
        }
        let built = self.build_info(last, slot, name_max, keep_agent, measure);
        if built.end_x > slot_right {
            // Not even the unsaved mark fits: show nothing rather than paint
            // over the chip next door.
            return empty(slot);
        }
        finish(slot, built)
    }

    /// Whether a run in another document gives the bar an agent item.
    fn has_agent_item(&self) -> bool {
        !self.text_running.is_empty()
    }

    fn build_info(
        &self,
        rung: Rung,
        slot: Rect,
        name_max: f32,
        keep_agent: bool,
        measure: &mut dyn FnMut(&str, f32) -> f32,
    ) -> Built {
        let top = slot.origin.y + (slot.size.y - ICON_BUTTON) / 2.0;
        let mut x = slot.origin.x;
        let mut placed_any = false;
        let mut info = InfoLayout::default();
        let mut name_width = 0.0;

        // Breadcrumb: project › folder › file name.
        let mut texts: Vec<(String, f32, bool)> = Vec::new();
        let part_width = |text: &str, size: f32, measure: &mut dyn FnMut(&str, f32) -> f32| {
            let shown = elide_middle(text, PART_MAX_WIDTH, |candidate| measure(candidate, size));
            let width = measure(&shown, size);
            (shown, width)
        };
        let ancestors = [
            (rung.keeps(Item::Project), self.info.project.as_str()),
            (rung.keeps(Item::Folder), self.info.folder.as_str()),
        ];
        for (shown, text) in ancestors {
            if shown && !text.is_empty() {
                let (text, width) = part_width(text, META_FONT, measure);
                if !text.is_empty() {
                    texts.push((text, width, false));
                }
            }
        }
        let name = elide_filename_to_width(&self.info.file_name, name_max, |candidate| {
            measure(candidate, NAME_FONT)
        });
        if !name.is_empty() {
            name_width = measure(&name, NAME_FONT);
            texts.push((name, name_width, true));
        }
        if texts.iter().any(|(_, _, is_file_name)| *is_file_name) {
            let separator_width = measure(SEPARATOR, META_FONT) + SEPARATOR_GAP * 2.0;
            let mut parts = Vec::new();
            let mut cursor = x + CHIP_PAD_X;
            let count = texts.len();
            for (index, (text, width, is_file_name)) in texts.into_iter().enumerate() {
                parts.push(BreadcrumbPart {
                    text,
                    x: cursor,
                    is_file_name,
                });
                cursor += width;
                if index + 1 < count {
                    parts.push(BreadcrumbPart {
                        text: SEPARATOR.to_string(),
                        x: cursor + SEPARATOR_GAP,
                        is_file_name: false,
                    });
                    cursor += separator_width;
                }
            }
            let rect = Rect::xywh(x, top, cursor + CHIP_PAD_X - x, ICON_BUTTON);
            x = rect.origin.x + rect.size.x;
            placed_any = true;
            info.breadcrumb = Some(BreadcrumbCell {
                rect,
                clickable: self.info.file_path.is_some(),
                parts,
            });
        }

        // Save marker: a label while it fits, else a dot (nothing when saved).
        let label = self.save_label();
        let save_width = if rung.keeps(Item::SaveLabel) && !label.is_empty() {
            Some((measure(label, META_FONT), Some(label)))
        } else if self.info.shows_unsaved_mark() {
            Some((UNSAVED_DOT, None))
        } else {
            None
        };
        if let Some((width, label)) = save_width {
            x += if placed_any { ITEM_GAP } else { 0.0 };
            info.save = Some(SaveCell {
                rect: Rect::xywh(x, top, width, ICON_BUTTON),
                label,
            });
            x += width;
            placed_any = true;
        }

        // Page chip: the current page, with a chevron that says "menu".
        if let Some(pages) = self
            .info
            .pages
            .as_ref()
            .filter(|_| rung.keeps(Item::PageChip))
        {
            let label = if rung.keeps(Item::PageLabel) {
                let name = pages.names.get(pages.active).map_or("", String::as_str);
                elide_middle(name, PAGE_LABEL_MAX_WIDTH, |candidate| {
                    measure(candidate, META_FONT)
                })
            } else {
                format!("{}/{}", pages.active + 1, pages.names.len())
            };
            let width =
                CHIP_PAD_X * 2.0 + measure(&label, META_FONT) + PAGE_CHEVRON_GAP + PAGE_CHEVRON;
            x += if placed_any { ITEM_GAP } else { 0.0 };
            info.page = Some(ChipCell {
                rect: Rect::xywh(x, top, width, ICON_BUTTON),
                label: Some(label),
            });
            x += width;
            placed_any = true;
        }

        // Agent activity: a run in another document.
        if keep_agent && self.has_agent_item() {
            let label = rung.keeps(Item::AgentLabel).then(|| {
                elide_middle(&self.text_running, AGENT_LABEL_MAX_WIDTH, |candidate| {
                    measure(candidate, META_FONT)
                })
            });
            let text_width = label
                .as_deref()
                .map_or(0.0, |text| AGENT_DOT_GAP + measure(text, META_FONT));
            let width = CHIP_PAD_X * 2.0 + AGENT_DOT + text_width;
            x += if placed_any { ITEM_GAP } else { 0.0 };
            info.agent = Some(ChipCell {
                rect: Rect::xywh(x, top, width, ICON_BUTTON),
                label: label.filter(|text| !text.is_empty()),
            });
            x += width;
            placed_any = true;
        }

        // Other unsaved documents: the count, with words while there is room.
        if !self.text_other_unsaved.is_empty() && rung.keeps(Item::UnsavedChip) {
            let label = if rung.keeps(Item::UnsavedLabel) {
                self.text_other_unsaved.clone()
            } else {
                self.info.other_unsaved.to_string()
            };
            let width = CHIP_PAD_X * 2.0 + measure(&label, META_FONT);
            x += if placed_any { ITEM_GAP } else { 0.0 };
            info.unsaved = Some(ChipCell {
                rect: Rect::xywh(x, top, width, ICON_BUTTON),
                label: Some(label),
            });
            x += width;
            placed_any = true;
        }

        // Git button: a fixed target right of everything else.
        let mut git_rect = None;
        if GIT_BUTTON_AVAILABLE && rung.keeps(Item::GitButton) {
            let branch_width = self
                .info
                .git_branch
                .as_deref()
                .filter(|_| rung.keeps(Item::GitLabel))
                .map(|branch| GIT_GAP - 4.0 + measure(branch, META_FONT))
                .unwrap_or(0.0);
            let width = GIT_BUTTON_PAD_X * 2.0 + ICON_SIZE + branch_width;
            x += if placed_any { GIT_GAP } else { 0.0 };
            git_rect = Some(Rect::xywh(x, top, width, ICON_BUTTON));
            x += width;
        }

        Built {
            info,
            git_rect,
            end_x: x,
            name_width,
        }
    }
}

fn finish(slot: Rect, built: Built) -> TopBarTitleLayout {
    TopBarTitleLayout {
        slot,
        git_rect: built.git_rect,
        info: built.info,
    }
}

#[cfg(test)]
#[path = "top_bar_info_chips_tests.rs"]
mod chips_tests;
#[cfg(test)]
#[path = "top_bar_info_tests.rs"]
mod tests;
