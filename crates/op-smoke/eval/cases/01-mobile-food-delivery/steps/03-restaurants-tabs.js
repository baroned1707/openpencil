const root = "n1";

const popular = I(root, {
  type: "frame", name: "Popular Near You", layout: "vertical", width: "fill_container",
  gap: 12, padding: [12, 20, 12, 20]
});
I(popular, { type: "text", content: "Popular near you", fontSize: 20, fontWeight: "700", fill: "#111827" });

const restaurants = [
  { name: "Burger Joint", meta: "American, Burgers", rating: "4.8", time: "20-30 min", tint: "#FEE2E2" },
  { name: "Napoli Slice", meta: "Italian, Pizza", rating: "4.6", time: "25-35 min", tint: "#FEF3C7" },
  { name: "Sakura Bar", meta: "Japanese, Sushi", rating: "4.9", time: "30-40 min", tint: "#DCFCE7" }
];
for (const place of restaurants) {
  const card = I(popular, {
    type: "frame", name: "Restaurant Card", layout: "horizontal", width: "fill_container",
    gap: 14, padding: 12, alignItems: "center", cornerRadius: 16, fill: "#FFFFFF",
    stroke: { thickness: 1, fill: [{ type: "solid", color: "#E5E7EB" }] }
  });
  const thumb = I(card, {
    type: "frame", name: "Thumbnail", layout: "horizontal", width: 72, height: 72,
    justifyContent: "center", alignItems: "center", cornerRadius: 12, fill: place.tint
  });
  I(thumb, { type: "icon_font", iconFontName: "utensils", width: 30, height: 30, fill: "#111827" });
  const info = I(card, { type: "frame", name: "Info", layout: "vertical", gap: 4, width: "fill_container" });
  I(info, { type: "text", content: place.name, fontSize: 16, fontWeight: "700", fill: "#111827" });
  I(info, { type: "text", content: place.meta, fontSize: 13, fill: "#4B5563" });
  const facts = I(info, { type: "frame", name: "Facts", layout: "horizontal", gap: 12, alignItems: "center" });
  I(facts, { type: "text", content: place.rating + " rating", fontSize: 13, fontWeight: "600", fill: "#111827" });
  I(facts, { type: "text", content: place.time, fontSize: 13, fill: "#4B5563" });
}

// Bottom tab bar.
const tabs = I(root, {
  type: "frame", name: "Bottom Tab Bar", role: "bottom-nav", layout: "horizontal", width: "fill_container",
  height: 72, justifyContent: "space_around", alignItems: "center", fill: "#FFFFFF",
  stroke: { thickness: 1, fill: [{ type: "solid", color: "#E5E7EB" }] }
});
const items = [
  { label: "Home", icon: "home", active: true },
  { label: "Search", icon: "search", active: false },
  { label: "Orders", icon: "receipt", active: false },
  { label: "Profile", icon: "user", active: false }
];
for (const item of items) {
  const tab = I(tabs, { type: "frame", name: "Tab " + item.label, layout: "vertical", gap: 4, alignItems: "center", width: 64 });
  I(tab, { type: "icon_font", iconFontName: item.icon, width: 24, height: 24, fill: item.active ? "#E11D48" : "#4B5563" });
  I(tab, { type: "text", content: item.label, fontSize: 12, fontWeight: item.active ? "700" : "500", fill: item.active ? "#E11D48" : "#4B5563" });
}
