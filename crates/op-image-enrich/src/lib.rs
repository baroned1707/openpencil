//! Shared image-slot enrichment primitives.
//!
//! Two consumers need the exact same slot-detection and write-back semantics:
//!
//! - the desktop host's background `ImageSearchSession` (which resolves slots
//!   on the live canvas as searches complete), and
//! - the headless MCP `enrich_images` tool in `op-host-services` (the
//!   in-process version of `openpencil-desktop --enrich-images`).
//!
//! Both were originally born in `op-host-desktop/src/image_search_session`;
//! they moved here as pure code motion so the dependency direction stays
//! legal (`op-host-services` must never depend on `op-host-desktop`). The
//! desktop session keeps its own provider fetches, memoization, and job
//! bookkeeping and re-exports these primitives under their original paths.
//!
//! `targets` owns the predicate vocabulary: which nodes want an image, what
//! query describes them, and whether the slot's acquisition mode is stock
//! search or AI generation. `apply` owns the write-back: how a resolved url
//! lands on an image node, a placeholder frame, or an empty image-fill
//! container, gated by the collaboration external-assets rule.

mod apply;
mod credits;
mod fallback_policy;
mod found_image;
mod generation_prompt;
mod image_kind;
mod judge_view;
mod license_policy;
#[cfg(feature = "net")]
pub mod net;
#[cfg(feature = "pixels")]
pub mod pixels;
mod targets;

/// User-Agent of every outbound HTTP client that fetches images or image
/// metadata. Wikimedia's policy asks automated clients for a contact URL and
/// throttles anonymous agents harder; the project page is that contact.
pub const HTTP_USER_AGENT: &str = concat!(
    "openpencil-desktop/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/baroned1707/openpencil)"
);

pub use apply::{apply_result, apply_result_with_credit, collaboration_image_result_gate};
pub use credits::{collect_image_credits, format_credits_text, CreditLine};
pub use fallback_policy::{
    apply_image_fallback_policy_to_node, fallback_request_mode, fallback_search_query_for_host,
    icon_name_for_query, image_fallback_policy, image_fallback_policy_with_widths,
    is_failed_image_slot_for_host, is_image_fallback, restore_image_fallback_node,
    ImageFallbackBranch, ImageFallbackPatch, IMAGE_FALLBACK_NAME_SUFFIX,
    SEARCH_FAILED_PLACEHOLDER_SRC,
};
pub use found_image::FoundImage;
pub use generation_prompt::compose_generation_prompt;
pub use image_kind::{ImageKind, VECTOR_QUERY_PREFIX};
pub use judge_view::{ImageJudgeFit, ImageJudgeView};
pub use license_policy::{
    iconify_spdx_policy, license_policy, license_policy_with, provider_license_policy,
    LicensePolicy,
};
pub use op_editor_core::ImageCredit;
pub use targets::{
    collect_targets, collect_targets_with_scene, has_empty_image_fill, image_request_mode,
    is_frame_placeholder_still_unfilled, is_image_area_rectangle_by_heuristic,
    is_unchanged_image_fallback, ImageAspectRatio, ImageRequestMode, ImageSearchTarget,
};
