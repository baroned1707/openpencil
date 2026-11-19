//! The gate rules, on synthetic records (no replay, no files, no environment).

use std::collections::BTreeMap;

use super::eval_gate::{
    gate, Baseline, CaseRecord, Direction, GateReport, MetricRule, Status, Thresholds,
};
use super::eval_prompt::PromptRender;

fn rule(name: &str, direction: Direction, gate: bool) -> MetricRule {
    MetricRule {
        name: name.to_string(),
        direction,
        gate,
    }
}

fn thresholds() -> Thresholds {
    Thresholds {
        metrics: vec![
            rule("layoutIssues", Direction::Lower, true),
            rule("contrastIssues", Direction::Lower, true),
            rule("placeholderText", Direction::Lower, true),
            rule("contentCoveragePermille", Direction::Higher, true),
            rule("tapTargetsUnder24", Direction::Lower, false),
        ],
        absolute: BTreeMap::from([
            ("contrastIssues".to_string(), 0),
            ("placeholderText".to_string(), 0),
        ]),
        prompt_token_ceiling: 1000,
    }
}

fn prompt(skills: &[&str], tokens: u32) -> PromptRender {
    PromptRender {
        skills: skills.iter().map(|name| name.to_string()).collect(),
        token_estimate: tokens,
    }
}

/// A clean case, with `overrides` applied on top.
fn record(overrides: &[(&str, u32)]) -> CaseRecord {
    let mut metrics = BTreeMap::from([
        ("layoutIssues".to_string(), 1),
        ("contrastIssues".to_string(), 0),
        ("placeholderText".to_string(), 0),
        ("contentCoveragePermille".to_string(), 900),
        ("tapTargetsUnder24".to_string(), 2),
    ]);
    for (name, value) in overrides {
        metrics.insert(name.to_string(), *value);
    }
    CaseRecord {
        metrics,
        prompt: prompt(&["mobile-app"], 800),
    }
}

fn run(now: CaseRecord, before: CaseRecord) -> GateReport {
    gate(
        &BTreeMap::from([("case".to_string(), now)]),
        &Baseline {
            cases: BTreeMap::from([("case".to_string(), before)]),
        },
        &thresholds(),
    )
}

fn status_of(report: &GateReport, metric: &str) -> Status {
    report
        .rows
        .iter()
        .find(|row| row.metric.starts_with(metric))
        .unwrap_or_else(|| panic!("no row for {metric}"))
        .status
}

/// Invariant: an identical result passes, with every row `ok` or `report`.
#[test]
fn gate_passes_on_equal() {
    let report = run(record(&[]), record(&[]));
    assert!(report.passed(), "{:?}", report.failures);
    assert_eq!(status_of(&report, "layoutIssues"), Status::Ok);
}

/// Invariant: a worse issue count fails (exact compare, no tolerance).
#[test]
fn gate_fails_on_a_worse_issue_count() {
    let report = run(record(&[("layoutIssues", 2)]), record(&[]));
    assert!(!report.passed());
    assert_eq!(status_of(&report, "layoutIssues"), Status::Regressed);
    assert!(report.failures[0].contains("layoutIssues got worse (1 -> 2"));
}

#[test]
fn gate_passes_and_marks_a_better_issue_count() {
    let report = run(record(&[("layoutIssues", 0)]), record(&[]));
    assert!(report.passed());
    assert_eq!(status_of(&report, "layoutIssues"), Status::Improved);
}

/// Invariant: lower content coverage fails, higher passes.
#[test]
fn gate_fails_on_lower_coverage() {
    let report = run(record(&[("contentCoveragePermille", 899)]), record(&[]));
    assert!(!report.passed());
    assert_eq!(
        status_of(&report, "contentCoveragePermille"),
        Status::Regressed
    );
    let better = run(record(&[("contentCoveragePermille", 901)]), record(&[]));
    assert!(better.passed());
}

