const SERIF = "DM Serif Display";
const page = I(null, {
  type: "frame", name: "Alder & Finch Landing", width: 1440, height: "fit_content",
  layout: "vertical", fill: "#FAF7F2"
});

// Navigation: logo | links | call to action.
const nav = I(page, {
  type: "frame", name: "Navigation", layout: "horizontal", width: "fill_container",
  justifyContent: "space_between", alignItems: "center", padding: [24, 80, 24, 80]
});
I(nav, { type: "text", content: "Alder & Finch", fontFamily: SERIF, fontSize: 26, fill: "#1C1917" });
const links = I(nav, { type: "frame", name: "Links", layout: "horizontal", gap: 32, alignItems: "center" });
for (const label of ["Coffee", "Subscriptions", "Journal", "Visit"]) {
  I(links, { type: "text", content: label, fontSize: 15, fill: "#44403C" });
}
const cta = I(nav, {
  type: "frame", name: "Nav Button", layout: "horizontal", alignItems: "center",
  padding: [12, 22, 12, 22], cornerRadius: 999, fill: "#1C1917"
});
I(cta, { type: "text", content: "Shop beans", fontSize: 15, fontWeight: "600", fill: "#FAF7F2" });

// Hero.
const hero = I(page, {
  type: "frame", name: "Hero", layout: "vertical", width: "fill_container", gap: 24,
  alignItems: "center", padding: [96, 160, 96, 160]
});
I(hero, { type: "text", content: "SMALL-BATCH COFFEE, SINCE 2014", fontSize: 13, fontWeight: "600", letterSpacing: 2, fill: "#9A3412" });
I(hero, {
  type: "text", content: "Roasted slowly, delivered while it is still alive.",
  fontFamily: SERIF, fontSize: 72, textAlign: "center", width: 960, textGrowth: "fixed-width", fill: "#1C1917"
});
I(hero, {
  type: "text", content: "Single-origin beans from farms we visit every year, roasted to order in Portland and at your door within four days.",
  fontSize: 19, textAlign: "center", width: 640, textGrowth: "fixed-width", fill: "#44403C"
});
const heroActions = I(hero, { type: "frame", name: "Hero Actions", layout: "horizontal", gap: 16 });
const primary = I(heroActions, { type: "frame", name: "Primary Button", layout: "horizontal", padding: [16, 32, 16, 32], cornerRadius: 999, fill: "#9A3412" });
I(primary, { type: "text", content: "Start your first box", fontSize: 16, fontWeight: "600", fill: "#FFFFFF" });
const secondary = I(heroActions, {
  type: "frame", name: "Secondary Button", layout: "horizontal", padding: [16, 32, 16, 32], cornerRadius: 999,
  stroke: { thickness: 1, fill: [{ type: "solid", color: "#1C1917" }] }
});
I(secondary, { type: "text", content: "Browse the roasts", fontSize: 16, fontWeight: "600", fill: "#1C1917" });
