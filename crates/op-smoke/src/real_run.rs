//! Real-model design run recorder (owner-run, spends the owner's API key).
//!
//! The offline eval replays frozen model calls, so it cannot say whether the
//! design agent designs well. This recorder runs the REAL in-app design loop
//! headlessly against the live model for each eval prompt, N times, and leaves
//! per-run artefacts plus `report.md` and a Vietnamese `review.html` the owner
//! uses to judge the results and export `labels.json`.
//!
//! Safety rules (binding):
//! - reachable only with `OPENPENCIL_SMOKE_REAL=<output dir>` AND
//!   `--confirm-spend` (`--plan` needs neither a key nor a network);
//! - the key is read from `ANTHROPIC_API_KEY` at run time, only by
//!   [`real_transport`], and is scrubbed from every text written to disk;
//! - hard token caps stop the run cleanly (see `real_executor`);
//! - a failed run is retried at most once.
//!
//! Spine: this file holds the run records, the orchestration and the entry
//! point; the siblings hold the command line (`real_args`), the executor and
//! budget (`real_executor`), files and screenshots (`real_artifacts`), the
//! real transport (`real_transport`), the reports (`real_report`,
//! `real_review`) and the label summary (`real_labels`).

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use op_ai::chat_provider::{ChatDelta, ChatRequest, EffortLevel, ThinkingMode};
use op_ai::chat_usage::RunUsage;
use op_editor_core::EditorState;
use serde_json::{json, Value};

use crate::eval::{self, Fixture};
use real_args::{case_selected, thousands, Config, Invocation};
use real_artifacts::{render_frames, run_dir, write_file, Shot};
use real_executor::{billable_tokens, RecordingExecutor, RunBudget, RunTransport, Scrubber};

#[path = "real_args.rs"]
pub(crate) mod real_args;
#[path = "real_artifacts.rs"]
pub(crate) mod real_artifacts;
#[path = "real_executor.rs"]
pub(crate) mod real_executor;
#[path = "real_labels.rs"]
pub(crate) mod real_labels;
#[path = "real_report.rs"]
pub(crate) mod real_report;
#[path = "real_review.rs"]
pub(crate) mod real_review;
#[path = "real_transport.rs"]
pub(crate) mod real_transport;

#[cfg(test)]
#[path = "real_fake.rs"]
mod fake;
#[cfg(test)]
#[path = "real_tests.rs"]
mod tests;

/// How a run ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RunStatus {
    Completed,
    /// A token cap ended the run; the document holds the work done so far.
    StoppedAtCap(String),
    Failed(String),
    Skipped(String),
}

impl RunStatus {
    pub(crate) fn label(&self) -> &'static str {
        match self {
            RunStatus::Completed => "completed",
            RunStatus::StoppedAtCap(_) => "stopped-at-cap",
            RunStatus::Failed(_) => "failed",
            RunStatus::Skipped(_) => "skipped",
        }
    }

    /// The one-line reason shown next to a run that did not simply complete.
    pub(crate) fn reason(&self) -> Option<&str> {
        match self {
            RunStatus::Completed => None,
            RunStatus::StoppedAtCap(reason)
            | RunStatus::Failed(reason)
            | RunStatus::Skipped(reason) => Some(reason),
        }
    }
}

/// Everything the reports need about one run.
#[derive(Debug, Clone)]
pub(crate) struct RunRecord {
    pub case_id: String,
    pub run: u32,
    pub prompt: String,
    /// A modify case (its seed document is part of the input).
    pub modify: bool,
    pub status: RunStatus,
    pub usage: RunUsage,
    pub seconds: f64,
    pub metrics: Option<Value>,
    pub verdict: String,
    pub shots: Vec<Shot>,
    pub seed_shots: Vec<Shot>,
}

impl RunRecord {
    pub(crate) fn run_id(&self) -> String {
        format!("{}/run-{}", self.case_id, self.run)
    }

    fn skipped(fixture: &Fixture, run: u32, reason: &str) -> Self {
        Self {
            case_id: fixture.id.clone(),
            run,
            prompt: fixture.file.prompt.clone(),
            modify: fixture.file.seed.is_some(),
            status: RunStatus::Skipped(reason.to_string()),
            usage: RunUsage::default(),
            seconds: 0.0,
            metrics: None,
            verdict: String::new(),
            shots: Vec::new(),
            seed_shots: Vec::new(),
        }
    }
}

