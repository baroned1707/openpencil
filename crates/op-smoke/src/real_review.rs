//! `review.html`: ONE self-contained static page the owner judges the runs on.
//!
//! Rules (the owner's review-page rules): Vietnamese labels, one card per
//! run, the run's input (prompt, seed screenshot) on the left and its output
//! (screenshots) on the right, stacked on a phone, one to three things to
//! check, Accept / Retry and a note per card, current run set only, every run
//! shown (failed and skipped with a one-line reason), no intro or how-to text,
//! no file paths. No external request of any kind: CSS and JS are inline and
//! images are data URIs.
//!
//! Choices live in `localStorage` keyed by run id; "Xuất nhãn" downloads
//! `labels.json`. Every piece of text from a run goes through [`escape`].

use base64::Engine as _;

use super::real_artifacts::{shrink_png, Shot, REVIEW_IMAGE_EDGE_PX};
use super::{RunRecord, RunStatus};

/// The checks shown on a card: (key in labels.json, label).
const COMMON_CHECKS: [(&str, &str); 2] = [
    ("legible", "Đọc được chữ và đủ tương phản?"),
    ("balanced", "Bố cục cân đối, không bị cắt?"),
];
const NEW_DESIGN_CHECK: (&str, &str) = (
    "distinct",
    "Có nét riêng cho sản phẩm này, không chung chung?",
);
const MODIFY_CHECK: (&str, &str) = (
    "scoped",
    "Chỉ phần được yêu cầu thay đổi, phần còn lại giữ nguyên?",
);

/// HTML-escape text from a run (prompts, reasons, names).
pub(crate) fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            other => out.push(other),
        }
    }
    out
}

fn data_uri(png: &[u8]) -> String {
    format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(shrink_png(png, REVIEW_IMAGE_EDGE_PX))
    )
}

fn images(shots: &[Shot]) -> String {
    shots
        .iter()
        .map(|shot| {
            format!(
                "<img src=\"{}\" alt=\"{}\">",
                data_uri(&shot.png),
                escape(&shot.name)
            )
        })
        .collect()
}

fn checks_for(record: &RunRecord) -> Vec<(&'static str, &'static str)> {
    let mut checks = COMMON_CHECKS.to_vec();
    checks.push(if record.modify {
        MODIFY_CHECK
    } else {
        NEW_DESIGN_CHECK
    });
    checks
}

fn status_line(status: &RunStatus) -> String {
    let prefix = match status {
        RunStatus::Completed => return String::new(),
        RunStatus::StoppedAtCap(_) => "Dừng do giới hạn token",
        RunStatus::Failed(_) => "Thất bại",
        RunStatus::Skipped(_) => "Bỏ qua",
    };
    let reason = status.reason().unwrap_or_default();
    // One line: a long or multi-line reason is cut.
    let one_line: String = reason
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(240)
        .collect();
    format!(
        "<p class=\"status\" data-status=\"{}\">{prefix}: {}</p>",
        status.label(),
        escape(&one_line)
    )
}

fn card(record: &RunRecord) -> String {
    let run_id = record.run_id();
    let mut input = format!("<p class=\"prompt\">{}</p>", escape(&record.prompt));
    if record.modify && !record.seed_shots.is_empty() {
        input.push_str(&format!(
            "<div class=\"shots\"><p class=\"cap\">Bản gốc</p>{}</div>",
            images(&record.seed_shots)
        ));
    }
    let output = if record.shots.is_empty() {
        String::new()
    } else {
        format!("<div class=\"shots\">{}</div>", images(&record.shots))
    };
    let checks: String = checks_for(record)
        .iter()
        .map(|(key, label)| {
            format!(
                "<div class=\"check\"><span>{label}</span>\
                 <button type=\"button\" data-check=\"{key}\" data-value=\"yes\" aria-pressed=\"false\">Có</button>\
                 <button type=\"button\" data-check=\"{key}\" data-value=\"no\" aria-pressed=\"false\">Không</button></div>"
            )
        })
        .collect();
    format!(
        "<section class=\"card\" data-run-id=\"{id}\" data-case-id=\"{case}\">\
         <h2>{case} · lần {run}</h2>\
         <div class=\"cols\"><div class=\"in\"><h3>Yêu cầu</h3>{input}</div>\
         <div class=\"out\"><h3>Kết quả</h3>{status}{output}</div></div>\
         <div class=\"checks\">{checks}</div>\
         <div class=\"decide\">\
         <button type=\"button\" data-decide=\"accept\" aria-pressed=\"false\">Chấp nhận</button>\
         <button type=\"button\" data-decide=\"retry\" aria-pressed=\"false\">Làm lại</button></div>\
         <textarea rows=\"2\" placeholder=\"Ghi chú\" aria-label=\"Ghi chú\"></textarea>\
         </section>",
        id = escape(&run_id),
        case = escape(&record.case_id),
        run = record.run,
        status = status_line(&record.status),
    )
}

