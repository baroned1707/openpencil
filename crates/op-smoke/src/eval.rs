//! Offline design-agent regression eval (no model, no network).
//!
//! A case is a folder under `eval/cases/<case>/` holding `case.json`: the
//! user prompt, an optional seed document, a hand-scripted transcript of tool
//! calls and the expectations the result must meet. The runner
//! ([`eval_replay`]) feeds that transcript through the same
//! [`HeadlessExecutor`](crate::loop_mode::HeadlessExecutor) the headless loop
//! uses, finalizes the document and scores it with
//! `op_chat_agent::design_eval_metrics::design_eval_metrics`. The prompt check
//! ([`eval_prompt`]) renders the design-agent system prompt for the same
//! prompt and records which skills load and what it costs.
//!
//! The gate ([`eval_gate`]) compares every case with the committed
//! `eval/baseline.json` under `eval/thresholds.json`: an issue count or the
//! content coverage getting worse fails; every other metric is report-only.
//!
//! What this can and cannot catch: a replayed transcript freezes the model's
//! calls, so it tests the code behind the tools (apply, repairs, finalize, the
//! detectors) and not skill or prompt quality. The one prompt-side check is
//! the render in [`eval_prompt`]. Checking behaviour needs a keyed re-record.
//!
//! Entry points
//! - `cargo test -p op-smoke design_eval_matches_baseline`
//! - `OPENPENCIL_SMOKE_EVAL=<dir | 1> op-smoke`: prints the table, exit 1 on a
//!   regression (`1` means this crate's `eval/` folder).
//! - `OPENPENCIL_SMOKE_EVAL_BLESS=1` additionally rewrites `baseline.json`.
//!   It is refused when `CI` is set, so CI never rewrites a baseline.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::Value;

use eval_gate::{Baseline, CaseRecord, GateReport, Thresholds};

#[path = "eval_gate.rs"]
pub(crate) mod eval_gate;
#[path = "eval_prompt.rs"]
pub(crate) mod eval_prompt;
#[path = "eval_replay.rs"]
pub(crate) mod eval_replay;

#[cfg(test)]
#[path = "eval_gate_tests.rs"]
mod gate_tests;
#[cfg(test)]
#[path = "eval_tests.rs"]
mod tests;

/// Why an eval run could not produce a verdict (not a regression: that is a
/// [`GateReport`] with failures).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EvalError {
    /// A fixture, thresholds, baseline or script file could not be read.
    Read { path: PathBuf, reason: String },
    /// A JSON file did not match its schema.
    Parse { path: PathBuf, reason: String },
    /// A transcript step names neither `args` nor `scriptFile` (or both).
    BadStep { case: String, index: usize },
    /// The seed document of a case could not be loaded.
    Seed { case: String, reason: String },
    /// `OPENPENCIL_SMOKE_EVAL_BLESS` was set while `CI` is set.
    BlessRefusedInCi,
    /// The baseline could not be written.
    Write { path: PathBuf, reason: String },
}

impl std::fmt::Display for EvalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EvalError::Read { path, reason } => write!(f, "read {}: {reason}", path.display()),
            EvalError::Parse { path, reason } => write!(f, "parse {}: {reason}", path.display()),
            EvalError::BadStep { case, index } => write!(
                f,
                "case {case}: step {index} needs exactly one of `args` and `scriptFile`"
            ),
            EvalError::Seed { case, reason } => write!(f, "case {case}: seed: {reason}"),
            EvalError::BlessRefusedInCi => f.write_str(
                "OPENPENCIL_SMOKE_EVAL_BLESS refused: CI is set (a baseline is regenerated locally and reviewed in a commit)",
            ),
            EvalError::Write { path, reason } => write!(f, "write {}: {reason}", path.display()),
        }
    }
}

impl std::error::Error for EvalError {}

/// `case.json`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CaseFile {
    pub prompt: String,
    /// A `.op` file next to `case.json` the replay starts from (`None`: an
    /// empty document).
    #[serde(default)]
    pub seed: Option<String>,
    pub steps: Vec<StepFile>,
    #[serde(default)]
    pub expect: Expect,
    /// Free text for reviewers; never read by the runner.
    #[serde(default)]
    #[allow(dead_code)]
    pub note: Option<String>,
}

/// One scripted tool call. `scriptFile` is shorthand for
/// `args: { "script": <file contents> }` so a `batch_design` program stays
/// readable as JavaScript instead of one escaped JSON string.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct StepFile {
    pub tool: String,
    #[serde(default)]
    pub args: Option<Value>,
    #[serde(default)]
    pub script_file: Option<String>,
}

/// What a case's result must satisfy. Violations count as
/// `expectationFailures` (an absolute zero in `thresholds.json`).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Expect {
    /// Every entry must appear (case-insensitive) inside some text node.
    #[serde(default)]
    pub required_text: Vec<String>,
    /// Lower bounds on a metric by its camelCase name (e.g. `screenCount`).
    #[serde(default)]
    pub min: BTreeMap<String, u32>,
    /// Upper bounds on a metric by its camelCase name.
    #[serde(default)]
    pub max: BTreeMap<String, u32>,
    /// A modify case: nodes outside this node's subtree must be unchanged
    /// from the seed. `outsideScopeEdits` counts the differences after the
    /// scripted calls alone, `outsideScopeChanges` after the loop-end repairs.
    #[serde(default)]
    pub scope_root_id: Option<String>,
}

