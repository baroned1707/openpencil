//! Proof scene for the README cover: `99-cover-dark` (`OP_PROOF_DIR`).
//!
//! One 1440x900 dark frame of the whole editor: the Layers rail with its page
//! list, a small mobile checkout design built from real nodes, the Design
//! panel for a selected button, the Projects dock with open-file marks, the
//! info bar and the floating chat composer. `scripts/generate-brand-assets.py
//! --cover` turns the PNG into `screenshot/op-cover.png`.
//!
//! Windows-gated like the other tests that solve layout.

#![cfg(all(test, not(target_os = "windows")))]

use super::super::ui3_proof::{editor_host, render_probes, PROOF_H, PROOF_W};
use crate::widget_host::WidgetHostNative;
use op_editor_core::chat::{AgentProvider, ModelEntry};
use op_editor_core::{
    DocumentId, DocumentTabStatus, DocumentTabView, EditorState, LeftPanelTab, NodeId,
    ProjectFileKind, ProjectFileView, ProjectView, ThemeMode,
};
use serde_json::{json, Value};

const INK: &str = "#14161A";
const MUTED: &str = "#6B7280";
const SURFACE: &str = "#F3F4F7";
const ACCENT: &str = "#0D99FF";
const WHITE: &str = "#FFFFFF";

fn solid(color: &str) -> Value {
    json!([{ "type": "solid", "color": color }])
}

fn text(id: &str, content: &str, size: u32, weight: u32, color: &str) -> Value {
    json!({
        "type": "text", "id": id, "name": id, "content": content,
        "fontSize": size, "fontWeight": weight, "fill": solid(color),
        "width": "fit_content", "height": "fit_content"
    })
}

/// A horizontal row with its children spread to both ends.
fn row(id: &str, children: Vec<Value>) -> Value {
    json!({
        "type": "frame", "id": id, "name": id, "layout": "horizontal",
        "width": "fill_container", "height": "fit_content",
        "justifyContent": "space_between", "alignItems": "center",
        "fill": [], "children": children
    })
}

/// A rounded vertical card.
fn card(id: &str, gap: u32, children: Vec<Value>) -> Value {
    json!({
        "type": "frame", "id": id, "name": id, "layout": "vertical", "gap": gap,
        "width": "fill_container", "height": "fit_content", "padding": 16,
        "cornerRadius": 16, "fill": solid(SURFACE), "children": children
    })
}

fn dot(id: &str, color: &str) -> Value {
    json!({
        "type": "ellipse", "id": id, "name": id, "width": 12, "height": 12,
        "fill": solid(color)
    })
}

fn button(id: &str, label: &str, fill: Value, label_color: &str) -> Value {
    json!({
        "type": "frame", "id": id, "name": id, "layout": "horizontal",
        "width": "fill_container", "height": 52, "cornerRadius": 14,
        "justifyContent": "center", "alignItems": "center", "fill": fill,
        "children": [text(&format!("{}-label", id.trim_end_matches("-button")), label, 16, 600, label_color)]
    })
}

fn phone(id: &str, x: u32, children: Vec<Value>) -> Value {
    json!({
        "type": "frame", "id": id, "name": id, "x": x, "y": 0,
        "width": 390, "height": 760, "layout": "vertical", "gap": 16,
        "padding": [64, 20, 24, 20], "cornerRadius": 36, "clipContent": true,
        "fill": solid(WHITE), "children": children
    })
}

