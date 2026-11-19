//! The recorder end to end with scripted models: no network, no key.

use std::path::{Path, PathBuf};

use serde_json::Value;

use super::fake::{calls_of, FakeTransport, PanicTransport, Script};
use super::real_args::{self, Config, Invocation};
use super::real_artifacts::find_secret;
use super::real_review::{escape, review_html};
use super::*;

const SENTINEL: &str = "sk-ant-api03-SENTINEL0123456789abcdef";
const NEW_CASE: &str = "01-mobile-food-delivery";
const MODIFY_CASE: &str = "09-modify-hero-bolder";
const FAILING_CASE: &str = "04-signup-form";

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("op-smoke-real-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        Scratch(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn fixtures(ids: &[&str]) -> Vec<Fixture> {
    op_host_native::bundled_design_fonts::register();
    eval::load_fixtures(&eval::default_dir().join("cases"))
        .expect("fixtures")
        .into_iter()
        .filter(|fixture| ids.contains(&fixture.id.as_str()))
        .collect()
}

fn config(out: &Path) -> Config {
    Config {
        out: out.to_path_buf(),
        runs: 1,
        only: None,
        max_total_tokens: 10_000_000,
        max_run_tokens: 10_000_000,
        max_turns: 28,
        model: "test-model".into(),
    }
}

/// A scripted model that replays each fixture's calls; `FAILING_CASE` fails
/// with an error that contains the key.
fn fake_for(fixtures: &[Fixture], leak: bool) -> FakeTransport {
    let mut fake = FakeTransport::default();
    for fixture in fixtures {
        let script = if fixture.id == FAILING_CASE {
            Script::Fail(format!("401 unauthorized for key {SENTINEL}"))
        } else {
            Script::Steps {
                calls: calls_of(fixture),
                per_call: (1_000, 500),
                leak: leak.then(|| SENTINEL.to_string()),
            }
        };
        fake = fake.with(&fixture.file.prompt, script);
    }
    fake
}

fn read_json(path: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(path).expect("read")).expect("json")
}

fn all_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).expect("dir").flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.extend(all_files(&path));
        } else {
            out.push(path);
        }
    }
    out
}

fn record(ids: &[&str], name: &str, leak: bool) -> (Scratch, Vec<RunRecord>, Config) {
    let scratch = Scratch::new(name);
    let all = fixtures(ids);
    let cfg = config(&scratch.0);
    let records = execute(&all, &cfg, &fake_for(&all, leak), SENTINEL).expect("execute");
    (scratch, records, cfg)
}

