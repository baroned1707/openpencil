//! Files one recorded run leaves behind, and the screenshots of its result.
//!
//! Screenshots use the SAME call the design agent's `get_screenshot` makes
//! (`execute_agent_tool`, long edge capped at 1568 px by that tool), once per
//! top-level frame of every page.

use std::path::{Path, PathBuf};

use base64::Engine as _;
use op_chat_agent::chat_agent_context::screenshot_image_base64;
use op_editor_core::EditorState;
use op_host_services::design_agent_tools::execute_agent_tool;
use serde_json::{json, Value};

/// One rendered frame.
#[derive(Debug, Clone)]
pub(crate) struct Shot {
    /// Frame name, used in the file name.
    pub name: String,
    pub png: Vec<u8>,
}

/// Longest edge of an image embedded in `review.html`.
pub(crate) const REVIEW_IMAGE_EDGE_PX: u32 = 900;

/// Render every top-level frame of every page of `state` (a clone is used).
/// A frame that cannot be rendered is skipped.
pub(crate) fn render_frames(state: &EditorState) -> Vec<Shot> {
    let mut scratch = state.clone();
    let mut shots = Vec::new();
    let mut used_names: Vec<String> = Vec::new();
    for page in 0..scratch.page_count() {
        scratch.ui.active_page_index = page;
        let roots: Vec<(String, String, bool)> = scratch
            .active_children()
            .iter()
            .map(|node| {
                let value = serde_json::to_value(node).unwrap_or(Value::Null);
                let text = |key: &str| {
                    value
                        .get(key)
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string()
                };
                (text("id"), text("name"), text("type") == "frame")
            })
            .collect();
        let any_frame = roots.iter().any(|(_, _, is_frame)| *is_frame);
        for (id, name, is_frame) in roots {
            if id.is_empty() || (any_frame && !is_frame) {
                continue;
            }
            let args = json!({ "nodeId": id }).to_string();
            let (result, _) = execute_agent_tool(&mut scratch, "get_screenshot", &args);
            let Some(b64) = screenshot_image_base64("get_screenshot", &result) else {
                continue;
            };
            let Ok(png) = base64::engine::general_purpose::STANDARD.decode(b64) else {
                continue;
            };
            let slug = unique_slug(if name.is_empty() { &id } else { &name }, &used_names);
            used_names.push(slug.clone());
            shots.push(Shot { name: slug, png });
        }
    }
    shots
}

/// A file-name-safe lowercase version of `name`, made unique among `used`.
fn unique_slug(name: &str, used: &[String]) -> String {
    let mut slug: String = name
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    slug = slug
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if slug.is_empty() {
        slug = "frame".into();
    }
    slug.truncate(48);
    let base = slug.clone();
    let mut counter = 2;
    while used.contains(&slug) {
        slug = format!("{base}-{counter}");
        counter += 1;
    }
    slug
}

/// `png` shrunk so its long edge is at most `max_edge` (unchanged when it is
/// already smaller or cannot be decoded).
pub(crate) fn shrink_png(png: &[u8], max_edge: u32) -> Vec<u8> {
    let Ok(decoded) = image::load_from_memory_with_format(png, image::ImageFormat::Png) else {
        return png.to_vec();
    };
    let (width, height) = (decoded.width(), decoded.height());
    if width.max(height) <= max_edge {
        return png.to_vec();
    }
    let small = decoded.resize(max_edge, max_edge, image::imageops::FilterType::Triangle);
    let mut out = std::io::Cursor::new(Vec::new());
    match small.write_to(&mut out, image::ImageFormat::Png) {
        Ok(()) => out.into_inner(),
        Err(_) => png.to_vec(),
    }
}

/// `<out>/<case>/run-<n>/`.
pub(crate) fn run_dir(out: &Path, case: &str, run: u32) -> PathBuf {
    out.join(case).join(format!("run-{run}"))
}

pub(crate) fn write_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("create {}: {error}", parent.display()))?;
    }
    std::fs::write(path, bytes).map_err(|error| format!("write {}: {error}", path.display()))
}

/// Scan every file under `dir` for `secret`; the first file that holds it is
/// returned (its path, never the secret).
pub(crate) fn find_secret(dir: &Path, secret: &str) -> Option<PathBuf> {
    if secret.trim().is_empty() {
        return None;
    }
    let needle = secret.trim().as_bytes();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(path) = pending.pop() {
        if path.is_dir() {
            pending.extend(
                std::fs::read_dir(&path)
                    .into_iter()
                    .flatten()
                    .flatten()
                    .map(|entry| entry.path()),
            );
        } else if let Ok(bytes) = std::fs::read(&path) {
            if bytes.windows(needle.len()).any(|window| window == needle) {
                return Some(path);
            }
        }
    }
    None
}
