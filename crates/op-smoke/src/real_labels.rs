//! `--summarize-labels labels.json`: the accept rate per case and the most
//! common failed check, short enough for the owner to paste back.

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::Value;

/// Counts for one case.
#[derive(Debug, Default)]
struct CaseTally {
    judged: u32,
    accepted: u32,
    failed_checks: BTreeMap<String, u32>,
}

fn most_common(counts: &BTreeMap<String, u32>) -> Option<(&String, &u32)> {
    // Highest count; the first key alphabetically breaks a tie.
    counts
        .iter()
        .fold(None, |best: Option<(&String, &u32)>, entry| match best {
            Some(current) if current.1 >= entry.1 => Some(current),
            _ => Some(entry),
        })
}

fn percent(part: u32, whole: u32) -> u32 {
    (part * 100 + whole / 2).checked_div(whole).unwrap_or(0)
}

pub(crate) fn summarize_file(path: &Path) -> Result<String, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| format!("read {}: {error}", path.display()))?;
    summarize(&text)
}

/// The summary of the text of a `labels.json`.
pub(crate) fn summarize(text: &str) -> Result<String, String> {
    let root: Value =
        serde_json::from_str(text).map_err(|error| format!("labels.json is not JSON: {error}"))?;
    let labels = root
        .get("labels")
        .and_then(Value::as_array)
        .ok_or("labels.json has no `labels` list")?;
    let mut per_case: BTreeMap<String, CaseTally> = BTreeMap::new();
    let mut overall = CaseTally::default();
    let mut undecided = 0u32;
    for label in labels {
        let run_id = label.get("runId").and_then(Value::as_str).unwrap_or("?");
        let case = label
            .get("caseId")
            .and_then(Value::as_str)
            .or_else(|| run_id.split('/').next())
            .unwrap_or("?")
            .to_string();
        let decision = label.get("decision").and_then(Value::as_str);
        if !matches!(decision, Some("accept" | "retry")) {
            undecided += 1;
            continue;
        }
        let tally = per_case.entry(case).or_default();
        for each in [&mut *tally, &mut overall] {
            each.judged += 1;
            each.accepted += u32::from(decision == Some("accept"));
        }
        if let Some(checks) = label.get("checks").and_then(Value::as_object) {
            for (key, answer) in checks {
                if answer.as_bool() == Some(false) {
                    for each in [&mut *tally, &mut overall] {
                        *each.failed_checks.entry(key.clone()).or_insert(0) += 1;
                    }
                }
            }
        }
    }
    let mut out = String::from("case | accepted | most failed check\n");
    for (case, tally) in &per_case {
        out.push_str(&format!(
            "{case} | {}/{} ({}%) | {}\n",
            tally.accepted,
            tally.judged,
            percent(tally.accepted, tally.judged),
            describe_failed(&tally.failed_checks)
        ));
    }
    out.push_str(&format!(
        "ALL | {}/{} ({}%) | {}\n",
        overall.accepted,
        overall.judged,
        percent(overall.accepted, overall.judged),
        describe_failed(&overall.failed_checks)
    ));
    if undecided > 0 {
        out.push_str(&format!(
            "{undecided} run(s) had no decision and are not counted\n"
        ));
    }
    Ok(out)
}

fn describe_failed(counts: &BTreeMap<String, u32>) -> String {
    match most_common(counts) {
        Some((key, count)) => format!("{key} ({count}x no)"),
        None => "none".to_string(),
    }
}
