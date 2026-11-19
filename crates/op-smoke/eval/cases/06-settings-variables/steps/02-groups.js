const root = "n1";

function group(title) {
  const wrap = I(root, { type: "frame", name: title + " Group", layout: "vertical", width: "fill_container", gap: 8, padding: [12, 20, 4, 20] });
  I(wrap, { type: "text", content: title, fontSize: 13, fontWeight: "700", fill: "$--muted-foreground" });
  return I(wrap, {
    type: "frame", name: title + " Rows", layout: "vertical", width: "fill_container", cornerRadius: 16,
    fill: "$--card", stroke: { thickness: 1, fill: [{ type: "solid", color: "$--border" }] }
  });
}
function toggleRow(parent, label, on) {
  const row = I(parent, {
    type: "frame", name: "Toggle Row", layout: "horizontal", width: "fill_container",
    justifyContent: "space_between", alignItems: "center", padding: [14, 16, 14, 16]
  });
  I(row, { type: "text", content: label, fontSize: 16, fill: "$--foreground" });
  const track = I(row, {
    type: "frame", name: "Switch", layout: "horizontal", width: 48, height: 28, cornerRadius: 14,
    padding: 2, justifyContent: on ? "end" : "start", alignItems: "center", fill: on ? "$--primary" : "$--switch-off"
  });
  I(track, { type: "ellipse", name: "Knob", width: 24, height: 24, fill: "$--primary-foreground" });
}
function linkRow(parent, label, value) {
  const row = I(parent, {
    type: "frame", name: "Link Row", layout: "horizontal", width: "fill_container",
    justifyContent: "space_between", alignItems: "center", padding: [14, 16, 14, 16]
  });
  I(row, { type: "text", content: label, fontSize: 16, fill: "$--foreground" });
  const trailing = I(row, { type: "frame", name: "Trailing", layout: "horizontal", gap: 6, alignItems: "center" });
  I(trailing, { type: "text", content: value, fontSize: 15, fill: "$--muted-foreground" });
  I(trailing, { type: "icon_font", iconFontName: "chevron-right", width: 18, height: 18, fill: "$--muted-foreground" });
}

const notifications = group("Notifications");
toggleRow(notifications, "Push notifications", true);
toggleRow(notifications, "Weekly summary email", false);

const appearance = group("Appearance");
toggleRow(appearance, "Dark mode", false);
linkRow(appearance, "Text size", "Medium");

const privacy = group("Privacy");
linkRow(privacy, "Blocked accounts", "2");
linkRow(privacy, "Data export", "Request");

const footer = I(root, { type: "frame", name: "Footer", layout: "horizontal", width: "fill_container", justifyContent: "center", padding: [20, 20, 32, 20] });
I(footer, { type: "text", content: "Streak 2.4.0", fontSize: 13, fill: "$--muted-foreground" });
