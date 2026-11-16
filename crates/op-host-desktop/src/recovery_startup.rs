//! Startup recovery: offer the unsaved documents a previous run left behind.
//!
//! At launch (before the session's tabs are reopened) the recovery folder is
//! scanned. Entries written by a process that is still running (another
//! instance, for example a debug build beside the installed app) are left
//! alone. For the rest ONE confirmation lists the documents:
//!
//! * Recover opens each as a new tab that is unsaved. A tab is bound to the
//!   file it came from, so Save writes back to it, unless that file changed on
//!   disk after the snapshot (or is gone): then it opens as an untitled copy,
//!   so recovering can never overwrite newer work. The list says which.
//! * Discard deletes the entries.
//!
//! The dialog's text is in the locale tables (`recovery.*`).

use crate::message_dialog::Choice;
use crate::recovery_liveness::process_is_alive;
use crate::recovery_store::{unix_now, RecoveryEntry, SnapshotReason};
use crate::DesktopApp;
use op_host_native::widget_host::{capture_loaded_thumbnails, ParkedDocument};
use op_host_services::doc_io;
use std::path::PathBuf;

/// How many documents the dialog names before it summarizes the rest.
const LISTED_LABELS: usize = 8;

/// How a recovered document is opened.
struct RecoveryPlan {
    entry: RecoveryEntry,
    /// The file the tab is bound to; `None` opens it untitled.
    bind_to: Option<PathBuf>,
    /// Why it is untitled although it came from a file.
    note: Option<UntitledReason>,
}

/// Why a document that came from a file opens as an untitled copy.
#[derive(Clone, Copy)]
enum UntitledReason {
    /// The file was changed on disk after the snapshot.
    DiskNewer,
    /// The file no longer exists.
    FileGone,
}

impl UntitledReason {
    fn locale_key(self) -> &'static str {
        match self {
            Self::DiskNewer => "recovery.noteNewer",
            Self::FileGone => "recovery.noteGone",
        }
    }
}

impl RecoveryPlan {
    fn new(entry: RecoveryEntry) -> Self {
        let (bind_to, note) = match entry.manifest.original_path.as_deref().map(PathBuf::from) {
            None => (None, None),
            Some(original) if !original.is_file() => (None, Some(UntitledReason::FileGone)),
            Some(original) => {
                // `>=`: file times have one-second resolution, and when in
                // doubt the recovered copy must not be the one that overwrites.
                let newer = file_modified_unix(&original)
                    .is_some_and(|modified| modified >= entry.manifest.saved_at_unix);
                if newer {
                    (None, Some(UntitledReason::DiskNewer))
                } else {
                    (Some(original), None)
                }
            }
        };
        Self {
            entry,
            bind_to,
            note,
        }
    }

    fn line(&self, locale: op_editor_core::Locale) -> String {
        match self.note {
            Some(reason) => format!(
                "{} ({})",
                self.entry.manifest.label,
                op_i18n::translate(locale, reason.locale_key())
            ),
            None => self.entry.manifest.label.clone(),
        }
    }
}

fn file_modified_unix(path: &std::path::Path) -> Option<u64> {
    std::fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|elapsed| elapsed.as_secs())
}

fn dialog_title(plans: &[RecoveryPlan], locale: op_editor_core::Locale) -> String {
    match plans.len() {
        1 => op_i18n::translate(locale, "recovery.titleOne").to_string(),
        count => op_i18n::translate(locale, "recovery.titleMany")
            .replace("{{count}}", &count.to_string()),
    }
}

fn dialog_body(plans: &[RecoveryPlan], locale: op_editor_core::Locale) -> String {
    let mut lines: Vec<String> = plans
        .iter()
        .take(LISTED_LABELS)
        .map(|plan| format!("• {}", plan.line(locale)))
        .collect();
    if plans.len() > LISTED_LABELS {
        let more = op_i18n::translate(locale, "recovery.more")
            .replace("{{count}}", &(plans.len() - LISTED_LABELS).to_string());
        lines.push(format!("• {more}"));
    }
    op_i18n::translate(locale, "recovery.body").replace("{{list}}", &lines.join("\n"))
}

