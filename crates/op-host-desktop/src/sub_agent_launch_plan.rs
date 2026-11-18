//! The one decision "can these sub-agents start?" behind `spawn_agents`.
//!
//! Two places need the answer: the tool ack in
//! `chat_session::handle_spawn_agents` (what the parent model is told) and
//! `sub_agent_session::launch_sub_agents` (what the host then does). They used
//! to decide separately, so the ack could claim N agents while the launcher
//! silently skipped them. Both now call [`plan_launch`] over the same
//! [`LaunchConditions`], so they cannot disagree.
//!
//! Today every condition is batch-wide (a model is ready or it is not, a batch
//! is running or it is not), so a plan launches all specs or none. The plan is
//! still per spec so a condition that does vary per spec can be added without
//! touching the ack or the launcher, and so a partial plan is reported as it
//! is: the real count, the real ids and a `skipped` list with reasons.

use std::fmt;

use op_host_native::WidgetHostNative;

use crate::chat_session::builtin_design_ready;
use crate::sub_agent_session::{nested_spawn_active, pending_spawn_queued, sub_batch_live};

/// What the tool tells a sub-agent that calls `spawn_agents` itself.
pub(crate) const SPAWN_AGENTS_NESTED: &str = "spawn_agents is not available to a sub-agent; \
nothing was started. Build your assigned section yourself with batch_design.";

/// The sentence that follows every other refusal, naming the fallback so the
/// parent model can continue without guessing.
const SPAWN_AGENTS_FALLBACK: &str = "Build the remaining sections yourself with batch_design \
(one section per call), then call finalize_design.";

/// Why a sub-agent cannot start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LaunchRefusal {
    /// The caller is itself a sub-agent; only the top-level loop spawns.
    Nested,
    /// No ready built-in model to run a sub-agent with.
    NoModelReady,
    /// Sub-agents of an earlier `spawn_agents` call are still running.
    BatchRunning,
    /// Another `spawn_agents` call of this turn is already waiting to launch.
    AlreadyQueued,
}

impl fmt::Display for LaunchRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            LaunchRefusal::Nested => "sub-agents cannot start further sub-agents",
            LaunchRefusal::NoModelReady => "no ready built-in model is configured for sub-agents",
            LaunchRefusal::BatchRunning => {
                "sub-agents of an earlier spawn_agents call are still running (one batch at a time)"
            }
            LaunchRefusal::AlreadyQueued => {
                "another spawn_agents call in this turn is already queued"
            }
        })
    }
}

impl LaunchRefusal {
    /// The tool-error text for a call that started nothing because of `self`.
    pub(crate) fn tool_error_text(self) -> String {
        match self {
            LaunchRefusal::Nested => SPAWN_AGENTS_NESTED.to_string(),
            other => format!("spawn_agents started nothing: {other}. {SPAWN_AGENTS_FALLBACK}"),
        }
    }
}

/// The facts the launch decision is made from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LaunchConditions {
    /// The call comes from a sub-agent's own turn.
    pub nested: bool,
    /// A built-in model is ready to run a sub-agent.
    pub model_ready: bool,
    /// The document already has a running sub-agent batch.
    pub batch_live: bool,
    /// A `spawn_agents` call is already stashed and not yet launched.
    pub queued: bool,
}

impl LaunchConditions {
    /// Read the conditions from the host and the sub-agent session state.
    pub(crate) fn observe(host: &WidgetHostNative) -> Self {
        Self {
            nested: nested_spawn_active(),
            model_ready: builtin_design_ready(host),
            batch_live: sub_batch_live(),
            queued: pending_spawn_queued(),
        }
    }

    /// The single launchability check: the reason a sub-agent cannot start
    /// now, or `None` when it can.
    pub(crate) fn refusal(&self) -> Option<LaunchRefusal> {
        if self.nested {
            Some(LaunchRefusal::Nested)
        } else if !self.model_ready {
            Some(LaunchRefusal::NoModelReady)
        } else if self.batch_live {
            Some(LaunchRefusal::BatchRunning)
        } else if self.queued {
            Some(LaunchRefusal::AlreadyQueued)
        } else {
            None
        }
    }
}

/// Which of the requested specs launch and which are skipped, by spec index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LaunchPlan {
    /// Indices of the specs that will launch, in request order.
    pub launch: Vec<usize>,
    /// Indices of the specs that will not, with the reason.
    pub skipped: Vec<(usize, LaunchRefusal)>,
}

impl LaunchPlan {
    /// Whether the spec at `index` launches.
    pub(crate) fn launches(&self, index: usize) -> bool {
        self.launch.contains(&index)
    }
}

/// Plan `spec_count` specs against `conditions`. This is the function both the
/// ack and the launcher call.
pub(crate) fn plan_launch(spec_count: usize, conditions: &LaunchConditions) -> LaunchPlan {
    plan_launch_with(spec_count, |_| conditions.refusal())
}

/// [`plan_launch`] with a per-spec check, for conditions that differ by spec.
pub(crate) fn plan_launch_with(
    spec_count: usize,
    check: impl Fn(usize) -> Option<LaunchRefusal>,
) -> LaunchPlan {
    let mut plan = LaunchPlan {
        launch: Vec::new(),
        skipped: Vec::new(),
    };
    for index in 0..spec_count {
        match check(index) {
            None => plan.launch.push(index),
            Some(reason) => plan.skipped.push((index, reason)),
        }
    }
    plan
}

/// The ack the parent model reads for a plan, as `(json, is_error)`.
///
/// Nothing launches: an error naming the first reason. Otherwise the REAL
/// count and ids (`agent-<spec index>`), plus `skipped` when some specs do not
/// launch.
pub(crate) fn spawn_ack(plan: &LaunchPlan) -> (String, bool) {
    if let Some((_, reason)) = plan.skipped.first().filter(|_| plan.launch.is_empty()) {
        let body = serde_json::json!({ "success": false, "error": reason.tool_error_text() });
        return (body.to_string(), true);
    }
    let agent_ids: Vec<String> = plan.launch.iter().map(|i| format!("agent-{i}")).collect();
    let mut data = serde_json::json!({ "spawned": plan.launch.len(), "agentIds": agent_ids });
    if !plan.skipped.is_empty() {
        data["skipped"] = plan
            .skipped
            .iter()
            .map(|(index, reason)| {
                serde_json::json!({ "index": index, "reason": reason.to_string() })
            })
            .collect();
    }
    (
        serde_json::json!({ "success": true, "data": data }).to_string(),
        false,
    )
}

#[cfg(test)]
#[path = "sub_agent_launch_plan_tests.rs"]
mod tests;