/// The runs an invocation would make, in order.
pub(crate) fn planned_runs<'a>(
    fixtures: &'a [Fixture],
    config: &Config,
) -> Vec<(&'a Fixture, u32)> {
    fixtures
        .iter()
        .filter(|fixture| case_selected(&fixture.id, config.only.as_deref()))
        .flat_map(|fixture| (1..=config.runs).map(move |run| (fixture, run)))
        .collect()
}

/// `--plan`: what would run and the worst-case token budget. No network, no
/// files, no key.
pub(crate) fn plan_text(fixtures: &[Fixture], config: &Config) -> String {
    let runs = planned_runs(fixtures, config);
    let mut out = String::new();
    out.push_str(&format!(
        "[PLAN] model {} | {} run(s) | nothing is sent in plan mode\n",
        config.model,
        runs.len()
    ));
    for (fixture, run) in &runs {
        let prompt = eval::eval_prompt::render_prompt(&fixture.file.prompt);
        out.push_str(&format!(
            "  {} run {run}: system prompt ~{} tokens, seed {}\n",
            fixture.id,
            thousands(u64::from(prompt.token_estimate)),
            if fixture.file.seed.is_some() {
                "yes"
            } else {
                "no"
            }
        ));
    }
    let attempts = runs.len() as u64 * 2;
    let uncapped = attempts * config.max_run_tokens;
    let bound = uncapped.min(config.max_total_tokens);
    out.push_str(&format!(
        "[PLAN] caps: {} tokens per run, {} turns per run, {} tokens in total\n",
        thousands(config.max_run_tokens),
        config.max_turns,
        thousands(config.max_total_tokens)
    ));
    out.push_str(&format!(
        "[PLAN] worst case: min({} runs x 2 attempts x {} per run, {} total) = {} tokens, \
         plus up to ~{} past a cap (a cap is checked before each request, not inside one response)\n",
        runs.len(),
        thousands(config.max_run_tokens),
        thousands(config.max_total_tokens),
        thousands(bound),
        thousands(real_args::OVERSHOOT_ESTIMATE)
    ));
    out.push_str(
        "[PLAN] a token here is every token processed: input + cache writes + cache reads + output \
         (a run re-reads its growing prompt each turn, so a run is often hundreds of thousands)\n",
    );
    out
}

/// What one attempt at a run produced.
struct Attempt {
    state: EditorState,
    usage: RunUsage,
    seconds: f64,
    error: Option<String>,
    recorded: real_executor::Recorded,
}

/// One attempt: build the starting document, run the loop through
/// `transport`, collect everything.
fn attempt(
    fixture: &Fixture,
    config: &Config,
    transport: &dyn RunTransport,
    scrub: &Scrubber,
    total_remaining: u64,
) -> Result<Attempt, String> {
    let initial = match fixture.file.seed.as_deref() {
        Some(relative) => eval::eval_replay::load_seed(fixture, relative)
            .map_err(|error| scrub.clean(&error.to_string()))?,
        None => EditorState::new(),
    };
    let prompt = fixture.file.prompt.clone();
    let system_prompt = op_host_services::design_context::design_loop_system_prompt(
        &initial,
        &prompt,
        Some(&config.model),
    );
    let state = Arc::new(Mutex::new(initial));
    let budget = RunBudget {
        max_run_tokens: config.max_run_tokens,
        total_remaining,
    };
    let executor = Arc::new(RecordingExecutor::new(
        state.clone(),
        &prompt,
        budget,
        scrub.clone(),
    ));
    let request = ChatRequest {
        system_prompt,
        user_message: prompt,
        history: Vec::new(),
        max_output_tokens: op_host_services::chat_builtin_http::DESIGN_LOOP_MAX_OUTPUT_TOKENS,
        thinking: ThinkingMode::Adaptive,
        effort: EffortLevel::default(),
        attachments: Vec::new(),
        model: None,
    };
    let started = Instant::now();
    let mut usage = RunUsage::default();
    let mut error: Option<String> = None;
    let max_turns = config.max_turns;
    let driven = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        for delta in transport.run(request, executor.clone(), max_turns) {
            match delta {
                ChatDelta::TextDelta(text) => executor.note_text(&text),
                ChatDelta::Usage(run) => usage = run,
                ChatDelta::Error(text) => {
                    error.get_or_insert_with(|| scrub.clean(&text));
                }
                ChatDelta::Thinking(_) | ChatDelta::ToolUse { .. } | ChatDelta::Done { .. } => {}
            }
        }
    }));
    if driven.is_err() {
        // The panic text is not copied: it could name anything.
        error.get_or_insert_with(|| "the run panicked".to_string());
    }
    let seconds = started.elapsed().as_secs_f64();
    let recorded = executor.take_recorded();
    let final_state = state.lock().map_err(|_| "document lock poisoned")?.clone();
    Ok(Attempt {
        state: final_state,
        usage,
        seconds,
        error,
        recorded,
    })
}

