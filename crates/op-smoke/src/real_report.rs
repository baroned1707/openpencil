//! `report.md`: one table row per run, for the owner and for pasting back.

use serde_json::Value;

use super::real_args::{thousands, Config};
use super::real_executor::billable_tokens;
use super::RunRecord;

/// `L2 I0 C1 F0 S0` from the design metrics, or `-` when there are none.
pub(crate) fn issue_counts(metrics: Option<&Value>) -> String {
    let Some(metrics) = metrics else {
        return "-".into();
    };
    let count = |key: &str| metrics.get(key).and_then(Value::as_u64).unwrap_or(0);
    format!(
        "L{} I{} C{} F{} S{}",
        count("layoutIssues"),
        count("iconIssues"),
        count("contrastIssues"),
        count("fontIssues"),
        count("structureIssues"),
    )
}

/// Content coverage as a percentage, or `-`.
pub(crate) fn coverage(metrics: Option<&Value>) -> String {
    metrics
        .and_then(|metrics| metrics.get("contentCoveragePermille"))
        .and_then(Value::as_u64)
        .map_or("-".into(), |permille| format!("{}%", permille / 10))
}

/// Table cells must not break the row.
fn cell(text: &str) -> String {
    text.replace('|', "/").replace(['\n', '\r'], " ")
}

pub(crate) fn report_markdown(records: &[RunRecord], config: &Config, spent: u64) -> String {
    let mut out = String::new();
    out.push_str("# Real-model design runs\n\n");
    out.push_str(&format!(
        "Model `{}`. Tokens used: {} of {} allowed (input + cache writes + cache reads + output).\n\n",
        config.model,
        thousands(spent),
        thousands(config.max_total_tokens)
    ));
    out.push_str(
        "Issue counts: L layout, I icon, C contrast, F font, S structure.\n\n\
         | case | run | verdict | issues | coverage | tokens | seconds |\n\
         | --- | --- | --- | --- | --- | --- | --- |\n",
    );
    for record in records {
        let verdict = match record.status.reason() {
            Some(reason) => format!("{}: {}", record.status.label(), reason),
            None => format!("{} ({})", record.status.label(), record.verdict),
        };
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {:.1} |\n",
            record.case_id,
            record.run,
            cell(&verdict),
            issue_counts(record.metrics.as_ref()),
            coverage(record.metrics.as_ref()),
            thousands(billable_tokens(&record.usage)),
            record.seconds,
        ));
    }
    out
}
