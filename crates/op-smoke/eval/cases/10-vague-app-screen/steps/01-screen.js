// Assumption: a personal planner "Today" screen (no product was named).
const BG = "#FFFFFF";
const screen = I(null, {
  type: "frame", name: "Today", width: 390, height: 844, layout: "vertical", fill: BG, clipContent: true
});

const header = I(screen, { type: "frame", name: "Header", layout: "vertical", width: "fill_container", gap: 4, padding: [12, 20, 12, 20] });
I(header, { type: "text", content: "Thursday, 19 November", fontSize: 14, fill: "#4B5563" });
I(header, { type: "text", content: "Good morning, Sam", fontSize: 28, fontWeight: "700", fill: "#111827" });

const summaryWrap = I(screen, { type: "frame", name: "Summary Wrap", layout: "vertical", width: "fill_container", padding: [8, 20, 8, 20] });
const summary = I(summaryWrap, {
  type: "frame", name: "Summary Card", layout: "vertical", width: "fill_container", gap: 6,
  padding: 20, cornerRadius: 18, fill: "#1D4ED8"
});
I(summary, { type: "text", content: "3 of 5 tasks done", fontSize: 22, fontWeight: "700", fill: "#FFFFFF" });
I(summary, { type: "text", content: "Next up: Design review at 14:00", fontSize: 14, fill: "#DBEAFE" });

const tasks = I(screen, { type: "frame", name: "Tasks", layout: "vertical", width: "fill_container", gap: 10, padding: [12, 20, 12, 20] });
I(tasks, { type: "text", content: "Today's tasks", fontSize: 18, fontWeight: "700", fill: "#111827" });
const items = [
  { title: "Reply to the vendor quote", time: "09:30", done: true },
  { title: "Book flights for the offsite", time: "11:00", done: true },
  { title: "Design review with the team", time: "14:00", done: false },
  { title: "Draft the Q1 plan", time: "16:30", done: false }
];
for (const item of items) {
  const row = I(tasks, {
    type: "frame", name: "Task Row", layout: "horizontal", width: "fill_container", gap: 12,
    alignItems: "center", padding: [14, 16, 14, 16], cornerRadius: 14, fill: "#F3F4F6"
  });
  I(row, { type: "icon_font", iconFontName: item.done ? "circle-check" : "circle", width: 22, height: 22, fill: item.done ? "#15803D" : "#4B5563" });
  I(row, { type: "text", content: item.title, fontSize: 15, fontWeight: "600", width: "fill_container", fill: "#111827" });
  I(row, { type: "text", content: item.time, fontSize: 13, fill: "#4B5563" });
}

const tabs = I(screen, {
  type: "frame", name: "Bottom Tab Bar", role: "bottom-nav", layout: "horizontal", width: "fill_container", height: 72,
  justifyContent: "space_around", alignItems: "center", fill: BG,
  stroke: { thickness: 1, fill: [{ type: "solid", color: "#E5E7EB" }] }
});
for (const [label, icon, active] of [["Today", "calendar-check", true], ["Plan", "list-todo", false], ["Inbox", "inbox", false], ["Me", "user", false]]) {
  const tab = I(tabs, { type: "frame", name: "Tab " + label, layout: "vertical", gap: 4, alignItems: "center", width: 64 });
  I(tab, { type: "icon_font", iconFontName: icon, width: 24, height: 24, fill: active ? "#1D4ED8" : "#4B5563" });
  I(tab, { type: "text", content: label, fontSize: 12, fontWeight: active ? "700" : "500", fill: active ? "#1D4ED8" : "#4B5563" });
}