/// A run, with at most one retry after a failure and only while budget is left.
/// Writes the run's artefacts. `spent` is the invocation's tokens so far.
fn record_run(
    fixture: &Fixture,
    run: u32,
    config: &Config,
    transport: &dyn RunTransport,
    scrub: &Scrubber,
    spent: &mut u64,
) -> RunRecord {
    let mut total_usage = RunUsage::default();
    let mut attempts = 0u32;
    let mut last: Option<Attempt> = None;
    let mut setup_error: Option<String> = None;
    for attempt_no in 1..=2u32 {
        let remaining = config.max_total_tokens.saturating_sub(*spent);
        if remaining == 0 {
            break;
        }
        match attempt(fixture, config, transport, scrub, remaining) {
            Ok(done) => {
                attempts = attempt_no;
                *spent += billable_tokens(&done.usage);
                total_usage.add_turn(&done.usage);
                println!(
                    "[REAL] {} run {run} attempt {attempt_no}: {} | this attempt in={} cache_w={} cache_r={} out={} | invocation total {} / {}",
                    fixture.id,
                    done.error.as_deref().map_or("ok", |_| "failed"),
                    thousands(done.usage.input_tokens),
                    thousands(done.usage.cache_creation_input_tokens),
                    thousands(done.usage.cache_read_input_tokens),
                    thousands(done.usage.output_tokens),
                    thousands(*spent),
                    thousands(config.max_total_tokens),
                );
                // A run a cap stopped is not retried (that would spend more
                // on the same prompt), even if it had drawn nothing yet.
                let failed = done.error.is_some()
                    || (done.recorded.stop_reason.is_none()
                        && done.state.active_children().is_empty());
                last = Some(done);
                if !failed {
                    break;
                }
            }
            Err(reason) => {
                setup_error = Some(reason);
                break;
            }
        }
    }
    finish_record(
        fixture,
        run,
        config,
        scrub,
        (total_usage, attempts),
        last,
        setup_error,
    )
}

