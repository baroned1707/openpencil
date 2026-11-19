// App shell: fixed sidebar + main column on one dark canvas.
const page = I(null, {
  type: "frame", name: "Agency Dashboard", width: 1440, height: 1024,
  layout: "horizontal", fill: "#0B1220", clipContent: true
});

const sidebar = I(page, {
  type: "frame", name: "Sidebar", layout: "vertical", width: 240, height: "fill_container",
  gap: 8, padding: [24, 16, 24, 16], fill: "#0F1A2E"
});
I(sidebar, { type: "text", content: "Brightline", fontSize: 20, fontWeight: "700", fill: "#F1F5F9" });
const nav = [
  { label: "Overview", icon: "layout-dashboard", active: true },
  { label: "Clients", icon: "users", active: false },
  { label: "Invoices", icon: "credit-card", active: false },
  { label: "Reports", icon: "chart-line", active: false },
  { label: "Settings", icon: "settings", active: false }
];
for (const item of nav) {
  const row = I(sidebar, {
    type: "frame", name: "Nav " + item.label, layout: "horizontal", width: "fill_container",
    gap: 12, alignItems: "center", padding: [10, 12, 10, 12], cornerRadius: 8,
    fill: item.active ? "#1E3A5F" : "#0F1A2E"
  });
  I(row, { type: "icon_font", iconFontName: item.icon, width: 18, height: 18, fill: item.active ? "#F1F5F9" : "#94A3B8" });
  I(row, { type: "text", content: item.label, fontSize: 14, fontWeight: item.active ? "600" : "500", fill: item.active ? "#F1F5F9" : "#94A3B8" });
}

const main = I(page, {
  type: "frame", name: "Main", layout: "vertical", width: "fill_container", height: "fill_container",
  gap: 24, padding: 32
});
const top = I(main, {
  type: "frame", name: "Top Bar", layout: "horizontal", width: "fill_container",
  justifyContent: "space_between", alignItems: "center"
});
const heading = I(top, { type: "frame", name: "Heading", layout: "vertical", gap: 4 });
I(heading, { type: "text", content: "Overview", fontSize: 28, fontWeight: "700", fill: "#F1F5F9" });
I(heading, { type: "text", content: "Performance for the last 30 days", fontSize: 14, fill: "#94A3B8" });
const action = I(top, {
  type: "frame", name: "Export Button", layout: "horizontal", alignItems: "center", gap: 8,
  padding: [10, 16, 10, 16], cornerRadius: 8, fill: "#3B82F6"
});
I(action, { type: "icon_font", iconFontName: "download", width: 16, height: 16, fill: "#0B1220" });
I(action, { type: "text", content: "Export report", fontSize: 14, fontWeight: "600", fill: "#0B1220" });
