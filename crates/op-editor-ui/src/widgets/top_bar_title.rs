//! Bounded slot of the info bar and the file-name elision helpers.
//!
//! The info bar lives in the center slot between the Home button and the
//! agent chip. What goes into the slot, and what is dropped when it is too
//! narrow, is decided in `top_bar_info`; this file computes the slot itself
//! and shortens a file name to a width, keeping its extension.

use crate::widgets::top_bar::*;
use crate::widgets::top_bar_info::InfoLayout;
use crate::{Point2D, Rect};

pub(super) const TITLE_SIDE_GAP: f32 = 8.0;
pub(super) const GIT_GAP: f32 = 10.0;

#[derive(Debug, Clone)]
pub(super) struct TopBarTitleLayout {
    /// The room between the Home button and the agent chip.
    pub slot: Rect,
    /// The Git button, when it is shown.
    pub git_rect: Option<Rect>,
    /// Everything else the bar says about the active file.
    pub info: InfoLayout,
}

impl TopBar {
    /// Compute the title layout using one caller-supplied text metric.
    ///
    /// Paint supplies exact backend metrics; native hit-test and popup
    /// anchoring pass the same family-aware metric, while backend-less callers
    /// use a conservative fallback. The slot never reaches under the Home
    /// button or the agent chip, so nothing the bar paints can either.
    pub(super) fn title_layout(
        &self,
        top_bar_rect: Rect,
        measure: impl FnMut(&str, f32) -> f32,
    ) -> TopBarTitleLayout {
        let mut measure = measure;
        let chip_text = self.chip_text();
        let chip_text_w = measure(&chip_text, 11.0);
        let chip_rect = self.agent_chip_rect(top_bar_rect, chip_text_w);
        let home_rect = self.home_button_rect(top_bar_rect);
        let slot_left = home_rect.origin.x + home_rect.size.x + TITLE_SIDE_GAP;
        let slot_right = chip_rect.origin.x - TITLE_SIDE_GAP;
        let slot_width = (slot_right - slot_left).max(0.0);
        let slot = Rect {
            origin: Point2D::new(slot_left, top_bar_rect.origin.y),
            size: Point2D::new(slot_width, top_bar_rect.size.y),
        };
        self.info_layout(slot, measure)
    }
}

/// Middle-elide a basename while retaining its final extension when possible.
pub(super) fn elide_filename_to_width(
    file_name: &str,
    max_width: f32,
    mut measure: impl FnMut(&str) -> f32,
) -> String {
    if file_name.is_empty() || max_width <= 0.0 {
        return String::new();
    }
    if measure(file_name) <= max_width {
        return file_name.to_string();
    }

    const ELLIPSIS: char = '…';
    let chars: Vec<char> = file_name.chars().collect();
    let extension_start = chars
        .iter()
        .rposition(|&c| c == '.')
        .filter(|&index| index > 0 && index + 1 < chars.len());

    if let Some(extension_start) = extension_start {
        let suffix: String = chars[extension_start..].iter().collect();
        let minimum = format!("{ELLIPSIS}{suffix}");
        if measure(&minimum) <= max_width {
            return widest_candidate(0, extension_start, max_width, &mut measure, |count| {
                let prefix: String = chars[..count].iter().collect();
                format!("{prefix}{ELLIPSIS}{suffix}")
            });
        }
    }

    let ellipsis = ELLIPSIS.to_string();
    if measure(&ellipsis) > max_width {
        return String::new();
    }
    widest_candidate(0, chars.len(), max_width, &mut measure, |count| {
        let prefix: String = chars[..count].iter().collect();
        format!("{prefix}{ELLIPSIS}")
    })
}

fn widest_candidate(
    mut low: usize,
    mut high: usize,
    max_width: f32,
    measure: &mut impl FnMut(&str) -> f32,
    candidate: impl Fn(usize) -> String,
) -> String {
    while low < high {
        let mid = low + (high - low).div_ceil(2);
        let text = candidate(mid);
        if measure(&text) <= max_width {
            low = mid;
        } else {
            high = mid - 1;
        }
    }
    candidate(low)
}
