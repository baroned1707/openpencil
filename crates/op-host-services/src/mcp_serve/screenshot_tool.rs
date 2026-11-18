//! MCP tool `get_screenshot` — renders a node (or the active page's
//! first top-level node when `nodeId` is `"root"`) to a base64 PNG.
//!
//! Part of the Pencil-style agentic tool-loop (Phase 0.3 — purely
//! additive). A design agent calls this after each generation step to
//! visually verify the result before proceeding to the next iteration.
//!
//! Layering note: this tool lives in `op-host-services` (not `op-mcp`)
//! because rendering requires skia, which is only available here. The
//! snapshot constructor takes `&EditorState` and derives the
//! layout-resolved scene via `op_pen_loader::editor_state_to_layout_scene`
//! at registration time, matching the pattern used by other read tools in
//! this crate (e.g. `debug_screenshot`).

use std::collections::BTreeMap;

use base64::Engine as _;
use op_editor_core::EditorState;
use op_editor_ui::layout_scene::LayoutScene;
use op_mcp::{McpTool, ToolErrorCode, ToolOutcome};

use crate::export::{render_node_raster_at_chosen_scale, RasterFormat};

/// Allowed range of the optional `scale` argument. Below the floor the image
/// is too small to read; above the ceiling a large frame costs far more
/// pixels than a visual check needs.
pub(crate) const MIN_SCALE: f32 = 0.25;
pub(crate) const MAX_SCALE: f32 = 2.0;

/// Longest edge, in pixels, of a screenshot taken without an explicit `scale`.
/// Vision models downscale anything larger before they look at it (the
/// documented figure for Claude is about 1568 px on the long edge), so
/// rendering more only costs time and tokens.
pub(crate) const DEFAULT_LONG_EDGE_PX: f32 = 1568.0;
/// Longest edge a screenshot may have even when `scale` asks for more.
pub(crate) const MAX_LONG_EDGE_PX: f32 = 2576.0;

/// Parse the optional `scale` argument: absent or blank is `None` (the size
/// cap picks the scale); anything else must be a finite number in
/// `MIN_SCALE..=MAX_SCALE`.
fn parse_scale(args: &BTreeMap<String, String>) -> Result<Option<f32>, String> {
    let Some(raw) = args
        .get("scale")
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
    else {
        return Ok(None);
    };
    match raw.parse::<f32>() {
        Ok(scale) if scale.is_finite() && (MIN_SCALE..=MAX_SCALE).contains(&scale) => {
            Ok(Some(scale))
        }
        _ => Err(format!(
            "scale must be a number from {MIN_SCALE} to {MAX_SCALE} (got '{raw}'); \
             omit it for 1.0"
        )),
    }
}

/// The scale a screenshot is really drawn at and why.
#[derive(Debug, Clone, Copy, PartialEq)]
struct ScalePlan {
    /// Scale the pixels are produced at.
    used: f32,
    /// Scale the caller asked for (1.0 when it passed none).
    requested: f32,
    /// The size cap lowered `used` below `requested`.
    downscaled: bool,
    /// The cap that applied was the hard maximum, not the default budget.
    hard_cap: bool,
}

/// Pick the scale for a frame of `width` x `height` document px.
///
/// No `scale`: 1.0, lowered so the longest edge is at most
/// [`DEFAULT_LONG_EDGE_PX`]. An explicit `scale` is honoured up to
/// [`MAX_LONG_EDGE_PX`]. A lowered scale is floored to four decimals so the
/// rounded pixel size can never exceed the cap, and it is NOT held to the
/// export floor of 0.5: the reported scale is the one used.
fn plan_scale(requested: Option<f32>, width: f32, height: f32) -> ScalePlan {
    let explicit = requested.is_some();
    let wanted = requested.unwrap_or(1.0);
    let long_edge = width.max(height);
    let cap_px = if explicit {
        MAX_LONG_EDGE_PX
    } else {
        DEFAULT_LONG_EDGE_PX
    };
    // f64 so an exact ratio (1568 / 4000) is not nudged below itself by f32
    // rounding before it is floored.
    let allowed = if long_edge > 0.0 {
        f64::from(cap_px) / f64::from(long_edge)
    } else {
        f64::from(wanted)
    };
    if allowed >= f64::from(wanted) {
        return ScalePlan {
            used: wanted,
            requested: wanted,
            downscaled: false,
            hard_cap: false,
        };
    }
    let floored = ((allowed * 10_000.0 + 1e-6).floor() / 10_000.0) as f32;
    ScalePlan {
        used: floored.max(0.0001),
        requested: wanted,
        downscaled: true,
        hard_cap: explicit,
    }
}

