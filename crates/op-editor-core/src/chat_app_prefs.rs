//! The app-level part of a chat tab.
//!
//! A chat belongs to one open document: its tabs, transcripts, drafts and
//! pending sends travel with the document (`DocumentScope::chat`). A few
//! fields of a tab are not about the conversation but about the user and the
//! app: which models are connected and selected, how many agents the next
//! design turn uses, and where the floating panel sits. Those must follow the
//! user from document to document, so [`ChatAppPrefs`] lifts them out of the
//! outgoing document's chat and writes them into every tab of the incoming
//! one.
//!
//! [`ChatAppPrefs::from_tab`] and [`ChatAppPrefs::apply_to_tab`] destructure
//! `ChatState` with no `..` rest pattern: a new `ChatState` field does not
//! compile until it is classified as app-level (carry it) or per-conversation
//! (`name: _`).

use crate::chat::{ChatAnchor, ChatState, ModelEntry};
use crate::chat_sessions::ChatSessions;

/// What follows the user across documents. See the module docs.
#[derive(Debug, Clone, PartialEq)]
pub struct ChatAppPrefs {
    discovered_models: Vec<ModelEntry>,
    available_models: Vec<ModelEntry>,
    selected_model: usize,
    agent_team_size: u32,
    anchor: ChatAnchor,
    panel_width: f32,
    panel_height: f32,
    panel_position: Option<(f32, f32)>,
    collapsed: bool,
    minimized: bool,
    maximized: bool,
}

impl ChatAppPrefs {
    /// Read the app-level fields of one tab.
    fn from_tab(tab: &ChatState) -> Self {
        let ChatState {
            runtime_instance: _,
            messages: _,
            title: _,
            input: _,
            focused: _,
            transcript_selection: _,
            anchor,
            panel_width,
            panel_height,
            panel_position,
            collapsed,
            minimized,
            maximized,
            transcript_scroll: _,
            transcript_pinned: _,
            pending_send: _,
            pending_new_chat: _,
            pending_stop_chat: _,
            pending_copy_text: _,
            launch_route: _,
            discovered_models,
            available_models,
            selected_model,
            // Per tab on purpose: a tab opened from another does not inherit
            // them, and neither does a document.
            thinking_mode: _,
            effort_level: _,
            agent_team_size,
            agents_running: _,
            pending_attachments: _,
            pending_attachment_pick: _,
            pending_subtask_retry: _,
            input_scroll: _,
            input_scroll_caret: _,
            // Per conversation: it describes this tab's last run.
            last_run_usage: _,
        } = tab;
        Self {
            discovered_models: discovered_models.clone(),
            available_models: available_models.clone(),
            selected_model: *selected_model,
            agent_team_size: *agent_team_size,
            anchor: *anchor,
            panel_width: *panel_width,
            panel_height: *panel_height,
            panel_position: *panel_position,
            collapsed: *collapsed,
            minimized: *minimized,
            maximized: *maximized,
        }
    }

    /// Write the app-level fields into one tab.
    fn apply_to_tab(&self, tab: &mut ChatState) {
        let ChatState {
            runtime_instance: _,
            messages: _,
            title: _,
            input: _,
            focused: _,
            transcript_selection: _,
            anchor,
            panel_width,
            panel_height,
            panel_position,
            collapsed,
            minimized,
            maximized,
            transcript_scroll: _,
            transcript_pinned: _,
            pending_send: _,
            pending_new_chat: _,
            pending_stop_chat: _,
            pending_copy_text: _,
            launch_route: _,
            discovered_models,
            available_models,
            selected_model,
            thinking_mode: _,
            effort_level: _,
            agent_team_size,
            agents_running: _,
            pending_attachments: _,
            pending_attachment_pick: _,
            pending_subtask_retry: _,
            input_scroll: _,
            input_scroll_caret: _,
            // Per conversation: it describes this tab's last run.
            last_run_usage: _,
        } = tab;
        discovered_models.clone_from(&self.discovered_models);
        available_models.clone_from(&self.available_models);
        *selected_model = self.selected_model;
        *agent_team_size = self.agent_team_size;
        *anchor = self.anchor;
        *panel_width = self.panel_width;
        *panel_height = self.panel_height;
        *panel_position = self.panel_position;
        *collapsed = self.collapsed;
        *minimized = self.minimized;
        *maximized = self.maximized;
    }
}

impl ChatSessions {
    /// The app-level preferences as the active tab holds them.
    pub fn app_prefs(&self) -> ChatAppPrefs {
        ChatAppPrefs::from_tab(self.active())
    }

    /// Write `prefs` into every tab, so whichever tab the user lands on shows
    /// the models and panel placement they last used.
    pub fn apply_app_prefs(&mut self, prefs: &ChatAppPrefs) {
        for tab in self.tabs_mut() {
            prefs.apply_to_tab(tab);
        }
    }
}
