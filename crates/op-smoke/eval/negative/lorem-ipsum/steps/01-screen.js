const screen = I(null, {
  type: "frame", name: "Profile", width: 390, height: 844, layout: "vertical", fill: "#FFFFFF", clipContent: true
});
const body = I(screen, { type: "frame", name: "Body", layout: "vertical", width: "fill_container", gap: 12, padding: [24, 20, 24, 20] });
I(body, { type: "text", content: "Title", fontSize: 28, fontWeight: "700", fill: "#111827" });
I(body, {
  type: "text", content: "Lorem ipsum dolor sit amet, consectetur adipiscing elit.",
  fontSize: 16, width: "fill_container", textGrowth: "fixed-width", fill: "#374151"
});
I(body, { type: "text", content: "Description", fontSize: 14, fill: "#4B5563" });
