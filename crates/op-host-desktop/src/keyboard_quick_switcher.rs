//! The quick switcher's keys (Cmd+K).
//!
//! While the switcher is open it is modal: it takes Esc, Enter, the arrows,
//! Home / End, Backspace / Delete and Cmd+K (which closes it), and swallows
//! every other chord so none can touch the document behind it. Plain typing,
//! Space and the clipboard chords (Cmd+A / C / X / V) are NOT taken here: they
//! flow on to the ordinary table, which already routes them to whichever text
//! input owns the keyboard (`active_text_input` resolves the switcher's query).
//! Opening it is the ordinary table's Cmd+K arm, gated like the other chrome
//! chords.

use crate::DesktopApp;
use winit::keyboard::{Key, NamedKey};

impl DesktopApp {
    /// Handle `logical_key` for the open quick switcher. `true` when the key is
    /// fully dealt with (the caller redraws and returns); `false` when the
    /// switcher is closed or the key is one that flows on to the ordinary table.
    pub(crate) fn handle_quick_switcher_key(&mut self, logical_key: &Key) -> bool {
        if !self.host.quick_switcher_active() {
            return false;
        }
        let extend = self.shift_modifier;
        match logical_key {
            Key::Named(NamedKey::Escape) => {
                self.host.apply_quick_switcher_cancel();
            }
            Key::Named(NamedKey::Enter) => {
                self.host.apply_quick_switcher_confirm();
            }
            Key::Named(NamedKey::ArrowUp) => {
                self.host.apply_quick_switcher_move(-1);
            }
            Key::Named(NamedKey::ArrowDown) => {
                self.host.apply_quick_switcher_move(1);
            }
            Key::Named(NamedKey::ArrowLeft) | Key::Named(NamedKey::ArrowRight) => {
                let forward = matches!(logical_key, Key::Named(NamedKey::ArrowRight));
                if self.zoom_modifier {
                    self.host.apply_quick_switcher_edge(forward, extend);
                } else {
                    self.host.apply_quick_switcher_caret(forward, extend);
                }
            }
            Key::Named(NamedKey::Home) => {
                self.host.apply_quick_switcher_edge(false, extend);
            }
            Key::Named(NamedKey::End) => {
                self.host.apply_quick_switcher_edge(true, extend);
            }
            Key::Named(NamedKey::Backspace) => {
                self.host.apply_quick_switcher_backspace();
            }
            Key::Named(NamedKey::Delete) => {
                self.host.apply_quick_switcher_delete();
            }
            // Typing and Space flow on; every other named key (Tab, PageUp,
            // function keys...) is the switcher's to swallow.
            Key::Named(NamedKey::Space) => return false,
            Key::Named(_) => {}
            Key::Character(character) if self.zoom_modifier => {
                let plain = !self.shift_modifier && !self.alt_modifier;
                match character.to_lowercase().as_str() {
                    "k" if plain => {
                        self.host.apply_toggle_quick_switcher();
                    }
                    // The clipboard chords reach the query through the table.
                    "a" | "c" | "x" | "v" if plain => return false,
                    _ => {}
                }
            }
            Key::Character(_) => return false,
            _ => {}
        }
        self.request_redraw(true);
        true
    }
}

#[cfg(test)]
#[path = "keyboard_quick_switcher_tests.rs"]
mod tests;