fn checkout_screen() -> Value {
    let summary = card(
        "order-summary",
        12,
        vec![
            text("summary-title", "Order summary", 15, 600, INK),
            row(
                "line-sneakers",
                vec![
                    text("sneakers-name", "Canvas sneakers x1", 14, 400, INK),
                    text("sneakers-price", "$64.00", 14, 500, INK),
                ],
            ),
            row(
                "line-socks",
                vec![
                    text("socks-name", "Wool socks x2", 14, 400, INK),
                    text("socks-price", "$18.00", 14, 500, INK),
                ],
            ),
            row(
                "line-shipping",
                vec![
                    text("shipping-name", "Shipping", 14, 400, MUTED),
                    text("shipping-price", "$2.00", 14, 500, MUTED),
                ],
            ),
            row(
                "line-total",
                vec![
                    text("total-name", "Total", 16, 700, INK),
                    text("total-price", "$84.00", 16, 700, INK),
                ],
            ),
        ],
    );
    let address = card(
        "delivery-card",
        6,
        vec![
            text("delivery-label", "Deliver to", 12, 500, MUTED),
            text("delivery-name", "Maya Ortiz", 16, 600, INK),
            text(
                "delivery-street",
                "214 Harbor Street, Portland",
                14,
                400,
                MUTED,
            ),
        ],
    );
    let payment = card(
        "payment-card",
        10,
        vec![
            text("payment-label", "Payment method", 12, 500, MUTED),
            row(
                "payment-row",
                vec![
                    text("payment-card-name", "Visa ending 4242", 15, 600, INK),
                    dot("payment-dot", ACCENT),
                ],
            ),
        ],
    );
    phone(
        "checkout",
        0,
        vec![
            row(
                "checkout-header",
                vec![
                    text("checkout-title", "Checkout", 28, 700, INK),
                    text("checkout-step", "Step 2 of 3", 13, 500, ACCENT),
                ],
            ),
            address,
            summary,
            payment,
            button("pay-button", "Pay $84.00", solid(ACCENT), WHITE),
        ],
    )
}

fn confirmation_screen() -> Value {
    let step = |n: &str, label: &str, done: bool| {
        row(
            &format!("step-{n}"),
            vec![
                text(
                    &format!("step-{n}-label"),
                    label,
                    14,
                    if done { 600 } else { 400 },
                    if done { INK } else { MUTED },
                ),
                dot(
                    &format!("step-{n}-dot"),
                    if done { "#22C55E" } else { "#D1D5DB" },
                ),
            ],
        )
    };
    phone(
        "order-placed",
        450,
        vec![
            json!({
                "type": "frame", "id": "confirm-badge", "name": "confirm-badge",
                "layout": "horizontal", "width": 88, "height": 88, "cornerRadius": 44,
                "justifyContent": "center", "alignItems": "center",
                "fill": solid("#DCFCE7"),
                "children": [text("confirm-mark", "OK", 28, 700, "#15803D")]
            }),
            text("confirm-title", "Order placed", 28, 700, INK),
            text(
                "confirm-note",
                "Arriving Thursday, November 21",
                15,
                400,
                MUTED,
            ),
            card(
                "tracking-card",
                14,
                vec![
                    text("tracking-title", "Tracking", 15, 600, INK),
                    step("1", "Order confirmed", true),
                    step("2", "Packed at the warehouse", true),
                    step("3", "Out for delivery", false),
                    step("4", "Delivered", false),
                ],
            ),
            button("track-button", "Track order", solid(SURFACE), INK),
        ],
    )
}

fn cover_document() -> jian_ops_schema::PenDocument {
    let page = |id: &str, name: &str, children: Vec<Value>| json!({ "id": id, "name": name, "children": children });
    let blank = |id: &str, title: &str| {
        vec![json!({
            "type": "frame", "id": id, "name": title, "x": 0, "y": 0,
            "width": 390, "height": 844, "cornerRadius": 36, "fill": solid(WHITE),
            "children": []
        })]
    };
    let document = json!({
        "version": "1.0.0",
        "children": [],
        "pages": [
            page("page-checkout", "Checkout", vec![checkout_screen(), confirmation_screen()]),
            page("page-cart", "Cart", blank("cart", "cart")),
            page("page-product", "Product detail", blank("product", "product")),
            page("page-account", "Account", blank("account", "account")),
        ]
    });
    jian_ops_schema::load_str(&document.to_string())
        .expect("cover document parses")
        .value
}

fn tab(id: u64, label: &str, dirty: bool, active: bool) -> DocumentTabView {
    DocumentTabView {
        id: DocumentId(id),
        label: label.to_string(),
        project: String::new(),
        dirty,
        status: DocumentTabStatus::Idle,
        active,
    }
}