#[test]
fn a_fake_run_writes_every_artefact_and_the_metrics_parse() {
    let (scratch, records, _) = record(&[NEW_CASE, MODIFY_CASE], "artefacts", false);
    assert_eq!(records.len(), 2);
    for case in [NEW_CASE, MODIFY_CASE] {
        let dir = scratch.0.join(case).join("run-1");
        let metrics = read_json(&dir.join("metrics.json"));
        assert_eq!(metrics["status"], "completed", "{case}: {metrics}");
        assert!(metrics["turns"].as_u64().unwrap() > 0);
        assert!(!metrics["skillsLoaded"].as_array().unwrap().is_empty());
        assert!(metrics["toolCalls"]["batch_design"].as_u64().unwrap() >= 1);
        assert!(!metrics["diagnosticsPerWrite"]
            .as_array()
            .unwrap()
            .is_empty());
        assert!(metrics["designMetrics"]["screenCount"].is_number());
        assert!(metrics["usage"]["billableTokens"].as_u64().unwrap() > 0);
        assert!(metrics["finalize"].as_str().unwrap().contains("repair"));
        let doc = std::fs::read_to_string(dir.join("result.op")).expect("result.op");
        assert!(serde_json::from_str::<Value>(&doc).is_ok());
        let transcript = read_json(&dir.join("transcript.json"));
        assert!(transcript["events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|event| event["type"] == "tool_call"));
        let shots: Vec<_> = all_files(&dir)
            .into_iter()
            .filter(|path| path.extension().is_some_and(|ext| ext == "png"))
            .collect();
        assert!(!shots.is_empty(), "{case}: a screenshot per frame");
        for shot in shots {
            let bytes = std::fs::read(&shot).unwrap();
            assert_eq!(&bytes[..4], b"\x89PNG", "{}", shot.display());
        }
    }
    let report = std::fs::read_to_string(scratch.0.join("report.md")).expect("report.md");
    assert!(report.contains("| case | run | verdict | issues | coverage | tokens | seconds |"));
    assert!(report.contains(NEW_CASE) && report.contains(MODIFY_CASE));
    // The modify case shows its seed in the review.
    assert!(records.iter().any(|r| r.modify && !r.seed_shots.is_empty()));
}

#[test]
fn transcript_truncates_arguments_and_results() {
    let (scratch, _, _) = record(&[NEW_CASE], "truncate", false);
    let transcript = read_json(&scratch.0.join(NEW_CASE).join("run-1/transcript.json"));
    for event in transcript["events"].as_array().unwrap() {
        if event["type"] == "tool_call" {
            let args = event["args"].as_str().unwrap();
            let result = event["result"].as_str().unwrap();
            // A cut value carries a short marker after the kept bytes.
            assert!(args.len() <= real_executor::TRANSCRIPT_ARGS_BYTES + 64);
            assert!(result.len() <= real_executor::TRANSCRIPT_RESULT_BYTES + 64);
        }
    }
    assert_eq!(
        real_executor::truncate_bytes("héllo", 2),
        "h…[truncated, 6 bytes]"
    );
}

#[test]
fn review_html_is_self_contained_with_one_card_per_run_including_a_failed_one() {
    let (scratch, records, _) = record(&[NEW_CASE, MODIFY_CASE, FAILING_CASE], "html", false);
    assert_eq!(records.len(), 3);
    let html = std::fs::read_to_string(scratch.0.join("review.html")).expect("review.html");
    for forbidden in [
        "http://",
        "https://",
        "<script src",
        "<link",
        "@import",
        "url(",
        "src=\"//",
        "<iframe",
        "fetch(",
        "XMLHttpRequest",
    ] {
        assert!(
            !html.contains(forbidden),
            "review.html contains {forbidden:?}"
        );
    }
    assert_eq!(html.matches("<section class=\"card\"").count(), 3);
    for record in &records {
        assert!(html.contains(&format!("data-run-id=\"{}\"", record.run_id())));
    }
    // The failed run is on the page with its reason, and its key is gone.
    assert!(html.contains("Thất bại"));
    assert!(html.contains("401 unauthorized"));
    assert!(html.contains("data:image/png;base64,"));
    assert!(html.contains("Xuất nhãn") && html.contains("labels.json"));
    assert!(html.contains("prefers-color-scheme:dark"));
    assert!(html.contains("overflow-wrap:anywhere"));
    // The page shows the prompt text of the modify case beside its seed shot.
    assert!(html.contains("Bản gốc"));
    // No file path from the output folder reaches the page.
    assert!(!html.contains(&scratch.0.display().to_string()));
}

#[test]
fn review_html_escapes_text_from_a_run() {
    let injection = "<script>alert(1)</script> & \"quote\" <img src=x onerror=alert(2)>";
    let record = RunRecord {
        case_id: "x".into(),
        run: 1,
        prompt: injection.into(),
        modify: false,
        status: RunStatus::Failed(format!("{injection}\nsecond line")),
        usage: RunUsage::default(),
        seconds: 0.0,
        metrics: None,
        verdict: String::new(),
        shots: Vec::new(),
        seed_shots: Vec::new(),
    };
    let html = review_html(&[record], "set-1");
    assert!(!html.contains("<script>alert(1)"));
    assert!(!html.contains("<img src=x"));
    assert!(html.contains("&lt;script&gt;alert(1)&lt;/script&gt; &amp; &quot;quote&quot;"));
    // The only script on the page is the page's own.
    assert_eq!(html.matches("<script>").count(), 1);
    assert_eq!(escape("a<b"), "a&lt;b");
}

#[test]
fn the_key_never_reaches_any_artefact() {
    let (scratch, records, _) = record(&[NEW_CASE, MODIFY_CASE, FAILING_CASE], "secret", true);
    // The scripted model really did try to leak it.
    let transcript =
        std::fs::read_to_string(scratch.0.join(NEW_CASE).join("run-1/transcript.json"))
            .expect("transcript");
    assert!(
        transcript.contains("[redacted]"),
        "the leak attempt was scrubbed"
    );
    assert!(
        records
            .iter()
            .any(|r| matches!(&r.status, RunStatus::Failed(m) if m.contains("[redacted]"))),
        "the error text kept its shape without the key"
    );
    assert!(
        find_secret(&scratch.0, SENTINEL).is_none(),
        "the sentinel key is in a file"
    );
    for path in all_files(&scratch.0) {
        let bytes = std::fs::read(&path).unwrap();
        assert!(
            !bytes
                .windows(SENTINEL.len())
                .any(|w| w == SENTINEL.as_bytes()),
            "{} holds the key",
            path.display()
        );
        assert!(
            !String::from_utf8_lossy(&bytes).contains("sk-ant-api03"),
            "{}",
            path.display()
        );
    }
    // The scanner itself finds a planted key (it is not vacuous).
    std::fs::write(scratch.0.join("planted.txt"), format!("x {SENTINEL} y")).unwrap();
    assert!(find_secret(&scratch.0, SENTINEL).is_some());
}

#[test]
fn a_failed_run_is_retried_once_and_recorded_with_its_scrubbed_error() {
    let all = fixtures(&[FAILING_CASE]);
    let scratch = Scratch::new("retry");
    let fake = fake_for(&all, false);
    let records = execute(&all, &config(&scratch.0), &fake, SENTINEL).expect("execute");
    assert_eq!(
        fake.started.load(std::sync::atomic::Ordering::SeqCst),
        2,
        "one retry, no more"
    );
    let RunStatus::Failed(reason) = &records[0].status else {
        panic!("{:?}", records[0].status)
    };
    assert!(reason.contains("401") && !reason.contains(SENTINEL));
}

#[test]
fn the_per_run_token_cap_stops_a_run_cleanly() {
    let all = fixtures(&[NEW_CASE]);
    let calls = calls_of(&all[0]).len();
    let scratch = Scratch::new("run-cap");
    let mut cfg = config(&scratch.0);
    // Each scripted call costs 1,500: the cap allows two of them.
    cfg.max_run_tokens = 3_000;
    let fake = fake_for(&all, false);
    let records = execute(&all, &cfg, &fake, SENTINEL).expect("execute");
    assert_eq!(
        fake.started.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "a capped run is not retried"
    );
    assert!(
        matches!(records[0].status, RunStatus::StoppedAtCap(_)),
        "{:?}",
        records[0].status
    );
    assert_eq!(records[0].usage.turns, 2, "of {calls} scripted calls");
    let metrics = read_json(&scratch.0.join(NEW_CASE).join("run-1/metrics.json"));
    assert_eq!(metrics["status"], "stopped-at-cap");
    // Even a capped run is finalized and written.
    assert!(scratch.0.join(NEW_CASE).join("run-1/result.op").is_file());
}

#[test]
fn the_total_token_cap_skips_the_remaining_runs_and_shows_them() {
    let all = fixtures(&[NEW_CASE, MODIFY_CASE]);
    let scratch = Scratch::new("total-cap");
    let mut cfg = config(&scratch.0);
    cfg.runs = 2;
    // One full run of the first case costs calls x 1,500; allow barely more.
    cfg.max_total_tokens = calls_of(&all[0]).len() as u64 * 1_500 + 10;
    let fake = fake_for(&all, false);
    let records = execute(&all, &cfg, &fake, SENTINEL).expect("execute");
    assert_eq!(records.len(), 4, "every planned run is recorded");
    assert!(matches!(
        records[0].status,
        RunStatus::Completed | RunStatus::StoppedAtCap(_)
    ));
    let skipped = records
        .iter()
        .filter(|r| matches!(r.status, RunStatus::Skipped(_)))
        .count();
    assert!(skipped >= 2, "{records:#?}");
    assert!(
        fake.started.load(std::sync::atomic::Ordering::SeqCst) < 4,
        "no model call for a skipped run"
    );
    let html = std::fs::read_to_string(scratch.0.join("review.html")).unwrap();
    assert_eq!(html.matches("<section class=\"card\"").count(), 4);
    assert!(html.contains("Bỏ qua"));
    let report = std::fs::read_to_string(scratch.0.join("report.md")).unwrap();
    assert!(report.contains("skipped: the total token cap"));
}

#[test]
fn plan_makes_no_network_call_and_reads_no_key() {
    let all = fixtures(&[NEW_CASE, MODIFY_CASE]);
    let args: Vec<String> = ["--plan", "--runs", "3"].map(String::from).to_vec();
    let invocation = real_args::parse(&args, Some("/tmp/x"), None, false)
        .expect("parse")
        .expect("a recorder invocation");
    assert!(matches!(invocation, Invocation::Plan(_)));
    // The factory that would read the key and build the transport panics.
    let factory = || -> Result<(Box<dyn RunTransport>, String), String> {
        let _ = PanicTransport;
        panic!("the plan asked for a transport")
    };
    let text = dispatch(invocation, &all, &factory).expect("plan");
    assert!(text.contains("6 run(s)"), "{text}");
    assert!(text.contains("worst case"));
    assert!(text.contains(NEW_CASE) && text.contains("seed yes"));
    // The default caps: 6 runs x 2 attempts x 600,000 is above 1,500,000.
    assert!(text.contains("= 1,500,000 tokens"), "{text}");
    // A real run through the same entry does call the factory.
    let record_args: Vec<String> = ["--confirm-spend"].map(String::from).to_vec();
    let record = real_args::parse(&record_args, Some("/tmp/x"), None, false)
        .unwrap()
        .unwrap();
    let outcome = std::panic::catch_unwind(|| dispatch(record, &[], &factory));
    assert!(outcome.is_err(), "a recording run builds its transport");
}

#[test]
fn the_gate_refuses_without_the_confirmation_flag_or_in_ci() {
    let none: Vec<String> = Vec::new();
    // No env var: not a recorder invocation, whatever the arguments.
    assert_eq!(real_args::parse(&none, None, None, false), Ok(None));
    assert_eq!(real_args::parse(&none, Some("  "), None, false), Ok(None));
    // Env var alone must not spend money.
    let refused = real_args::parse(&none, Some("/tmp/x"), None, false).unwrap_err();
    assert!(refused.contains("--confirm-spend"), "{refused}");
    // CI never runs it, even with the flag.
    let flag: Vec<String> = vec!["--confirm-spend".into()];
    assert!(real_args::parse(&flag, Some("/tmp/x"), None, true)
        .unwrap_err()
        .contains("CI"));
    // Both acts together: a real run, with the documented defaults.
    let Some(Invocation::Record(cfg)) =
        real_args::parse(&flag, Some("/tmp/x"), None, false).unwrap()
    else {
        panic!("expected a recording run")
    };
    assert_eq!(cfg.runs, 2);
    assert_eq!(cfg.max_total_tokens, 1_500_000);
    assert_eq!(cfg.max_run_tokens, 600_000);
    assert_eq!(cfg.max_turns, 28);
    assert_eq!(
        cfg.model,
        op_editor_core::agent_settings_builtin_presets::DEFAULT_ANTHROPIC_MODEL
    );
    // Model and options.
    let args: Vec<String> = [
        "--confirm-spend",
        "--runs",
        "3",
        "--only",
        "09",
        "--max-total-tokens",
        "5",
    ]
    .map(String::from)
    .to_vec();
    let Some(Invocation::Record(cfg)) =
        real_args::parse(&args, Some("/tmp/x"), Some("my-model"), false).unwrap()
    else {
        panic!("expected a recording run")
    };
    assert_eq!(
        (cfg.runs, cfg.max_total_tokens, cfg.model.as_str()),
        (3, 5, "my-model")
    );
    assert!(real_args::case_selected(MODIFY_CASE, cfg.only.as_deref()));
    assert!(!real_args::case_selected(NEW_CASE, cfg.only.as_deref()));
    // Bad values are refused.
    for bad in [
        vec!["--runs", "0"],
        vec!["--runs", "x"],
        vec!["--nope"],
        vec!["--only"],
    ] {
        let mut args: Vec<String> = vec!["--confirm-spend".into()];
        args.extend(bad.iter().map(|s| s.to_string()));
        assert!(
            real_args::parse(&args, Some("/tmp/x"), None, false).is_err(),
            "{bad:?}"
        );
    }
    // Summarizing labels needs no environment.
    let summarize: Vec<String> = vec!["--summarize-labels".into(), "labels.json".into()];
    assert!(matches!(
        real_args::parse(&summarize, None, None, false),
        Ok(Some(Invocation::SummarizeLabels(_)))
    ));
}

#[test]
fn a_used_output_folder_is_refused() {
    let scratch = Scratch::new("used");
    std::fs::create_dir_all(&scratch.0).unwrap();
    std::fs::write(scratch.0.join("old.txt"), "x").unwrap();
    let all = fixtures(&[NEW_CASE]);
    let error = execute(&all, &config(&scratch.0), &fake_for(&all, false), SENTINEL).unwrap_err();
    assert!(error.contains("not empty"), "{error}");
    let unknown = Config {
        only: Some("99".into()),
        ..config(&Scratch::new("unknown").0)
    };
    assert!(execute(&all, &unknown, &fake_for(&all, false), SENTINEL)
        .unwrap_err()
        .contains("no eval case"));
}

#[test]
fn the_label_summary_gives_accept_rate_per_case_and_the_most_failed_check() {
    let labels = r#"{"version":1,"labels":[
      {"runId":"a/run-1","caseId":"a","decision":"accept","checks":{"legible":true,"balanced":true,"distinct":false},"note":""},
      {"runId":"a/run-2","caseId":"a","decision":"retry","checks":{"legible":false,"balanced":true,"distinct":false},"note":""},
      {"runId":"b/run-1","caseId":"b","decision":"retry","checks":{"legible":false,"balanced":null,"distinct":true},"note":""},
      {"runId":"b/run-2","caseId":"b","decision":null,"checks":{},"note":""}]}"#;
    let text = real_labels::summarize(labels).expect("summary");
    assert!(text.contains("a | 1/2 (50%) | distinct (2x no)"), "{text}");
    assert!(text.contains("b | 0/1 (0%) | legible (1x no)"), "{text}");
    assert!(
        text.contains("ALL | 1/3 (33%) | distinct (2x no)"),
        "{text}"
    );
    assert!(text.contains("1 run(s) had no decision"));
    assert!(real_labels::summarize("{}").is_err());
}

