//! Host pump for the Settings > Images image-search "Test" button.
//!
//! Same session pattern as `image_gen_test_host.rs`: the settings press
//! raises `agent_settings.pending_image_search_test` (the request seam in
//! op-editor-core) after setting the status to Testing, the redraw pump
//! drains it onto a worker thread that runs one tiny real query through the
//! first configured provider, and a later frame lands Valid / Invalid in
//! `images_search_test_status`.

use std::sync::mpsc::{self, Receiver, TryRecvError};

use op_editor_core::agent_settings::{AgentSettings, ImageTestStatus};

use crate::image_search_session::OpenverseCredentials;
use crate::DesktopApp;

/// One in-flight image-search probe.
pub(crate) struct ImageSearchTestJob {
    rx: Receiver<ImageTestStatus>,
}

impl ImageSearchTestJob {
    /// Run `probe` on a worker thread (the production probe dials the first
    /// provider; tests pass a scripted closure).
    fn spawn_with(probe: impl FnOnce() -> ImageTestStatus + Send + 'static) -> Self {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(probe());
        });
        Self { rx }
    }

    fn spawn(credentials: Option<OpenverseCredentials>) -> Self {
        Self::spawn_with(move || {
            op_host_services::chat_runtime::block_on_anywhere(async move {
                let Ok(client) = reqwest::Client::builder()
                    .use_rustls_tls()
                    .timeout(std::time::Duration::from_secs(20))
                    .user_agent(op_image_enrich::HTTP_USER_AGENT)
                    .build()
                else {
                    return ImageTestStatus::Invalid;
                };
                op_image_enrich::net::providers::test_first_provider(
                    &client,
                    &op_image_enrich::net::providers::default_providers(),
                    credentials.as_ref().map(OpenverseCredentials::as_web),
                )
                .await
            })
        })
    }

    fn poll(&self) -> Option<ImageTestStatus> {
        match self.rx.try_recv() {
            Ok(status) => Some(status),
            Err(TryRecvError::Empty) => None,
            // The worker died without an answer: report failure, never hang
            // the button in Testing.
            Err(TryRecvError::Disconnected) => Some(ImageTestStatus::Invalid),
        }
    }
}

/// Land a finished probe. Only a button still in its Testing phase accepts
/// the outcome (the user may have edited the credentials or pressed again).
fn land_status(settings: &mut AgentSettings, status: ImageTestStatus) -> bool {
    if settings.images_search_test_status == ImageTestStatus::Testing {
        settings.images_search_test_status = status;
        true
    } else {
        false
    }
}

impl DesktopApp {
    /// Pump: land a finished probe, then start the next requested one.
    /// Returns true when state changed.
    pub(crate) fn drain_image_search_test(&mut self) -> bool {
        let mut changed = false;
        if let Some(job) = self.image_search_test_job.as_ref() {
            if let Some(status) = job.poll() {
                self.image_search_test_job = None;
                let settings = &mut self.host.editor_state_mut().editor_ui.agent_settings;
                changed |= land_status(settings, status);
            }
        }
        if self.image_search_test_job.is_some() {
            return changed;
        }
        let state = self.host.editor_state_mut();
        if std::mem::take(&mut state.editor_ui.agent_settings.pending_image_search_test) {
            let credentials = OpenverseCredentials::from_state(state);
            self.image_search_test_job = Some(ImageSearchTestJob::spawn(credentials));
            changed = true;
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn wait_for(job: &ImageSearchTestJob) -> ImageTestStatus {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(status) = job.poll() {
                return status;
            }
            assert!(Instant::now() < deadline, "probe never answered");
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    fn testing_settings() -> AgentSettings {
        AgentSettings {
            images_search_test_status: ImageTestStatus::Testing,
            ..AgentSettings::default()
        }
    }

    #[test]
    fn search_test_button_resolves_valid_and_invalid() {
        for scripted in [ImageTestStatus::Valid, ImageTestStatus::Invalid] {
            let job = ImageSearchTestJob::spawn_with(move || scripted);
            let mut settings = testing_settings();
            assert!(land_status(&mut settings, wait_for(&job)));
            assert_eq!(settings.images_search_test_status, scripted);
        }
    }

    #[test]
    fn a_vanished_worker_reads_invalid_not_testing_forever() {
        let job = ImageSearchTestJob::spawn_with(|| panic!("probe worker crashed"));
        assert_eq!(wait_for(&job), ImageTestStatus::Invalid);
    }

    #[test]
    fn a_stale_outcome_never_overwrites_a_button_that_left_testing() {
        let mut settings = AgentSettings {
            images_search_test_status: ImageTestStatus::Idle,
            ..AgentSettings::default()
        };
        assert!(!land_status(&mut settings, ImageTestStatus::Valid));
        assert_eq!(settings.images_search_test_status, ImageTestStatus::Idle);
    }
}