fn finish_record(
    fixture: &Fixture,
    run: u32,
    config: &Config,
    scrub: &Scrubber,
    (usage, attempts): (RunUsage, u32),
    last: Option<Attempt>,
    setup_error: Option<String>,
) -> RunRecord {
    let mut record = RunRecord::skipped(fixture, run, "");
    record.usage = usage;
    let Some(done) = last else {
        record.status = match setup_error {
            Some(reason) => RunStatus::Failed(reason),
            None => RunStatus::Skipped("the total token cap was already used up".into()),
        };
        return record;
    };
    record.seconds = done.seconds;
    let wrote_nothing = done.state.active_children().is_empty();
    record.status = if let Some(error) = &done.error {
        RunStatus::Failed(error.clone())
    } else if let Some(reason) = &done.recorded.stop_reason {
        RunStatus::StoppedAtCap(reason.clone())
    } else if wrote_nothing {
        RunStatus::Failed("the model finished without drawing anything".into())
    } else {
        RunStatus::Completed
    };
    let metrics = op_chat_agent::design_eval_metrics::design_eval_metrics(&done.state);
    let metrics_value = serde_json::to_value(metrics).unwrap_or(Value::Null);
    record.verdict = finalize_verdict(&done.recorded);
    record.shots = render_frames(&done.state);
    record.seed_shots = seed_shots(fixture);
    let skills = eval::eval_prompt::render_prompt(&fixture.file.prompt).skills;
    let metrics_file = json!({
        "caseId": fixture.id,
        "run": run,
        "model": config.model,
        "status": record.status.label(),
        "reason": record.status.reason().map(|text| scrub.clean(text)),
        "skillsLoaded": skills,
        // `turns` and `usage` add up every attempt; the tool counts, the
        // timeline and the document come from the last attempt only.
        "attempts": attempts,
        "turns": record.usage.turns,
        "toolCalls": done.recorded.tool_counts,
        "diagnosticsPerWrite": done.recorded.timeline,
        "usage": {
            "inputTokens": record.usage.input_tokens,
            "cacheWriteTokens": record.usage.cache_creation_input_tokens,
            "cacheReadTokens": record.usage.cache_read_input_tokens,
            "outputTokens": record.usage.output_tokens,
            "billableTokens": billable_tokens(&record.usage),
        },
        "wallSeconds": (done.seconds * 10.0).round() / 10.0,
        "finalize": record.verdict,
        "designMetrics": metrics_value,
    });
    // A failed run's numbers describe an empty or partial document: not shown.
    record.metrics = (!matches!(record.status, RunStatus::Failed(_))).then_some(metrics_value);
    let doc_json = serde_json::to_string_pretty(&done.state.doc).unwrap_or_default();
    let transcript =
        json!({ "caseId": fixture.id, "run": run, "events": done.recorded.transcript });
    let dir = run_dir(&config.out, &fixture.id, run);
    let mut files: Vec<(String, Vec<u8>)> = vec![
        ("result.op".into(), scrub.clean(&doc_json).into_bytes()),
        (
            "metrics.json".into(),
            pretty(&metrics_file, scrub).into_bytes(),
        ),
        (
            "transcript.json".into(),
            pretty(&transcript, scrub).into_bytes(),
        ),
    ];
    for shot in &record.shots {
        files.push((format!("shot-{}.png", shot.name), shot.png.clone()));
    }
    for (name, bytes) in files {
        if let Err(error) = write_file(&dir.join(&name), &bytes) {
            record.status = RunStatus::Failed(scrub.clean(&error));
        }
    }
    record
}

fn pretty(value: &Value, scrub: &Scrubber) -> String {
    scrub.clean(&serde_json::to_string_pretty(value).unwrap_or_default())
}

/// What finalize reported, in one line.
fn finalize_verdict(recorded: &real_executor::Recorded) -> String {
    let Some(report) = &recorded.finalize else {
        return "finalize did not run".to_string();
    };
    let repairs: usize = report.quality.repairs.iter().map(|(_, count)| count).sum();
    let mut text = format!(
        "{} screen(s), {} repair(s)",
        report.screens.committed.len(),
        repairs
    );
    if !report.screens.unfilled.is_empty() {
        text.push_str(&format!(
            ", unfilled: {}",
            report.screens.unfilled.join(", ")
        ));
    }
    text
}

/// The frames of a modify case's seed document.
fn seed_shots(fixture: &Fixture) -> Vec<Shot> {
    let Some(relative) = fixture.file.seed.as_deref() else {
        return Vec::new();
    };
    eval::eval_replay::load_seed(fixture, relative)
        .map(|state| render_frames(&state))
        .unwrap_or_default()
}

/// Run everything `config` selects through `transport`, write the artefacts
/// and both reports, and return the records. `fixtures` are the eval cases.
pub(crate) fn execute(
    fixtures: &[Fixture],
    config: &Config,
    transport: &dyn RunTransport,
    secret: &str,
) -> Result<Vec<RunRecord>, String> {
    ensure_empty_output(&config.out)?;
    let planned = planned_runs(fixtures, config);
    if planned.is_empty() {
        return Err(format!(
            "no eval case matches --only {:?}",
            config.only.as_deref().unwrap_or_default()
        ));
    }
    let scrub = Scrubber::new(secret);
    let mut records = Vec::new();
    let mut spent = 0u64;
    for (fixture, run) in planned {
        if spent >= config.max_total_tokens {
            let reason = format!(
                "the total token cap ({}) was reached",
                thousands(config.max_total_tokens)
            );
            println!("[REAL] {} run {run}: skipped ({reason})", fixture.id);
            records.push(RunRecord::skipped(fixture, run, &reason));
            continue;
        }
        records.push(record_run(
            fixture, run, config, transport, &scrub, &mut spent,
        ));
    }
    println!(
        "[REAL] done: {} token(s) used of {} allowed",
        thousands(spent),
        thousands(config.max_total_tokens)
    );
    write_file(
        &config.out.join("report.md"),
        scrub
            .clean(&real_report::report_markdown(&records, config, spent))
            .as_bytes(),
    )?;
    write_file(
        &config.out.join("review.html"),
        scrub
            .clean(&real_review::review_html(&records, &run_set_id()))
            .as_bytes(),
    )?;
    if let Some(file) = real_artifacts::find_secret(&config.out, secret) {
        return Err(format!(
            "the API key was found in {}: delete the output folder and report this",
            file.display()
        ));
    }
    Ok(records)
}