const STYLE: &str = "\
:root{color-scheme:light dark;--bg:#f6f6f7;--fg:#16181d;--muted:#5d6470;--card:#fff;--line:#d9dce2;--accent:#2f6fed;--ok:#1f8a4c;--bad:#c0392b}\
@media (prefers-color-scheme:dark){:root{--bg:#101215;--fg:#e8eaee;--muted:#9aa1ad;--card:#1a1d22;--line:#2d323a;--accent:#6b9bff;--ok:#4cc380;--bad:#ff7a6b}}\
*{box-sizing:border-box}\
body{margin:0;padding:16px;background:var(--bg);color:var(--fg);font:15px/1.5 system-ui,-apple-system,Segoe UI,Roboto,sans-serif;overflow-wrap:anywhere}\
.bar{display:flex;justify-content:flex-end;margin:0 0 16px}\
.card{background:var(--card);border:1px solid var(--line);border-radius:10px;padding:16px;margin:0 0 16px}\
h2{font-size:17px;margin:0 0 12px}h3{font-size:12px;letter-spacing:.04em;text-transform:uppercase;color:var(--muted);margin:0 0 8px}\
.cols{display:grid;grid-template-columns:minmax(0,1fr) minmax(0,2fr);gap:16px}\
@media (max-width:720px){.cols{grid-template-columns:minmax(0,1fr)}.check span{flex-basis:100%}}\
.prompt{margin:0 0 12px;white-space:pre-wrap}\
.shots img{display:block;width:100%;height:auto;border:1px solid var(--line);border-radius:6px;margin:0 0 12px;background:#fff}\
.cap{margin:0 0 6px;color:var(--muted);font-size:13px}\
.status{margin:0 0 12px;color:var(--bad)}.status[data-status=stopped-at-cap],.status[data-status=skipped]{color:var(--muted)}\
.checks{margin:16px 0 0}.check{display:flex;flex-wrap:wrap;align-items:center;gap:8px;margin:0 0 8px}.check span{flex:1 1 220px}\
.decide{display:flex;flex-wrap:wrap;gap:8px;margin:12px 0}\
button{font:inherit;color:var(--fg);background:transparent;border:1px solid var(--line);border-radius:6px;padding:6px 14px;cursor:pointer}\
button[aria-pressed=true]{background:var(--accent);border-color:var(--accent);color:#fff}\
button[data-decide=accept][aria-pressed=true]{background:var(--ok);border-color:var(--ok)}\
button[data-decide=retry][aria-pressed=true]{background:var(--bad);border-color:var(--bad)}\
textarea{display:block;width:100%;font:inherit;color:var(--fg);background:transparent;border:1px solid var(--line);border-radius:6px;padding:8px;resize:vertical}";

const SCRIPT: &str = "\
(function(){\
var SET=__SET__;var PREFIX='opreview:'+SET+':';\
function load(id){try{return JSON.parse(localStorage.getItem(PREFIX+id))||{}}catch(e){return {}}}\
function save(id,s){try{localStorage.setItem(PREFIX+id,JSON.stringify(s))}catch(e){}}\
var cards=[].slice.call(document.querySelectorAll('.card'));\
cards.forEach(function(card){\
var id=card.getAttribute('data-run-id'),s=load(id);s.checks=s.checks||{};\
var note=card.querySelector('textarea');\
function paint(){\
[].forEach.call(card.querySelectorAll('[data-decide]'),function(b){b.setAttribute('aria-pressed',String(b.getAttribute('data-decide')===s.decision))});\
[].forEach.call(card.querySelectorAll('[data-check]'),function(b){b.setAttribute('aria-pressed',String(s.checks[b.getAttribute('data-check')]===(b.getAttribute('data-value')==='yes')))});\
note.value=s.note||''}\
card.addEventListener('click',function(e){var b=e.target.closest('button');if(!b)return;\
if(b.hasAttribute('data-decide')){s.decision=b.getAttribute('data-decide')}\
else if(b.hasAttribute('data-check')){s.checks[b.getAttribute('data-check')]=b.getAttribute('data-value')==='yes'}\
save(id,s);paint()});\
note.addEventListener('input',function(){s.note=note.value;save(id,s)});\
paint()});\
document.getElementById('export').addEventListener('click',function(){\
var labels=cards.map(function(card){var id=card.getAttribute('data-run-id'),s=load(id),checks={};\
[].forEach.call(card.querySelectorAll('[data-check]'),function(b){var k=b.getAttribute('data-check');\
checks[k]=(s.checks&&typeof s.checks[k]==='boolean')?s.checks[k]:null});\
return {runId:id,caseId:card.getAttribute('data-case-id'),decision:s.decision||null,checks:checks,note:s.note||''}});\
var blob=new Blob([JSON.stringify({version:1,runSet:SET,labels:labels},null,2)],{type:'application/json'});\
var a=document.createElement('a');a.href=URL.createObjectURL(blob);a.download='labels.json';\
document.body.appendChild(a);a.click();document.body.removeChild(a)});\
})();";

/// `set_id` names this run set; choices saved in the browser are keyed by it, so
/// the next run set (same run ids) never shows this one's answers.
pub(crate) fn review_html(records: &[RunRecord], set_id: &str) -> String {
    let cards: String = records.iter().map(card).collect();
    format!(
        "<!doctype html><html lang=\"vi\"><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
         <title>Đánh giá</title><style>{STYLE}</style></head><body>\
         <div class=\"bar\"><button type=\"button\" id=\"export\">Xuất nhãn</button></div>\
         {cards}<script>{script}</script></body></html>",
        script = SCRIPT.replace(
            "__SET__",
            &format!("{:?}", set_id.replace(['<', '>', '&'], ""))
        )
    )
}