/// One-line advice attached when the size cap lowered the scale.
fn downscale_hint(plan: &ScalePlan, width: f32, height: f32) -> String {
    let frame = format!("{}x{}", width.round() as i64, height.round() as i64);
    if plan.hard_cap {
        format!(
            "frame is {frame}; capped at {} px on the long edge (scale {}); crop to a child node for detail",
            MAX_LONG_EDGE_PX as i64, plan.used
        )
    } else {
        format!(
            "frame is {frame}; shown at {}; crop to a child node or raise scale for detail",
            plan.used
        )
    }
}

/// MCP tool struct — stores the layout-resolved scene (derived at
/// registration time) and the first top-level node id on the active page
/// for resolving the `"root"` alias at call time.
pub struct GetScreenshot {
    /// Layout-resolved scene snapshot — same derive the live canvas uses.
    scene: LayoutScene,
    /// Id of the first top-level node on the active page, used to resolve
    /// the `"root"` alias. `None` when the active page is empty.
    root_node_id: Option<String>,
}

impl McpTool for GetScreenshot {
    fn name(&self) -> &str {
        "get_screenshot"
    }

    fn call(&self, args: &BTreeMap<String, String>) -> ToolOutcome {
        // The browser loads the shipped simple-icons catalog from the daemon,
        // but this exact PNG is rendered server-side. Install the same catalog
        // before painting so brand icon_font nodes do not degrade to the
        // honest fallback dot during agent visual QA.
        let _ = op_editor_ui::set_brand_catalog(crate::web_static::ICONIFY_BRANDS_JSON);

        // Parse nodeId argument.
        let raw_id = match args.get("nodeId").map(String::as_str) {
            Some(id) if !id.trim().is_empty() => id.trim(),
            _ => {
                return ToolOutcome::Err(
                    ToolErrorCode::MissingArgument,
                    "nodeId is required (pass a node id or \"root\")".into(),
                )
            }
        };

        let requested_scale = match parse_scale(args) {
            Ok(scale) => scale,
            Err(message) => return ToolOutcome::Err(ToolErrorCode::InvalidArgument, message),
        };

        // Resolve "root" alias to the first top-level node on the active page.
        let node_id: &str = if raw_id == "root" {
            match &self.root_node_id {
                Some(id) => id.as_str(),
                None => {
                    return ToolOutcome::Err(
                        ToolErrorCode::ToolFailed,
                        "active page is empty — no root node to render".into(),
                    )
                }
            }
        } else {
            raw_id
        };

        // Render to PNG bytes via the shared raster pipeline. The scale is
        // chosen from the frame's real size so the result stays within the
        // vision-model pixel budget; the metadata reports what was used.
        let mut plan = None;
        let mut frame_size = (0.0_f32, 0.0_f32);
        let raster = match render_node_raster_at_chosen_scale(
            &self.scene,
            node_id,
            RasterFormat::Png,
            |width, height| {
                frame_size = (width, height);
                let chosen = plan_scale(requested_scale, width, height);
                plan = Some(chosen);
                chosen.used
            },
        ) {
            Ok(raster) => raster,
            Err(e) => return ToolOutcome::Err(ToolErrorCode::ToolFailed, e.to_string()),
        };
        let plan = plan.expect("the scale chooser runs before any render");

        // Base64-encode using the same engine used elsewhere in this crate
        // (see `export/screenshot.rs` and `export/tests.rs`).
        let image_base64 = base64::engine::general_purpose::STANDARD.encode(&raster.bytes);

        let mut metadata = serde_json::json!({
            "nodeId": node_id,
            "format": "png",
            "scale": raster.scale,
            "requestedScale": plan.requested,
            "width": raster.width_px,
            "height": raster.height_px,
            "size": format!("{}x{}", raster.width_px, raster.height_px),
            // The in-app chat agent reads the PNG from this metadata (it never
            // sees the image block). `op_mcp::tool_response_to_json` drops the
            // field from the wire text block when the image block carries the
            // same bytes, so an MCP client is not sent the PNG twice.
            "image_base64": image_base64,
        });
        if plan.downscaled {
            metadata["hint"] =
                serde_json::Value::String(downscale_hint(&plan, frame_size.0, frame_size.1));
        }
        ToolOutcome::OkImageContent {
            image_base64,
            mime_type: "image/png".into(),
            metadata_json: Some(metadata.to_string()),
        }
    }
}