fn file(root: &str, folder: &str, label: &str, open: Option<u64>) -> ProjectFileView {
    ProjectFileView {
        path: format!("{root}/{folder}/{label}"),
        label: label.to_string(),
        open_id: open.map(DocumentId),
        dirty: false,
        status: DocumentTabStatus::Idle,
        active: false,
        folder: folder.to_string(),
        kind: ProjectFileKind::Design,
        missing: false,
        removable: false,
    }
}

fn project(name: &str, files: Vec<ProjectFileView>, expanded: bool) -> ProjectView {
    ProjectView {
        root: format!("/Users/maya/Work/{name}"),
        name: name.to_string(),
        files,
        expanded,
        ..ProjectView::default()
    }
}

/// The projects of the cover. Open ids follow the order of the tabs below.
fn cover_projects() -> Vec<ProjectView> {
    let storefront_root = "/Users/maya/Work/storefront";
    let mut checkout = file(storefront_root, "mobile", "checkout.op", Some(1));
    checkout.active = true;
    checkout.dirty = true;
    let mut cart = file(storefront_root, "mobile", "cart.op", Some(2));
    cart.dirty = true;
    let storefront = project(
        "storefront",
        vec![
            checkout,
            cart,
            file(storefront_root, "mobile", "product-detail.op", None),
            file(storefront_root, "mobile", "order-status.op", None),
        ],
        true,
    );
    let admin_root = "/Users/maya/Work/admin-dashboard";
    let mut orders = file(admin_root, "reports", "orders.op", Some(3));
    orders.dirty = true;
    let admin = project(
        "admin-dashboard",
        vec![orders, file(admin_root, "reports", "revenue.op", None)],
        true,
    );
    let brand_root = "/Users/maya/Work/brand-kit";
    let brand = project(
        "brand-kit",
        vec![
            file(brand_root, "", "colors.op", None),
            file(brand_root, "", "icons.op", None),
        ],
        false,
    );
    vec![storefront, admin, brand]
}

fn cover_host(mode: ThemeMode) -> WidgetHostNative {
    // `editor_host` builds the host; the state is then swapped for the cover
    // document, which resets the editor UI, so everything below is set after.
    let mut host = editor_host(mode, None);
    assert!(host.replace_editor_state(EditorState::from_document(cover_document())));
    let state = host.editor_state_mut();
    state.editor_ui.home.hide();
    state.editor_ui.theme_mode = mode;
    state.set_single_selection(NodeId::new("pay-button"));
    // Fold the detail rows so the Layers rail reads top-down: the screen,
    // its sections, then the selected button.
    for id in [
        "delivery-card",
        "order-summary",
        "payment-card",
        "order-placed",
    ] {
        state.toggle_node_collapsed(&NodeId::new(id));
    }
    state.chat.available_models = vec![ModelEntry::new(
        AgentProvider::ClaudeCode,
        "claude-sonnet-4-6",
        "Claude Sonnet 4.6",
    )];
    state.chat.selected_model = 0;
    let ui = &mut state.editor_ui;
    ui.slides_panel.tab = LeftPanelTab::Layers;
    ui.document_tabs_supported = true;
    ui.file_name_display = Some("checkout.op".to_string());
    ui.document_dirty = true;
    ui.git_panel.branch = Some("feature/checkout".to_string());
    ui.document_tabs = vec![
        tab(1, "checkout.op", true, true),
        tab(2, "cart.op", true, false),
        tab(3, "orders.op", true, false),
    ];
    ui.projects_panel.projects = cover_projects();
    state
        .chat
        .set_input_text("Add a promo code field under the order summary");
    host.mark_dirty();
    host.zoom_to_fit(PROOF_W, PROOF_H);
    host
}

#[test]
fn ui3_proof_cover_dark() {
    let mut host = cover_host(ThemeMode::Dark);
    let pixels = render_probes(&mut host, "99-cover", &[(700, 450)]);
    assert!(pixels[0] != [0, 0, 0], "the canvas painted something");
}
