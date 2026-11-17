//! OpenPencil ships as a purely local app: no account, no sign-in, no
//! collaboration. These tests pin the switch that guarantees it.
//!
//! `local_only_is_the_default_feature` reads the manifest and runs in every
//! configuration, so flipping the default off cannot make the suite vanish; the
//! behaviour tests are gated on the feature so that
//! `--no-default-features` keeps its previous (prebuilt-backed) behaviour.

use op_auth_bridge::{AuthStatus, ENV_DEV_FAKE_LOGIN};

#[test]
fn local_only_is_the_default_feature() {
    let manifest = include_str!("../Cargo.toml");
    let default_line = manifest
        .lines()
        .find(|line| line.trim_start().starts_with("default ="))
        .expect("the crate declares its default features");
    assert!(
        default_line.contains("\"local-only\""),
        "`local-only` must stay a default feature, found: {default_line}"
    );
}

#[cfg(feature = "local-only")]
#[test]
fn local_only_build_reports_collab_unavailable() {
    assert!(std::hint::black_box(op_auth_bridge::LOCAL_ONLY));
    assert!(!op_auth_bridge::available());
    assert!(!op_auth_bridge::collab_ticket_available());
    assert!(!op_auth_bridge::collab_ticket_provider().available());
    assert!(!op_auth_bridge::hosted_access_provider().available());
    assert_eq!(op_auth_bridge::login_begin(), 0);
    assert!(!op_auth_bridge::restore());
    assert_eq!(
        op_auth_bridge::poll(op_auth_bridge::SESSION_HANDLE),
        AuthStatus::Idle
    );
}

#[cfg(feature = "local-only")]
#[test]
fn fake_login_env_is_ignored_in_local_only() {
    assert!(!op_auth_bridge::dev_fake_login_enabled_for(Some("1")));
    assert!(!op_auth_bridge::dev_fake_login_enabled_for(None));

    // The process-environment entry point goes through the same gate. This is
    // the only test in this binary that touches the variable.
    std::env::set_var(ENV_DEV_FAKE_LOGIN, "1");
    let enabled = op_auth_bridge::dev_fake_login_enabled();
    std::env::remove_var(ENV_DEV_FAKE_LOGIN);
    assert!(
        !enabled,
        "OPENPENCIL_DEV_FAKE_LOGIN=1 must not reopen the account UI"
    );
}
