//! Tests for the `spawn_agents` launch decision: the ack and the launcher
//! must tell the same story, and a call that cannot launch is a tool error.

use super::*;
use crate::chat_session::handle_spawn_agents;
use crate::sub_agent_session::{
    abort_all, launch_sub_agents, set_sub_batch_live, take_pending_spawn,
};
use op_editor_core::{
    AgentProvider, BuiltinAgentConfig, BuiltinAgentKind, BuiltinAgentPresetKey, ModelEntry,
};
use op_mcp::spawn_agents_tool::SpawnSpec;

fn ready() -> LaunchConditions {
    LaunchConditions {
        nested: false,
        model_ready: true,
        batch_live: false,
        queued: false,
    }
}

fn two_agent_args() -> String {
    serde_json::json!({
        "config": [
            {"prompt": "Home", "containerNodes": ["a"], "styleguideName": "brand"},
            {"prompt": "Cart mobile screen", "containerNodes": ["b"], "styleguideName": "brand"},
        ]
    })
    .to_string()
}

fn specs(count: usize) -> Vec<SpawnSpec> {
    (0..count)
        .map(|i| SpawnSpec {
            prompt: format!("screen {i}"),
            container_nodes: vec![format!("c{i}")],
            styleguide_name: "brand".into(),
            guideline_names: vec![],
        })
        .collect()
}

/// A host whose selected model is a ready built-in one (no network is touched:
/// the base URL is a closed local port).
fn host_with_ready_model() -> op_host_native::WidgetHostNative {
    let mut host = op_host_native::WidgetHostNative::new();
    host.editor_state_mut()
        .editor_ui
        .agent_settings
        .builtin_agents
        .push(BuiltinAgentConfig {
            id: "builtin-1".into(),
            preset: BuiltinAgentPresetKey::Custom,
            display_name: "MiniMax".into(),
            kind: BuiltinAgentKind::OpenAiCompat,
            api_key: "sk-test".into(),
            models: vec!["MiniMax-M3".into()],
            base_url: "http://127.0.0.1:9".into(),
            enabled: true,
        });
    host.editor_state_mut().chat.available_models = vec![ModelEntry::builtin(
        AgentProvider::ClaudeCode,
        "builtin-1",
        "builtin:builtin-1:MiniMax-M3",
        "MiniMax M3",
    )];
    host.editor_state_mut().chat.selected_model = 0;
    host
}

/// Reset the process-wide spawn statics a test may leave behind.
fn reset_spawn_state() {
    let _ = take_pending_spawn();
    set_sub_batch_live(false);
}

fn lock() -> std::sync::MutexGuard<'static, ()> {
    crate::agent_indicator_test_lock::LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

#[test]
fn ack_reports_real_launch_count_and_skips() {
    // Spec 1 cannot launch: the ack carries the real count, the real ids and
    // a `skipped` entry with the reason.
    let plan = plan_launch_with(3, |index| {
        (index == 1).then_some(LaunchRefusal::NoModelReady)
    });
    assert_eq!(plan.launch, vec![0, 2]);
    let (content, is_error) = spawn_ack(&plan);
    assert!(!is_error, "{content}");
    let value: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(value["success"], true);
    assert_eq!(value["data"]["spawned"], 2);
    assert_eq!(
        value["data"]["agentIds"],
        serde_json::json!(["agent-0", "agent-2"])
    );
    assert_eq!(value["data"]["skipped"][0]["index"], 1);
    assert_eq!(
        value["data"]["skipped"][0]["reason"],
        LaunchRefusal::NoModelReady.to_string()
    );

    // A fully launchable plan carries no `skipped` key.
    let (content, _) = spawn_ack(&plan_launch(2, &ready()));
    let value: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(value["data"]["spawned"], 2);
    assert!(value["data"].get("skipped").is_none(), "{content}");
}

