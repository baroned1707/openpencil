//! The sign-in modal's two painted blocks: the primary browser action
//! and the security / status note beneath it.
//!
//! Sibling of `login_modal.rs`, split out at the 800-line cap the collab
//! security-boundary check enforces over this crate.

use super::*;

pub(super) fn paint_primary_action(
    backend: &mut dyn RenderBackend,
    theme: &Theme,
    locale: Locale,
    panel: Rect,
    enabled: bool,
    hovered: bool,
    pressed: bool,
) {
    let button = sign_in_rect(panel);
    if !enabled {
        disabled_action::paint(backend, theme, button, t(locale, "settings.account.signIn"));
        return;
    }
    let state_mix = if pressed {
        0.13
    } else if hovered {
        0.08
    } else {
        0.0
    };
    let state_color = if pressed { Color::BLACK } else { Color::WHITE };
    // Flat primary fill; hover lightens and press darkens it.
    backend.fill_round_rect(button, 6.0, mix(theme.primary, state_color, state_mix));

    let content_offset_y = if pressed { 1.0 } else { 0.0 };
    // Web: the plain "Sign in" label — "with browser" is redundant when
    // the editor itself runs in one.
    let label = if cfg!(target_arch = "wasm32") {
        t(locale, "settings.account.signIn")
    } else {
        t(locale, "account.signInWithBrowser")
    };
    let label_size = 13.5;
    let label_weight = 600;
    let label_width = backend.measure_text_weighted(label, label_size, label_weight);
    let icon_size = 17.0;
    let group_width = icon_size + 9.0 + label_width;
    let group_x = button.origin.x + (button.size.x - group_width) / 2.0;
    let icon_y = button.origin.y + (button.size.y - icon_size) / 2.0 + content_offset_y;
    draw_icon(
        backend,
        Icon::Globe,
        Point2D::new(group_x, icon_y),
        icon_size,
        theme.primary_foreground,
        1.7,
    );
    let label_layout = TextLayout::single_run(
        label,
        "system-ui",
        label_size,
        theme.primary_foreground.to_jian(),
        Point2D::ZERO,
    )
    .with_font_weight(label_weight);
    backend.draw_text(
        &label_layout,
        Point2D::new(
            group_x + icon_size + 9.0,
            button.origin.y + button.size.y / 2.0 + 4.5 + content_offset_y,
        ),
    );

    let arrow_size = 15.0;
    draw_icon(
        backend,
        Icon::ArrowRight,
        Point2D::new(
            button.origin.x + button.size.x - 28.0,
            button.origin.y + (button.size.y - arrow_size) / 2.0 + content_offset_y,
        ),
        arrow_size,
        theme.primary_foreground.with_alpha(0.82),
        1.6,
    );
}

pub(super) fn paint_status_note(
    backend: &mut dyn RenderBackend,
    theme: &Theme,
    locale: Locale,
    panel: Rect,
    stub_hint_shown: bool,
    flow_status: Option<op_editor_core::LoginFlowStatus>,
) {
    use op_editor_core::{LoginFlowError, LoginFlowStatus};
    let status = status_rect(panel);
    // The web build runs IN a browser: the approval happens in a popup
    // window, so "waiting for your browser" would read as nonsense there.
    let (waiting_key, approval_key) = if cfg!(target_arch = "wasm32") {
        ("account.openingPopup", "account.approveInPopup")
    } else {
        ("account.waitingForBrowser", "account.waitingForApproval")
    };
    let (icon, text, background, color) = if let Some(flow) = flow_status {
        match flow {
            LoginFlowStatus::WaitingBrowser => (
                Icon::Globe,
                t(locale, waiting_key),
                theme.row_selected_primary,
                theme.foreground,
            ),
            LoginFlowStatus::WaitingApproval => (
                Icon::Globe,
                t(locale, approval_key),
                theme.row_selected_primary,
                theme.foreground,
            ),
            LoginFlowStatus::Exchanging => (
                Icon::Loader,
                t(locale, "account.signingIn"),
                theme.row_selected_primary,
                theme.foreground,
            ),
            LoginFlowStatus::Failed(error) => (
                Icon::AlertTriangle,
                match error {
                    LoginFlowError::Denied => t(locale, "account.signInDenied"),
                    LoginFlowError::Expired => t(locale, "account.signInExpired"),
                    LoginFlowError::Canceled => t(locale, "account.signInCanceled"),
                    LoginFlowError::Unavailable => t(locale, "account.signInFailed"),
                },
                theme.destructive.with_alpha(0.14),
                theme.foreground,
            ),
        }
    } else if stub_hint_shown {
        (
            Icon::Info,
            t(locale, "account.signInComingSoon"),
            theme.row_selected_primary,
            theme.foreground,
        )
    } else {
        (
            Icon::Lock,
            security_note(locale),
            theme.muted.with_alpha(0.72),
            theme.muted_foreground,
        )
    };
    backend.fill_round_rect(status, 6.0, background);

    let font_size = 10.5;
    let icon_size = 13.0;
    let gap = 7.0;
    let text = text_metrics::fit_chrome(
        backend,
        text,
        (status.size.x - 18.0 - icon_size - gap).max(0.0),
        font_size,
    );
    let text_width = text_metrics::measure_chrome(backend, &text, font_size);
    let group_width = icon_size + gap + text_width;
    let group_x = status.origin.x + (status.size.x - group_width) / 2.0;
    draw_icon(
        backend,
        icon,
        Point2D::new(group_x, status.origin.y + (status.size.y - icon_size) / 2.0),
        icon_size,
        if stub_hint_shown {
            theme.primary
        } else {
            color
        },
        1.5,
    );
    let layout = TextLayout::single_run(
        &text,
        "system-ui",
        font_size,
        color.to_jian(),
        Point2D::ZERO,
    );
    backend.draw_text(
        &layout,
        Point2D::new(
            group_x + icon_size + gap,
            status.origin.y + status.size.y / 2.0 + 3.5,
        ),
    );
}
