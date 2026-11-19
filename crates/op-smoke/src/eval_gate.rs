//! The regression gate: committed thresholds and baseline in, a verdict and a
//! table out. Everything here is pure (no files, no environment), so the rules
//! are tested directly.
//!
//! Rules, per case, in `thresholds.json` order:
//! - a gated metric that got WORSE than the baseline fails (exact compare, no
//!   tolerance: replay is deterministic); equal passes; better passes and is
//!   marked `improved` (bless to lock it in);
//! - an `absolute` limit fails whatever the baseline says;
//! - a metric with `gate: false` is report-only: shown, never failing;
//! - the skills loaded into the rendered prompt must equal the baseline's, and
//!   the prompt token estimate must stay at or under `promptTokenCeiling`;
//! - a case with no baseline entry, or a baseline entry with no case, fails.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::eval_prompt::PromptRender;

/// Which way a metric improves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Direction {
    Lower,
    Higher,
}

/// One metric's line in `thresholds.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct MetricRule {
    pub name: String,
    pub direction: Direction,
    /// `false`: report-only.
    pub gate: bool,
}

/// `thresholds.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Thresholds {
    /// Every metric to show, in table order.
    pub metrics: Vec<MetricRule>,
    /// Per-case limits that hold whatever the baseline is: `now <= limit`.
    pub absolute: BTreeMap<String, u32>,
    /// The most tokens a rendered prompt may cost.
    pub prompt_token_ceiling: u32,
}

/// What one case scored and rendered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CaseRecord {
    pub metrics: BTreeMap<String, u32>,
    pub prompt: PromptRender,
}

/// `baseline.json`. Cases are keyed by folder name in a sorted map, so the
/// file is byte-stable.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Baseline {
    pub cases: BTreeMap<String, CaseRecord>,
}

impl Baseline {
    pub fn to_pretty_json(&self) -> String {
        let mut text = serde_json::to_string_pretty(self).unwrap_or_default();
        text.push('\n');
        text
    }
}

/// How one table row came out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Status {
    Ok,
    Improved,
    Regressed,
    /// Report-only metric, same as the baseline.
    Report,
    /// Report-only metric that got worse (never fails).
    ReportWorse,
    NoBaseline,
}

impl Status {
    fn label(self) -> &'static str {
        match self {
            Status::Ok => "ok",
            Status::Improved => "improved",
            Status::Regressed => "REGRESSED",
            Status::Report => "report",
            Status::ReportWorse => "report (worse)",
            Status::NoBaseline => "NO BASELINE",
        }
    }
}

/// One line of the table: a metric of a case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Row {
    pub case: String,
    pub metric: String,
    pub baseline: String,
    pub now: String,
    pub status: Status,
}

/// The verdict: every row, and one sentence per failure.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct GateReport {
    pub rows: Vec<Row>,
    pub failures: Vec<String>,
}

impl GateReport {
    pub fn passed(&self) -> bool {
        self.failures.is_empty()
    }

