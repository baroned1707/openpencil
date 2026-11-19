//! The one place the API key is read and used.
//!
//! `ANTHROPIC_API_KEY` is read from the process environment when a real run
//! starts. It is held in memory, handed to the production provider for the
//! request header, and never printed, logged or written: `Debug` is redacted
//! and every text the recorder writes goes through the [`Scrubber`].

use std::sync::Arc;

use op_ai::chat_provider::{ChatDelta, ChatProvider, ChatRequest, ChatToolExecutor, StopReason};
use op_editor_core::{BuiltinAgentConfig, BuiltinAgentKind, BuiltinAgentPresetKey};
use op_host_services::chat_builtin_http::ConfiguredBuiltinProvider;
use op_host_services::design_agent_tools::design_tool_defs;

use crate::real_run::real_executor::RunTransport;

const KEY_VARIABLE: &str = "ANTHROPIC_API_KEY";

pub(crate) struct AnthropicTransport {
    api_key: String,
    model: String,
}

impl std::fmt::Debug for AnthropicTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnthropicTransport")
            .field("api_key", &"<redacted>")
            .field("model", &self.model)
            .finish()
    }
}

impl AnthropicTransport {
    /// Read the key from the environment. The message never contains it.
    pub(crate) fn from_env(model: &str) -> Result<Self, String> {
        let api_key = std::env::var(KEY_VARIABLE)
            .ok()
            .map(|key| key.trim().to_string())
            .filter(|key| !key.is_empty())
            .ok_or_else(|| format!("{KEY_VARIABLE} is not set in the environment"))?;
        Ok(Self {
            api_key,
            model: model.to_string(),
        })
    }

    /// The key, for the scrubber and the final artefact scan.
    pub(crate) fn secret(&self) -> &str {
        &self.api_key
    }
}

impl RunTransport for AnthropicTransport {
    fn run(
        &self,
        request: ChatRequest,
        executor: Arc<dyn ChatToolExecutor>,
        max_turns: usize,
    ) -> Box<dyn Iterator<Item = ChatDelta> + Send> {
        // Default Anthropic endpoint: `BuiltinAgentKind::default_base_url`.
        let config = BuiltinAgentConfig {
            id: "smoke-real".into(),
            preset: BuiltinAgentPresetKey::Anthropic,
            display_name: "smoke-real".into(),
            kind: BuiltinAgentKind::Anthropic,
            api_key: self.api_key.clone(),
            models: vec![self.model.clone()],
            base_url: String::new(),
            enabled: true,
        };
        let Some(provider) = ConfiguredBuiltinProvider::from_builtin_agent(&config) else {
            return Box::new(
                [
                    ChatDelta::Error("the Anthropic provider could not be set up".into()),
                    ChatDelta::Done {
                        stop_reason: StopReason::Aborted,
                    },
                ]
                .into_iter(),
            );
        };
        // The same provider setup the desktop design loop gets: the design
        // tools, the loop-end finalize, the production turn cap (lowered by
        // `--max-turns`).
        provider
            .with_canvas_tools(design_tool_defs(), executor)
            .with_loop_finalize()
            .with_max_turns(max_turns)
            .send(request)
    }
}