#[test]
fn the_scrubber_removes_the_key_and_key_shaped_text() {
    let scrub = Scrubber::new(SENTINEL);
    let cleaned = scrub.clean(&format!("a {SENTINEL} b sk-ant-other_KEY-123 c"));
    assert_eq!(cleaned, "a [redacted] b [redacted] c");
    assert_eq!(format!("{scrub:?}"), "Scrubber(<redacted>)");
}

#[test]
fn saved_answers_are_keyed_by_run_set_so_the_next_set_starts_blank() {
    let one = review_html(&[], "111-1");
    let two = review_html(&[], "222-2");
    assert!(one.contains("var SET=\"111-1\";") && two.contains("var SET=\"222-2\";"));
    assert_ne!(one, two);
    assert!(one.contains("'opreview:'+SET+':'"));
    assert!(one.contains("runSet:SET"));
}

#[test]
fn every_token_the_model_processed_counts_towards_the_cap() {
    let usage = RunUsage {
        input_tokens: 1,
        cache_creation_input_tokens: 20,
        cache_read_input_tokens: 300,
        output_tokens: 4_000,
        turns: 1,
    };
    assert_eq!(billable_tokens(&usage), 4_321);
    let budget = RunBudget {
        max_run_tokens: 4_321,
        total_remaining: u64::MAX,
    };
    assert!(budget.reached(&usage).is_some());
    let reads_only = RunUsage {
        cache_read_input_tokens: 5_000,
        ..RunUsage::default()
    };
    assert!(
        budget.reached(&reads_only).is_some(),
        "cache reads alone reach the cap"
    );
}