    /// `case | metric | baseline | now | status`, columns aligned.
    pub fn table(&self) -> String {
        let header = ["case", "metric", "baseline", "now", "status"].map(String::from);
        let lines: Vec<[String; 5]> = std::iter::once(header)
            .chain(self.rows.iter().map(|row| {
                [
                    row.case.clone(),
                    row.metric.clone(),
                    row.baseline.clone(),
                    row.now.clone(),
                    row.status.label().to_string(),
                ]
            }))
            .collect();
        let widths: Vec<usize> = (0..5)
            .map(|column| {
                lines
                    .iter()
                    .map(|line| line[column].len())
                    .max()
                    .unwrap_or(0)
            })
            .collect();
        lines
            .iter()
            .map(|line| {
                line.iter()
                    .zip(&widths)
                    .map(|(cell, width)| format!("{cell:<width$}"))
                    .collect::<Vec<_>>()
                    .join(" | ")
                    .trim_end()
                    .to_string()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Compare what the cases scored `now` with the committed `baseline`.
pub(crate) fn gate(
    now: &BTreeMap<String, CaseRecord>,
    baseline: &Baseline,
    thresholds: &Thresholds,
) -> GateReport {
    let mut report = GateReport::default();
    for case in baseline
        .cases
        .keys()
        .filter(|case| !now.contains_key(*case))
    {
        report.failures.push(format!(
            "{case}: baseline entry has no case (remove it with a bless)"
        ));
    }
    for (case, record) in now {
        let Some(previous) = baseline.cases.get(case) else {
            report.failures.push(format!(
                "{case}: no baseline entry (review the case, then run with OPENPENCIL_SMOKE_EVAL_BLESS=1)"
            ));
            report.rows.push(Row {
                case: case.clone(),
                metric: "(all)".into(),
                baseline: "-".into(),
                now: "-".into(),
                status: Status::NoBaseline,
            });
            continue;
        };
        gate_metrics(case, record, previous, thresholds, &mut report);
        gate_prompt(
            case,
            &record.prompt,
            &previous.prompt,
            thresholds,
            &mut report,
        );
    }
    report
}

fn gate_metrics(
    case: &str,
    record: &CaseRecord,
    previous: &CaseRecord,
    thresholds: &Thresholds,
    report: &mut GateReport,
) {
    for rule in &thresholds.metrics {
        let Some(&value) = record.metrics.get(&rule.name) else {
            report
                .failures
                .push(format!("{case}: metric {} was not produced", rule.name));
            continue;
        };
        let before = previous.metrics.get(&rule.name).copied();
        let worse = before.is_some_and(|before| match rule.direction {
            Direction::Lower => value > before,
            Direction::Higher => value < before,
        });
        let better = before.is_some_and(|before| match rule.direction {
            Direction::Lower => value < before,
            Direction::Higher => value > before,
        });
        let over_limit = thresholds
            .absolute
            .get(&rule.name)
            .filter(|limit| value > **limit);
        let status = if let Some(limit) = over_limit {
            report.failures.push(format!(
                "{case}: {} = {value} is over the absolute limit {limit}",
                rule.name
            ));
            Status::Regressed
        } else if rule.gate && before.is_none() {
            report.failures.push(format!(
                "{case}: baseline has no {} (bless to record it)",
                rule.name
            ));
            Status::NoBaseline
        } else if !rule.gate {
            if worse {
                Status::ReportWorse
            } else {
                Status::Report
            }
        } else if worse {
            report.failures.push(format!(
                "{case}: {} got worse ({} -> {value}; {} is better)",
                rule.name,
                before.unwrap_or_default(),
                match rule.direction {
                    Direction::Lower => "lower",
                    Direction::Higher => "higher",
                }
            ));
            Status::Regressed
        } else if better {
            Status::Improved
        } else {
            Status::Ok
        };
        report.rows.push(Row {
            case: case.to_string(),
            metric: if rule.gate {
                rule.name.clone()
            } else {
                format!("{} (report)", rule.name)
            },
            baseline: before.map_or("-".into(), |before| before.to_string()),
            now: value.to_string(),
            status,
        });
    }
}

fn gate_prompt(
    case: &str,
    now: &PromptRender,
    before: &PromptRender,
    thresholds: &Thresholds,
    report: &mut GateReport,
) {
    let skills_changed = now.skills != before.skills;
    if skills_changed {
        report.failures.push(format!(
            "{case}: skills in the prompt changed ({} -> {}); bless if intended",
            before.skills.join(","),
            now.skills.join(",")
        ));
    }
    report.rows.push(Row {
        case: case.to_string(),
        metric: "prompt.skills".into(),
        baseline: before.skills.len().to_string(),
        now: now.skills.len().to_string(),
        status: if skills_changed {
            Status::Regressed
        } else {
            Status::Ok
        },
    });
    let ceiling = thresholds.prompt_token_ceiling;
    let over = now.token_estimate > ceiling;
    if over {
        report.failures.push(format!(
            "{case}: prompt is {} tokens, over the ceiling {ceiling}",
            now.token_estimate
        ));
    }
    report.rows.push(Row {
        case: case.to_string(),
        metric: format!("prompt.tokens (ceiling {ceiling}; report)"),
        baseline: before.token_estimate.to_string(),
        now: now.token_estimate.to_string(),
        status: if over {
            Status::Regressed
        } else if now.token_estimate > before.token_estimate {
            Status::ReportWorse
        } else {
            Status::Report
        },
    });
}