/// A case loaded from disk.
#[derive(Debug, Clone)]
pub(crate) struct Fixture {
    /// The folder name.
    pub id: String,
    pub dir: PathBuf,
    pub file: CaseFile,
}

/// This crate's `eval/` folder.
pub(crate) fn default_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("eval")
}

pub(crate) fn read_text(path: &Path) -> Result<String, EvalError> {
    std::fs::read_to_string(path).map_err(|error| EvalError::Read {
        path: path.to_path_buf(),
        reason: error.to_string(),
    })
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, EvalError> {
    serde_json::from_str(&read_text(path)?).map_err(|error| EvalError::Parse {
        path: path.to_path_buf(),
        reason: error.to_string(),
    })
}

/// Every `<root>/<case>/case.json`, sorted by folder name.
pub(crate) fn load_fixtures(root: &Path) -> Result<Vec<Fixture>, EvalError> {
    let read_dir = std::fs::read_dir(root).map_err(|error| EvalError::Read {
        path: root.to_path_buf(),
        reason: error.to_string(),
    })?;
    let mut dirs: Vec<PathBuf> = read_dir
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.join("case.json").is_file())
        .collect();
    dirs.sort();
    dirs.into_iter()
        .map(|dir| {
            let id = dir
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            let file = read_json(&dir.join("case.json"))?;
            Ok(Fixture { id, dir, file })
        })
        .collect()
}

/// What an eval run is allowed to do, read once from the process environment.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct EvalEnv {
    /// `OPENPENCIL_SMOKE_EVAL_BLESS` is truthy.
    pub bless: bool,
    /// `CI` is set (GitHub Actions sets it to `true`).
    pub ci: bool,
}

impl EvalEnv {
    pub fn from_process() -> Self {
        let truthy = |name: &str| {
            crate::smoke_support::truthy_env_value(std::env::var(name).ok().as_deref())
        };
        Self {
            bless: truthy("OPENPENCIL_SMOKE_EVAL_BLESS"),
            ci: truthy("CI"),
        }
    }

    /// Whether the baseline may be rewritten: only when asked for explicitly,
    /// and never in CI.
    pub fn bless_allowed(&self) -> Result<bool, EvalError> {
        match (self.bless, self.ci) {
            (false, _) => Ok(false),
            (true, true) => Err(EvalError::BlessRefusedInCi),
            (true, false) => Ok(true),
        }
    }
}

/// What a run did.
pub(crate) enum Outcome {
    /// The baseline was rewritten for this many cases.
    Blessed(usize),
    Gated(GateReport),
}

/// Replay every case under `dir/cases`, render each prompt and either gate
/// the result against `dir/baseline.json` or (explicitly) rewrite it.
pub(crate) fn run(dir: &Path, env: &EvalEnv) -> Result<Outcome, EvalError> {
    let bless = env.bless_allowed()?;
    let thresholds: Thresholds = read_json(&dir.join("thresholds.json"))?;
    let mut now = BTreeMap::new();
    for fixture in load_fixtures(&dir.join("cases"))? {
        now.insert(fixture.id.clone(), score_case(&fixture)?);
    }
    let baseline_path = dir.join("baseline.json");
    if bless {
        let baseline = Baseline { cases: now };
        let count = baseline.cases.len();
        std::fs::write(&baseline_path, baseline.to_pretty_json()).map_err(|error| {
            EvalError::Write {
                path: baseline_path.clone(),
                reason: error.to_string(),
            }
        })?;
        return Ok(Outcome::Blessed(count));
    }
    let baseline: Baseline = read_json(&baseline_path)?;
    Ok(Outcome::Gated(eval_gate::gate(
        &now,
        &baseline,
        &thresholds,
    )))
}

/// Replay one case and render its prompt: the record the gate compares.
pub(crate) fn score_case(fixture: &Fixture) -> Result<CaseRecord, EvalError> {
    let replayed = eval_replay::replay(fixture)?;
    Ok(CaseRecord {
        metrics: replayed.metrics,
        prompt: eval_prompt::render_prompt(&fixture.file.prompt),
    })
}

/// `OPENPENCIL_SMOKE_EVAL=<dir | 1>`: run the eval and exit (0 pass, 1
/// regression, 3 cannot run). `None` when the variable is unset.
pub(crate) fn run_if_requested() -> Option<std::process::ExitCode> {
    let requested = std::env::var("OPENPENCIL_SMOKE_EVAL").ok()?;
    let dir = if requested == "1" || requested.is_empty() {
        default_dir()
    } else {
        PathBuf::from(requested)
    };
    Some(match run(&dir, &EvalEnv::from_process()) {
        Ok(Outcome::Blessed(count)) => {
            println!(
                "[EVAL] baseline rewritten for {count} case(s): review the diff and commit it"
            );
            std::process::ExitCode::SUCCESS
        }
        Ok(Outcome::Gated(report)) => {
            println!("{}", report.table());
            if report.passed() {
                std::process::ExitCode::SUCCESS
            } else {
                eprintln!("[EVAL] {} regression(s):", report.failures.len());
                for failure in &report.failures {
                    eprintln!("  {failure}");
                }
                std::process::ExitCode::from(1)
            }
        }
        Err(error) => {
            eprintln!("[EVAL] {error}");
            std::process::ExitCode::from(3)
        }
    })
}
