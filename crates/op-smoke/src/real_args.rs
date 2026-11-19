//! Command line and gate of the real-model design recorder.
//!
//! The recorder spends the owner's money, so it is reachable only through TWO
//! explicit acts: the environment variable `OPENPENCIL_SMOKE_REAL=<output dir>`
//! AND the flag `--confirm-spend` (or `--plan`, which never touches the
//! network). A stray environment variable alone refuses to run, and so does
//! `CI`. Nothing here reads the API key.

use std::path::PathBuf;

pub(crate) const ENV_OUT: &str = "OPENPENCIL_SMOKE_REAL";
pub(crate) const ENV_MODEL: &str = "OPENPENCIL_SMOKE_MODEL";
pub(crate) const CONFIRM_FLAG: &str = "--confirm-spend";

/// Runs per case when `--runs` is not given.
pub(crate) const DEFAULT_RUNS: u32 = 2;
/// Tokens (input + cache writes + cache reads + output, see `real_executor`)
/// one whole invocation may use, summed over every run and retry.
pub(crate) const DEFAULT_MAX_TOTAL_TOKENS: u64 = 1_500_000;
/// Tokens one run may use before it is stopped and finalized.
pub(crate) const DEFAULT_MAX_RUN_TOKENS: u64 = 600_000;
/// A cap is checked BEFORE each model request, so the last request can run
/// past it by a whole request (its full input, cached or not, plus up to
/// 6,144 output tokens). This is an estimate for a long design context, shown
/// in the plan; it is not enforced.
pub(crate) const OVERSHOOT_ESTIMATE: u64 = 100_000;

/// Everything one invocation needs, parsed once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Config {
    pub out: PathBuf,
    pub runs: u32,
    pub only: Option<String>,
    pub max_total_tokens: u64,
    pub max_run_tokens: u64,
    pub max_turns: usize,
    pub model: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Invocation {
    /// `--summarize-labels <labels.json>`: no environment, no network.
    SummarizeLabels(PathBuf),
    /// Print what would run and the worst-case budget. No network.
    Plan(Config),
    /// Run for real.
    Record(Config),
}

/// `Ok(None)`: this is not a recorder invocation (the normal smoke runner goes
/// on). `Err`: a recorder invocation that is refused or malformed.
pub(crate) fn parse(
    args: &[String],
    out_env: Option<&str>,
    model_env: Option<&str>,
    ci: bool,
) -> Result<Option<Invocation>, String> {
    if let Some(at) = args.iter().position(|arg| arg == "--summarize-labels") {
        let path = args
            .get(at + 1)
            .ok_or("--summarize-labels needs the path of labels.json")?;
        return Ok(Some(Invocation::SummarizeLabels(PathBuf::from(path))));
    }
    let Some(out) = out_env.map(str::trim).filter(|out| !out.is_empty()) else {
        return Ok(None);
    };
    if ci {
        return Err(
            "refused: CI is set. The real-model recorder spends money and never runs in CI"
                .to_string(),
        );
    }
    let model = model_env
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .unwrap_or(op_editor_core::agent_settings_builtin_presets::DEFAULT_ANTHROPIC_MODEL);
    let mut config = Config {
        out: PathBuf::from(out),
        runs: DEFAULT_RUNS,
        only: None,
        max_total_tokens: DEFAULT_MAX_TOTAL_TOKENS,
        max_run_tokens: DEFAULT_MAX_RUN_TOKENS,
        max_turns: op_host_services::chat_builtin_http::DESIGN_LOOP_MAX_TURNS,
        model: model.to_string(),
    };
    let (mut plan, mut confirmed) = (false, false);
    let mut rest = args.iter();
    while let Some(flag) = rest.next() {
        let mut value = |name: &str| {
            rest.next()
                .map(String::as_str)
                .ok_or_else(|| format!("{name} needs a value"))
        };
        match flag.as_str() {
            "--plan" => plan = true,
            CONFIRM_FLAG => confirmed = true,
            "--only" => config.only = Some(value("--only")?.to_string()),
            "--runs" => config.runs = positive(value("--runs")?, "--runs")?,
            "--max-total-tokens" => {
                config.max_total_tokens =
                    positive(value("--max-total-tokens")?, "--max-total-tokens")?
            }
            "--max-run-tokens" => {
                config.max_run_tokens = positive(value("--max-run-tokens")?, "--max-run-tokens")?
            }
            "--max-turns" => {
                config.max_turns = positive::<u64>(value("--max-turns")?, "--max-turns")? as usize
            }
            other => return Err(format!("unknown option {other:?}")),
        }
    }
    if plan {
        return Ok(Some(Invocation::Plan(config)));
    }
    if !confirmed {
        return Err(format!(
            "refused: {ENV_OUT} is set but {CONFIRM_FLAG} was not given. \
             Use --plan to see what would run without any network call"
        ));
    }
    Ok(Some(Invocation::Record(config)))
}

fn positive<T: std::str::FromStr + PartialOrd + Default>(
    text: &str,
    name: &str,
) -> Result<T, String> {
    match text.parse::<T>() {
        Ok(value) if value > T::default() => Ok(value),
        _ => Err(format!(
            "{name} needs a whole number above zero, got {text:?}"
        )),
    }
}

/// Whether `--only <wanted>` selects the case folder `id`: the full folder
/// name, or its leading number (`09`).
pub(crate) fn case_selected(id: &str, only: Option<&str>) -> bool {
    only.is_none_or(|wanted| id == wanted || id.split('-').next() == Some(wanted))
}

/// `1234567` -> `1,234,567`.
pub(crate) fn thousands(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::new();
    for (index, ch) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}
