//! Every outbound HTTP client that fetches images or image metadata must send
//! the one contact user-agent. Wikimedia's policy asks automated clients for a
//! contact URL and throttles anonymous agents harder, so a client that falls
//! back to a bare `openpencil-desktop/<ver>` string regresses the rate limit.
//!
//! This is a source scan (cheap, no network): each listed file must pass
//! `HTTP_USER_AGENT` to every `Client::builder()` it contains.

use std::path::{Path, PathBuf};

/// Files that build image / image-metadata HTTP clients, relative to `crates/`.
const IMAGE_CLIENT_FILES: &[&str] = &[
    "op-image-enrich/src/net/fetch.rs",
    "op-image-enrich/src/net/resolve.rs",
    "op-image-enrich/src/net/providers.rs",
    "op-image-enrich/src/net/providers/original_download.rs",
    "op-host-services/src/web_image_generate.rs",
    "op-host-desktop/src/image_panel_host.rs",
    "op-host-desktop/src/remote_image_host.rs",
    "op-host-desktop/src/iconify_host.rs",
    "op-host-desktop/src/image_gen_test_host.rs",
    "op-host-desktop/src/image_generate_host.rs",
];

fn crates_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates dir")
        .to_path_buf()
}

fn source_of(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{path:?}: {e}"))
}

#[test]
fn every_http_client_sends_the_contact_user_agent() {
    assert!(op_image_enrich::HTTP_USER_AGENT.starts_with("openpencil-desktop/"));
    assert!(
        op_image_enrich::HTTP_USER_AGENT.contains("(+https://github.com/baroned1707/openpencil)")
    );
    assert!(
        !op_image_enrich::HTTP_USER_AGENT.contains('@'),
        "the contact is a project URL, never a personal email"
    );
    for relative in IMAGE_CLIENT_FILES {
        let source = source_of(&crates_dir().join(relative));
        let builders = source.matches("Client::builder()").count();
        let agents = source.matches("HTTP_USER_AGENT").count();
        // Files that delegate to the SSRF guard build no client themselves
        // but must still hand it the shared constant.
        assert!(agents > 0, "{relative}: never mentions HTTP_USER_AGENT");
        assert!(
            builders <= agents,
            "{relative}: every Client::builder() must set .user_agent(HTTP_USER_AGENT)"
        );
        assert!(
            !source.contains("concat!(\"openpencil-"),
            "{relative}: ad-hoc user-agent string"
        );
    }
}
