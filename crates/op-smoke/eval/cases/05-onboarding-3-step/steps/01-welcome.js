// Onboarding step 1 of 3.
const screen = I(null, {
  type: "frame", name: "Onboarding 1", x: 0, y: 0, width: 390, height: 844,
  layout: "vertical", fill: "#FFFFFF", clipContent: true
});

const top = I(screen, {
  type: "frame", name: "Top Row", layout: "horizontal", width: "fill_container",
  justifyContent: "space_between", alignItems: "center", padding: [12, 24, 12, 24]
});
I(top, { type: "text", content: "Step 1 of 3", fontSize: 14, fontWeight: "600", fill: "#4B5563" });
I(top, { type: "text", content: "Skip", fontSize: 14, fontWeight: "600", fill: "#1D4ED8" });

const hero = I(screen, {
  type: "frame", name: "Illustration", layout: "vertical", width: "fill_container", height: 340,
  justifyContent: "center", alignItems: "center", fill: "#E0F2FE"
});
I(hero, { type: "icon_font", iconFontName: "sprout", width: 96, height: 96, fill: "#0369A1" });

const copy = I(screen, {
  type: "frame", name: "Copy", layout: "vertical", width: "fill_container", gap: 12,
  padding: [32, 24, 16, 24]
});
I(copy, { type: "text", content: "Welcome to Streak", fontSize: 30, fontWeight: "700", fill: "#111827" });
I(copy, { type: "text", content: "Build small daily habits that stick. Streak keeps score so you can keep going.", fontSize: 16, width: "fill_container", textGrowth: "fixed-width", fill: "#374151" });

const footer = I(screen, {
  type: "frame", name: "Footer", layout: "vertical", width: "fill_container", gap: 20,
  alignItems: "center", padding: [24, 24, 32, 24]
});
const dots = I(footer, { type: "frame", name: "Progress Dots", layout: "horizontal", gap: 8, alignItems: "center" });
for (let index = 1; index <= 3; index++) {
  I(dots, {
    type: "ellipse", name: index === 1 ? "Dot Active" : "Dot",
    width: index === 1 ? 12 : 8, height: index === 1 ? 12 : 8,
    fill: index === 1 ? "#0369A1" : "#D1D5DB"
  });
}
const next = I(footer, {
  type: "frame", name: "Continue Button", layout: "horizontal", width: "fill_container", height: 52,
  justifyContent: "center", alignItems: "center", cornerRadius: 12, fill: "#0369A1"
});
I(next, { type: "text", content: "Continue", fontSize: 16, fontWeight: "700", fill: "#FFFFFF" });
