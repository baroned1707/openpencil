// Onboarding step 2 of 3.
const screen = I(null, {
  type: "frame", name: "Onboarding 2", x: 450, y: 0, width: 390, height: 844,
  layout: "vertical", fill: "#FFFFFF", clipContent: true
});

const top = I(screen, {
  type: "frame", name: "Top Row", layout: "horizontal", width: "fill_container",
  justifyContent: "space_between", alignItems: "center", padding: [12, 24, 12, 24]
});
I(top, { type: "text", content: "Step 2 of 3", fontSize: 14, fontWeight: "600", fill: "#4B5563" });
I(top, { type: "text", content: "Skip", fontSize: 14, fontWeight: "600", fill: "#1D4ED8" });

const hero = I(screen, {
  type: "frame", name: "Illustration", layout: "vertical", width: "fill_container", height: 200,
  justifyContent: "center", alignItems: "center", fill: "#DCFCE7"
});
I(hero, { type: "icon_font", iconFontName: "target", width: 96, height: 96, fill: "#15803D" });

const copy = I(screen, {
  type: "frame", name: "Copy", layout: "vertical", width: "fill_container", gap: 12,
  padding: [32, 24, 16, 24]
});
I(copy, { type: "text", content: "Pick your goals", fontSize: 30, fontWeight: "700", fill: "#111827" });
I(copy, { type: "text", content: "Choose a few habits to start with. You can change them any time.", fontSize: 16, width: "fill_container", textGrowth: "fixed-width", fill: "#374151" });
const goals = I(copy, { type: "frame", name: "Goal Options", layout: "vertical", width: "fill_container", gap: 10 });
for (const goal of ["Drink more water", "Walk 8,000 steps", "Read for 20 minutes"]) {
  const option = I(goals, {
    type: "frame", name: "Goal Option", layout: "horizontal", width: "fill_container", gap: 12,
    alignItems: "center", padding: [12, 14, 12, 14], cornerRadius: 12,
    stroke: { thickness: 1, fill: [{ type: "solid", color: "#D1D5DB" }] }
  });
  I(option, { type: "icon_font", iconFontName: "circle-check", width: 20, height: 20, fill: "#15803D" });
  I(option, { type: "text", content: goal, fontSize: 15, fontWeight: "600", fill: "#111827" });
}

const footer = I(screen, {
  type: "frame", name: "Footer", layout: "vertical", width: "fill_container", gap: 20,
  alignItems: "center", padding: [24, 24, 32, 24]
});
const dots = I(footer, { type: "frame", name: "Progress Dots", layout: "horizontal", gap: 8, alignItems: "center" });
for (let index = 1; index <= 3; index++) {
  I(dots, {
    type: "ellipse", name: index === 2 ? "Dot Active" : "Dot",
    width: index === 2 ? 12 : 8, height: index === 2 ? 12 : 8,
    fill: index === 2 ? "#15803D" : "#D1D5DB"
  });
}
const next = I(footer, {
  type: "frame", name: "Continue Button", layout: "horizontal", width: "fill_container", height: 52,
  justifyContent: "center", alignItems: "center", cornerRadius: 12, fill: "#15803D"
});
I(next, { type: "text", content: "Continue", fontSize: 16, fontWeight: "700", fill: "#FFFFFF" });