#[test]
fn ack_is_an_error_when_no_subagent_can_launch() {
    let _guard = lock();
    reset_spawn_state();
    for (conditions, reason) in [
        (
            LaunchConditions {
                model_ready: false,
                ..ready()
            },
            "no ready built-in model",
        ),
        (
            LaunchConditions {
                batch_live: true,
                ..ready()
            },
            "still running",
        ),
        (
            LaunchConditions {
                queued: true,
                ..ready()
            },
            "already queued",
        ),
    ] {
        let ack = handle_spawn_agents(&two_agent_args(), &conditions);
        assert!(ack.is_error, "{}", ack.content);
        let value: serde_json::Value = serde_json::from_str(&ack.content).unwrap();
        assert_eq!(value["success"], false);
        let text = value["error"].as_str().unwrap();
        assert!(text.contains(reason), "{text}");
        assert!(text.contains("started nothing"), "{text}");
        assert!(text.contains("batch_design"), "{text}");
        assert!(
            take_pending_spawn().is_none(),
            "a refused call stashes nothing"
        );
    }
}

#[test]
fn ack_and_launcher_share_one_launchability_check() {
    let _guard = lock();
    reset_spawn_state();
    let mut host = host_with_ready_model();

    // Launchable: the ack counts 2 and the launcher starts exactly 2.
    let conditions = LaunchConditions::observe(&host);
    assert_eq!(conditions.refusal(), None);
    let (content, is_error) = spawn_ack(&plan_launch(2, &conditions));
    assert!(!is_error, "{content}");
    let promised = serde_json::from_str::<serde_json::Value>(&content).unwrap()["data"]["spawned"]
        .as_u64()
        .unwrap() as usize;
    let mut subs = launch_sub_agents(&mut host, specs(2));
    assert_eq!(
        subs.len(),
        promised,
        "the launcher must start what the ack counted"
    );
    let mut active = 0;
    abort_all(&mut subs, &mut active);

    // Not launchable (batch already running): the ack is an error and the
    // launcher starts nothing, off the same observed conditions.
    set_sub_batch_live(true);
    let ack = handle_spawn_agents(&two_agent_args(), &LaunchConditions::observe(&host));
    assert!(ack.is_error, "{}", ack.content);
    assert!(launch_sub_agents(&mut host, specs(2)).is_empty());
    reset_spawn_state();

    // No model: same agreement from the other side.
    let mut bare = op_host_native::WidgetHostNative::new();
    let ack = handle_spawn_agents(&two_agent_args(), &LaunchConditions::observe(&bare));
    assert!(ack.is_error, "{}", ack.content);
    assert!(launch_sub_agents(&mut bare, specs(2)).is_empty());
    assert_eq!(bare.editor_state().chat.agents_running, (0, 0));
}

#[test]
fn valid_spawn_still_acks_and_launches_in_order() {
    let _guard = lock();
    reset_spawn_state();
    let mut host = host_with_ready_model();

    let ack = handle_spawn_agents(&two_agent_args(), &LaunchConditions::observe(&host));
    assert!(!ack.is_error, "{}", ack.content);
    let value: serde_json::Value = serde_json::from_str(&ack.content).unwrap();
    assert_eq!(value["data"]["spawned"], 2);
    assert_eq!(
        value["data"]["agentIds"],
        serde_json::json!(["agent-0", "agent-1"])
    );

    // A second call before the host launched the first is refused, not
    // silently overwriting the queued batch.
    let second = handle_spawn_agents(&two_agent_args(), &LaunchConditions::observe(&host));
    assert!(second.is_error, "{}", second.content);
    assert!(
        second.content.contains("already queued"),
        "{}",
        second.content
    );

    let stashed = take_pending_spawn().expect("the first call's specs are queued");
    assert_eq!(stashed[0].prompt, "Home");
    assert_eq!(stashed[1].prompt, "Cart mobile screen");
    let mut subs = launch_sub_agents(&mut host, stashed);
    assert_eq!(subs.len(), 2);
    // Order follows the spec order: only the second prompt asks for mobile.
    assert!(!subs[0].root_seed_mobile);
    assert!(subs[1].root_seed_mobile);
    assert_ne!(subs[0].identity, subs[1].identity);
    assert_eq!(host.editor_state().chat.agents_running, (2, 2));
    let mut active = 0;
    abort_all(&mut subs, &mut active);
}
