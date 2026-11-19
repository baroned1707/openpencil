const screen = I(null, {
  type: "frame", name: "Cart", x: 450, y: 0, width: 390, height: 844,
  layout: "vertical", fill: "#FFFFFF", clipContent: true
});

const header = I(screen, { type: "frame", name: "Cart Header", layout: "horizontal", width: "fill_container", alignItems: "center", gap: 12, padding: [8, 20, 8, 20] });
I(header, { type: "icon_font", iconFontName: "arrow-left", width: 24, height: 24, fill: "#111827" });
I(header, { type: "text", content: "Your cart", fontSize: 24, fontWeight: "700", fill: "#111827" });

const list = I(screen, { type: "frame", name: "Cart Items", layout: "vertical", width: "fill_container", gap: 14, padding: [12, 20, 12, 20] });
const items = [
  { name: "Trailhead Rain Jacket", meta: "Size M, Forest", price: "$189.00" },
  { name: "Ridgeline Trail Socks", meta: "Pack of 3, L", price: "$27.00" },
  { name: "Summit 20 L Daypack", meta: "Slate", price: "$96.00" }
];
for (const item of items) {
  const row = I(list, {
    type: "frame", name: "Cart Item", layout: "horizontal", width: "fill_container", gap: 14,
    alignItems: "center", padding: 12, cornerRadius: 14,
    stroke: { thickness: 1, fill: [{ type: "solid", color: "#E5E7EB" }] }
  });
  const thumb = I(row, { type: "frame", name: "Item Thumb", layout: "horizontal", width: 64, height: 64, cornerRadius: 10, justifyContent: "center", alignItems: "center", fill: "#ECFDF5" });
  I(thumb, { type: "icon_font", iconFontName: "package", width: 28, height: 28, fill: "#14532D" });
  const text = I(row, { type: "frame", name: "Item Text", layout: "vertical", gap: 4, width: "fill_container" });
  I(text, { type: "text", content: item.name, fontSize: 15, fontWeight: "600", fill: "#111827" });
  I(text, { type: "text", content: item.meta, fontSize: 13, fill: "#4B5563" });
  I(row, { type: "text", content: item.price, fontSize: 15, fontWeight: "700", fill: "#111827" });
}

const summary = I(screen, { type: "frame", name: "Order Summary", layout: "vertical", width: "fill_container", gap: 8, padding: [12, 20, 12, 20] });
const lines = [["Subtotal", "$312.00"], ["Shipping", "Free"], ["Estimated tax", "$24.96"]];
for (const [label, value] of lines) {
  const line = I(summary, { type: "frame", name: "Summary Line", layout: "horizontal", width: "fill_container", justifyContent: "space_between" });
  I(line, { type: "text", content: label, fontSize: 14, fill: "#4B5563" });
  I(line, { type: "text", content: value, fontSize: 14, fill: "#111827" });
}
const total = I(summary, { type: "frame", name: "Total Line", layout: "horizontal", width: "fill_container", justifyContent: "space_between" });
I(total, { type: "text", content: "Total", fontSize: 18, fontWeight: "700", fill: "#111827" });
I(total, { type: "text", content: "$336.96", fontSize: 18, fontWeight: "700", fill: "#111827" });

const checkoutWrap = I(screen, { type: "frame", name: "Checkout Bar", layout: "horizontal", width: "fill_container", padding: [8, 20, 28, 20] });
const checkout = I(checkoutWrap, {
  type: "frame", name: "Checkout Button", layout: "horizontal", width: "fill_container", height: 52,
  justifyContent: "center", alignItems: "center", cornerRadius: 12, fill: "#14532D"
});
I(checkout, { type: "text", content: "Checkout", fontSize: 16, fontWeight: "700", fill: "#FFFFFF" });