impl DesktopApp {
    /// Offer recovery through the real dialog. Run once at launch.
    pub(crate) fn offer_recovery_at_startup(&mut self) -> usize {
        self.offer_recovery_with(|title, body| {
            crate::message_dialog::ask_yes_no(title, body, rfd::MessageLevel::Info)
        })
    }

    /// [`Self::offer_recovery_at_startup`] with the question asked through
    /// `ask`. Returns how many documents were reopened. A dialog that cannot be
    /// shown (`None`) recovers: unsaved work is never discarded unseen. `Cancel`
    /// leaves the entries for the next launch.
    pub(crate) fn offer_recovery_with(
        &mut self,
        ask: impl FnOnce(&str, &str) -> Option<Choice>,
    ) -> usize {
        let Some(store) = self.recovery.store().cloned() else {
            return 0;
        };
        store.prune(unix_now());
        let report = store.scan(process_is_alive);
        if report.skipped > 0 {
            eprintln!("[recovery] skipped {} unusable entries", report.skipped);
        }
        if report.entries.is_empty() {
            return 0;
        }
        let plans: Vec<RecoveryPlan> = report.entries.into_iter().map(RecoveryPlan::new).collect();
        let locale = self.host.editor_state().editor_ui.locale;
        match ask(&dialog_title(&plans, locale), &dialog_body(&plans, locale)) {
            Some(Choice::No) => {
                for plan in &plans {
                    if let Err(error) = store.remove_entry(&plan.entry) {
                        eprintln!("[recovery] {error}");
                    }
                }
                0
            }
            Some(Choice::Cancel) => 0,
            Some(Choice::Yes) | None => {
                let recovered = plans
                    .iter()
                    .filter(|plan| self.recover_document(plan))
                    .count();
                self.host.mark_editor_state_dirty();
                recovered
            }
        }
    }

    /// Open one snapshot as a new unsaved tab. The old entry is deleted only
    /// after the new tab has its own entry, so a crash right after recovering
    /// loses nothing. An entry that cannot be opened is kept.
    fn recover_document(&mut self, plan: &RecoveryPlan) -> bool {
        let locale = self.host.editor_state().editor_ui.locale;
        let (loaded, thumbs) = capture_loaded_thumbnails(|| {
            doc_io::load_editor_state_with_report(&plan.entry.document_path, locale)
        });
        let mut state = match loaded {
            Ok(loaded) => loaded.state,
            Err(error) => {
                eprintln!(
                    "[recovery] cannot open {}: {error}",
                    plan.entry.document_path.display()
                );
                return false;
            }
        };
        state.editor_ui.home.hide();
        doc_io::set_file_name_display(&mut state, plan.bind_to.as_deref());
        state.clear_selection();
        // Whatever it was bound to, it carries edits the file does not have.
        state.mark_document_changed();
        let replaces_blank = self.active_document_is_pristine_untitled();
        let blank_index = self.documents.active_index();
        let open_in_normal_mode = self.host.home_visible() || self.host.workspace_visible();
        let incoming = ParkedDocument::from_state(state, thumbs);
        if !self.install_new_document_tab(incoming, plan.bind_to.clone()) {
            return false;
        }
        self.host.editor_state_mut().editor_ui.home.hide();
        if open_in_normal_mode {
            crate::persistence::open_loaded_normal_workspace(&mut self.host);
        }
        self.host.arm_missing_fonts_detection();
        crate::persistence::fit_loaded_document(&mut self.host, self.window.as_ref());
        if replaces_blank {
            self.remove_slot(blank_index);
            self.publish_document_tabs();
        }
        let id = self.documents.active_id();
        self.snapshot_document_now(id, SnapshotReason::Autosnapshot);
        if self.recovery.has_entry(id) {
            if let Some(store) = self.recovery.store() {
                if let Err(error) = store.remove_entry(&plan.entry) {
                    eprintln!("[recovery] {error}");
                }
            }
        }
        true
    }
}