/// Names this invocation's run set: start time and process id.
fn run_set_id() -> String {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs());
    format!("{seconds}-{}", std::process::id())
}

/// Only a current run set lives in the folder: refuse one that has files.
fn ensure_empty_output(out: &Path) -> Result<(), String> {
    let has_files = std::fs::read_dir(out).is_ok_and(|mut entries| entries.next().is_some());
    if has_files {
        return Err(format!(
            "the output folder {} is not empty; use a new folder so the review shows only this run set",
            out.display()
        ));
    }
    Ok(())
}

/// Entry point called from `main`: `None` when this is not a recorder
/// invocation.
pub(crate) async fn run_if_requested() -> Option<std::process::ExitCode> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let out_env = std::env::var(real_args::ENV_OUT).ok();
    let model_env = std::env::var(real_args::ENV_MODEL).ok();
    let ci = crate::smoke_support::truthy_env_value(std::env::var("CI").ok().as_deref());
    let invocation = match real_args::parse(&args, out_env.as_deref(), model_env.as_deref(), ci) {
        Ok(None) => return None,
        Ok(Some(invocation)) => invocation,
        Err(message) => {
            eprintln!("[REAL] {message}");
            return Some(std::process::ExitCode::from(2));
        }
    };
    let code = tokio::task::spawn_blocking(move || run_invocation(invocation))
        .await
        .unwrap_or(std::process::ExitCode::from(1));
    Some(code)
}

/// Builds the transport and returns it with the secret to scrub. Called only
/// by a real run, so `--plan` never reads the key.
pub(crate) type TransportFactory<'a> =
    &'a dyn Fn() -> Result<(Box<dyn RunTransport>, String), String>;

/// Plan or record. A plan never calls `make_transport`.
pub(crate) fn dispatch(
    invocation: Invocation,
    fixtures: &[Fixture],
    make_transport: TransportFactory<'_>,
) -> Result<String, String> {
    match invocation {
        Invocation::SummarizeLabels(path) => real_labels::summarize_file(&path),
        Invocation::Plan(config) => Ok(plan_text(fixtures, &config)),
        Invocation::Record(config) => {
            let (transport, secret) = make_transport()?;
            let scrub = Scrubber::new(&secret);
            let records = execute(fixtures, &config, transport.as_ref(), &secret)
                .map_err(|message| scrub.clean(&message))?;
            let failed = records
                .iter()
                .filter(|record| matches!(record.status, RunStatus::Failed(_)))
                .count();
            Ok(format!(
                "[REAL] {} run(s), {failed} failed; open review.html in {}",
                records.len(),
                config.out.display()
            ))
        }
    }
}

fn run_invocation(invocation: Invocation) -> std::process::ExitCode {
    let fixtures = if matches!(invocation, Invocation::SummarizeLabels(_)) {
        Vec::new()
    } else {
        match eval::load_fixtures(&eval::default_dir().join("cases")) {
            Ok(fixtures) => fixtures,
            Err(error) => {
                eprintln!("[REAL] {error}");
                return std::process::ExitCode::from(3);
            }
        }
    };
    if matches!(invocation, Invocation::Record(_)) {
        op_host_native::bundled_design_fonts::register();
    }
    let model = match &invocation {
        Invocation::Plan(config) | Invocation::Record(config) => config.model.clone(),
        Invocation::SummarizeLabels(_) => String::new(),
    };
    let factory = move || {
        let transport = real_transport::AnthropicTransport::from_env(&model)?;
        let secret = transport.secret().to_string();
        Ok((Box::new(transport) as Box<dyn RunTransport>, secret))
    };
    match dispatch(invocation, &fixtures, &factory) {
        Ok(text) => {
            println!("{text}");
            std::process::ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("[REAL] {message}");
            std::process::ExitCode::from(1)
        }
    }
}