/// Invariant: placeholder text fails the gate even when the baseline already
/// holds it (the absolute limit does not depend on the baseline).
#[test]
fn gate_fails_on_placeholder_text_whatever_the_baseline_says() {
    let both = record(&[("placeholderText", 2)]);
    let report = run(both.clone(), both);
    assert!(!report.passed());
    assert!(
        report
            .failures
            .iter()
            .any(|line| line.contains("placeholderText = 2 is over the absolute limit 0")),
        "{:?}",
        report.failures
    );
}

#[test]
fn gate_fails_on_contrast_issues_over_the_absolute_limit() {
    let both = record(&[("contrastIssues", 1)]);
    assert!(!run(both.clone(), both).passed());
}

/// Invariant: report-only metrics are shown and marked, and never fail.
#[test]
fn report_only_metrics_never_fail() {
    let report = run(record(&[("tapTargetsUnder24", 9)]), record(&[]));
    assert!(report.passed(), "{:?}", report.failures);
    assert_eq!(status_of(&report, "tapTargetsUnder24"), Status::ReportWorse);
    let table = report.table();
    assert!(table.contains("tapTargetsUnder24 (report)"), "{table}");
    assert!(!table.contains("layoutIssues (report)"), "{table}");
}

#[test]
fn the_table_lists_every_metric_of_every_case() {
    let now = BTreeMap::from([
        ("a".to_string(), record(&[])),
        ("b".to_string(), record(&[])),
    ]);
    let report = gate(&now, &Baseline { cases: now.clone() }, &thresholds());
    let table = report.table();
    for case in ["a", "b"] {
        for metric in thresholds().metrics {
            let line = table
                .lines()
                .find(|line| line.starts_with(case) && line.contains(&metric.name));
            assert!(
                line.is_some(),
                "{case} / {} missing from\n{table}",
                metric.name
            );
        }
    }
}

/// Invariant: a case without a baseline entry, and a baseline entry without a
/// case, both fail (a new fixture needs a reviewed bless).
#[test]
fn cases_and_baseline_entries_must_match() {
    let now = BTreeMap::from([("new".to_string(), record(&[]))]);
    let report = gate(&now, &Baseline::default(), &thresholds());
    assert!(
        report.failures[0].contains("no baseline entry"),
        "{:?}",
        report.failures
    );

    let stale = Baseline {
        cases: BTreeMap::from([("gone".to_string(), record(&[]))]),
    };
    let report = gate(&BTreeMap::new(), &stale, &thresholds());
    assert!(
        report.failures[0].contains("baseline entry has no case"),
        "{:?}",
        report.failures
    );
}

/// Invariant: the prompt check pins skill names and a token ceiling.
#[test]
fn prompt_check_pins_skills_and_a_token_ceiling() {
    let mut now = record(&[]);
    now.prompt = prompt(&["mobile-app", "design-craft"], 800);
    let report = run(now, record(&[]));
    assert!(
        report.failures[0].contains("skills in the prompt changed"),
        "{:?}",
        report.failures
    );

    let mut over = record(&[]);
    over.prompt = prompt(&["mobile-app"], 1001);
    let report = run(over, record(&[]));
    assert!(
        report.failures[0].contains("over the ceiling 1000"),
        "{:?}",
        report.failures
    );

    let mut growth = record(&[]);
    growth.prompt = prompt(&["mobile-app"], 999);
    let report = run(growth, record(&[]));
    assert!(report.passed(), "growth under the ceiling is report-only");
}

#[test]
fn a_gated_metric_missing_from_the_baseline_fails() {
    let mut before = record(&[]);
    before.metrics.remove("layoutIssues");
    let report = run(record(&[]), before);
    assert!(
        report.failures[0].contains("baseline has no layoutIssues"),
        "{:?}",
        report.failures
    );
}

#[test]
fn baseline_json_is_byte_stable() {
    let baseline = Baseline {
        cases: BTreeMap::from([
            ("b".to_string(), record(&[])),
            ("a".to_string(), record(&[])),
        ]),
    };
    let text = baseline.to_pretty_json();
    assert_eq!(text, baseline.clone().to_pretty_json());
    assert!(text.find("\"a\"").unwrap() < text.find("\"b\"").unwrap());
    assert!(text.ends_with("}\n"));
    let back: Baseline = serde_json::from_str(&text).unwrap();
    assert_eq!(back, baseline);
}