/// Snapshot constructor — derives the layout scene and resolves the root
/// node id from the live editor state at registration time.
pub fn get_screenshot_snapshot(state: &EditorState) -> GetScreenshot {
    get_screenshot_from_scene(op_pen_loader::editor_state_to_active_page_layout_scene(
        state,
    ))
}

/// Build the tool over an already-derived scene, so a caller that needs the
/// same scene for another tool (the lean profile's export path) derives the
/// layout once.
pub(crate) fn get_screenshot_from_scene(scene: LayoutScene) -> GetScreenshot {
    let root_node_id = scene_root_node_id(&scene);
    GetScreenshot {
        scene,
        root_node_id,
    }
}

/// The node the `"root"` alias names: the active page's first top-level node.
pub(crate) fn scene_root_node_id(scene: &LayoutScene) -> Option<String> {
    scene
        .active_page()
        .and_then(|page| page.children.first())
        .map(|node| node.id.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::test_support::{filled_rect, scene_with};
    use op_editor_ui::layout_scene::{NodeKind, SceneNode};
    use op_editor_ui::{Color, Rect};

    const PNG_MAGIC: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

    fn decode_base64(s: &str) -> Vec<u8> {
        base64::engine::general_purpose::STANDARD
            .decode(s)
            .expect("valid base64")
    }

    /// Build a minimal `GetScreenshot` directly from a hand-crafted
    /// `LayoutScene` to avoid the `EditorState` round-trip in unit tests.
    fn tool_from_scene(scene: LayoutScene) -> GetScreenshot {
        let root_node_id = scene
            .active_page()
            .and_then(|p| p.children.first())
            .map(|n| n.id.clone());
        GetScreenshot {
            scene,
            root_node_id,
        }
    }

    fn call(tool: &GetScreenshot, node_id: &str) -> ToolOutcome {
        let mut args = BTreeMap::new();
        args.insert("nodeId".into(), node_id.into());
        tool.call(&args)
    }

    /// "root" resolves to the first top-level node; returned base64 decodes
    /// to a valid PNG (magic bytes check).
    #[test]
    fn root_alias_renders_first_top_level_node_as_png() {
        let scene = scene_with(vec![filled_rect(
            "n1",
            0.0,
            0.0,
            80.0,
            40.0,
            Color {
                r: 0.2,
                g: 0.4,
                b: 0.8,
                a: 1.0,
            },
        )]);
        let tool = tool_from_scene(scene);
        match call(&tool, "root") {
            ToolOutcome::OkImageContent {
                image_base64,
                mime_type,
                metadata_json,
            } => {
                assert_eq!(mime_type, "image/png");
                assert!(!image_base64.is_empty(), "image_base64 must not be empty");
                let bytes = decode_base64(&image_base64);
                assert_eq!(&bytes[..8], PNG_MAGIC, "must be a PNG payload");
                // Metadata carries nodeId + format + image_base64 (for chat agent path).
                let meta: serde_json::Value =
                    serde_json::from_str(&metadata_json.expect("metadata")).expect("valid JSON");
                assert_eq!(meta["format"], "png");
                assert_eq!(meta["nodeId"], "n1");
                assert!(!meta["image_base64"].as_str().unwrap_or("").is_empty());
            }
            other => panic!("expected OkImageContent, got {other:?}"),
        }
    }

    #[test]
    fn exact_screenshot_loads_the_shipped_brand_catalog_before_painting() {
        let mut icon = SceneNode::leaf("wechat", NodeKind::Other("icon_font".into()));
        icon.bounds = Rect::xywh(0.0, 0.0, 32.0, 32.0);
        icon.text = Some("wechat".into());
        icon.font_family = "simple-icons".into();
        icon.fill = Some(Color::BLACK);
        let tool = tool_from_scene(scene_with(vec![icon]));

        let ToolOutcome::OkImageContent { image_base64, .. } = call(&tool, "root") else {
            panic!("expected exact brand screenshot");
        };
        let bytes = decode_base64(&image_base64);
        let image = skia_safe::Image::from_encoded(skia_safe::Data::new_copy(&bytes))
            .expect("decode screenshot PNG");
        let info = skia_safe::ImageInfo::new(
            (image.width(), image.height()),
            skia_safe::ColorType::RGBA8888,
            skia_safe::AlphaType::Unpremul,
            None,
        );
        let stride = image.width() as usize * 4;
        let mut pixels = vec![0u8; stride * image.height() as usize];
        assert!(image.read_pixels(
            &info,
            pixels.as_mut_slice(),
            stride,
            (0, 0),
            skia_safe::image::CachingHint::Allow,
        ));
        let painted = pixels.chunks_exact(4).filter(|rgba| rgba[3] > 0).count();
        assert!(
            painted > 120,
            "expected the WeChat silhouette rather than the small fallback dot, got {painted} pixels"
        );
    }

    /// Explicit node id works the same way as "root".
    #[test]
    fn explicit_node_id_renders_that_node_as_png() {
        let scene = scene_with(vec![
            filled_rect("r1", 0.0, 0.0, 60.0, 60.0, Color::BLACK),
            filled_rect(
                "r2",
                200.0,
                200.0,
                40.0,
                40.0,
                Color {
                    r: 1.0,
                    g: 0.0,
                    b: 0.0,
                    a: 1.0,
                },
            ),
        ]);
        let tool = tool_from_scene(scene);
        match call(&tool, "r2") {
            ToolOutcome::OkImageContent {
                image_base64,
                mime_type,
                ..
            } => {
                assert_eq!(mime_type, "image/png");
                let bytes = decode_base64(&image_base64);
                assert_eq!(&bytes[..8], PNG_MAGIC, "must be a PNG payload");
            }
            other => panic!("expected OkImageContent, got {other:?}"),
        }
    }

    /// Unknown node id returns a tool-level error (not a JSON-RPC transport error).
    #[test]
    fn unknown_node_id_returns_tool_failed_error() {
        let scene = scene_with(vec![filled_rect("n1", 0.0, 0.0, 10.0, 10.0, Color::BLACK)]);
        let tool = tool_from_scene(scene);
        match call(&tool, "ghost") {
            ToolOutcome::Err(code, msg) => {
                assert_eq!(code, ToolErrorCode::ToolFailed);
                assert!(msg.contains("not found"), "{msg}");
            }
            other => panic!("expected Err, got {other:?}"),
        }
    }

    /// "root" on an empty page returns a clear error.
    #[test]
    fn root_on_empty_page_returns_tool_failed_error() {
        let scene = scene_with(vec![]);
        let tool = tool_from_scene(scene);
        match call(&tool, "root") {
            ToolOutcome::Err(code, msg) => {
                assert_eq!(code, ToolErrorCode::ToolFailed);
                assert!(msg.contains("empty"), "{msg}");
            }
            other => panic!("expected Err, got {other:?}"),
        }
    }

    fn call_with_scale(tool: &GetScreenshot, scale: &str) -> ToolOutcome {
        let mut args = BTreeMap::new();
        args.insert("nodeId".into(), "n1".into());
        args.insert("scale".into(), scale.into());
        tool.call(&args)
    }

    fn png_width(outcome: ToolOutcome) -> u32 {
        let ToolOutcome::OkImageContent { image_base64, .. } = outcome else {
            panic!("expected a screenshot, got {outcome:?}");
        };
        let bytes = decode_base64(&image_base64);
        // PNG IHDR: width is the big-endian u32 at byte 16.
        u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]])
    }

    fn square_tool() -> GetScreenshot {
        tool_from_scene(scene_with(vec![filled_rect(
            "n1",
            0.0,
            0.0,
            80.0,
            80.0,
            Color::BLACK,
        )]))
    }

    /// Invariant: `scale` sizes the image (default 1.0 when absent or blank).
    #[test]
    fn scale_sizes_the_image_and_defaults_to_one() {
        let tool = square_tool();
        let base = png_width(call(&tool, "n1"));
        assert_eq!(png_width(call_with_scale(&tool, "1")), base);
        assert_eq!(png_width(call_with_scale(&tool, "  ")), base);
        assert_eq!(png_width(call_with_scale(&tool, "2")), base * 2);
        assert_eq!(png_width(call_with_scale(&tool, "0.5")), base / 2);
    }

    /// Invariant: a `scale` outside 0.25..=2.0, or not a number, is refused
    /// before any rendering, and the error text states the allowed range.
    #[test]
    fn scale_outside_the_allowed_range_is_refused_with_the_range_in_the_message() {
        let tool = square_tool();
        for bad in ["0.1", "0", "-1", "2.5", "abc", "NaN", "inf"] {
            match call_with_scale(&tool, bad) {
                ToolOutcome::Err(code, message) => {
                    assert_eq!(code, ToolErrorCode::InvalidArgument, "{bad}");
                    assert!(
                        message.contains("0.25") && message.contains('2'),
                        "{message}"
                    );
                    assert!(message.contains(bad), "{message}");
                }
                other => panic!("scale {bad:?} must be refused, got {other:?}"),
            }
        }
        for good in ["0.25", "2", "2.0", "1.5"] {
            assert!(
                matches!(
                    call_with_scale(&tool, good),
                    ToolOutcome::OkImageContent { .. }
                ),
                "scale {good} is allowed"
            );
        }
    }

    /// Invariant: on the MCP wire the PNG travels once, in the image block;
    /// the text block keeps the metadata but not a second copy of the pixels.
    #[test]
    fn mcp_wire_response_does_not_repeat_the_png_in_the_text_block() {
        let tool = square_tool();
        let mut registry = op_mcp::ToolRegistry::default();
        registry.register(Box::new(tool));
        let mut arguments = BTreeMap::new();
        arguments.insert("nodeId".to_string(), "n1".to_string());
        let response = registry.dispatch(op_mcp::ToolCall {
            id: op_mcp::RequestId::Num(1),
            tool: "get_screenshot".into(),
            arguments,
        });
        let op_mcp::ToolResponse::Ok { json, image, .. } = &response else {
            panic!("expected Ok, got {response:?}");
        };
        let png = image.as_ref().expect("image block").data.clone();
        // The chat agent's path reads the metadata JSON, which keeps the PNG.
        let metadata: serde_json::Value =
            serde_json::from_str(json.as_deref().expect("metadata")).expect("json");
        assert_eq!(metadata["image_base64"], png.as_str());

        let wire_text = op_mcp::tool_response_to_json(&response);
        let wire: serde_json::Value = serde_json::from_str(&wire_text).expect("wire json");
        let blocks = wire["result"]["content"].as_array().expect("content");
        assert_eq!(blocks[0]["type"], "image");
        assert_eq!(blocks[0]["data"], png.as_str());
        let text: serde_json::Value =
            serde_json::from_str(blocks[1]["text"].as_str().expect("text")).expect("text json");
        assert!(text.get("image_base64").is_none(), "{text}");
        assert_eq!(text["format"], "png");
        assert_eq!(text["nodeId"], "n1");
        assert_eq!(
            wire_text.matches(&png).count(),
            1,
            "the PNG appears exactly once on the wire"
        );
    }

    /// Missing nodeId argument returns MissingArgument.
    #[test]
    fn missing_node_id_returns_missing_argument_error() {
        let scene = scene_with(vec![filled_rect("n1", 0.0, 0.0, 10.0, 10.0, Color::BLACK)]);
        let tool = tool_from_scene(scene);
        let args: BTreeMap<String, String> = BTreeMap::new();
        match tool.call(&args) {
            ToolOutcome::Err(code, _) => assert_eq!(code, ToolErrorCode::MissingArgument),
            other => panic!("expected MissingArgument, got {other:?}"),
        }
    }
}

#[cfg(test)]
#[path = "screenshot_tool_tests.rs"]
mod size_cap_tests;
