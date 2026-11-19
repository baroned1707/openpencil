// Onboarding step 3 of 3.
const screen = I(null, {
  type: "frame", name: "Onboarding 3", x: 900, y: 0, width: 390, height: 844,
  layout: "vertical", fill: "#FFFFFF", clipContent: true
});

const top = I(screen, {
  type: "frame", name: "Top Row", layout: "horizontal", width: "fill_container",
  justifyContent: "space_between", alignItems: "center", padding: [12, 24, 12, 24]
});
I(top, { type: "text", content: "Step 3 of 3", fontSize: 14, fontWeight: "600", fill: "#4B5563" });
I(top, { type: "text", content: "Skip", fontSize: 14, fontWeight: "600", fill: "#1D4ED8" });

const hero = I(screen, {
  type: "frame", name: "Illustration", layout: "vertical", width: "fill_container", height: 340,
  justifyContent: "center", alignItems: "center", fill: "#EDE9FE"
});
I(hero, { type: "icon_font", iconFontName: "bell-ring", width: 96, height: 96, fill: "#6D28D9" });

const copy = I(screen, {
  type: "frame", name: "Copy", layout: "vertical", width: "fill_container", gap: 12,
  padding: [32, 24, 16, 24]
});
I(copy, { type: "text", content: "Never miss a day", fontSize: 30, fontWeight: "700", fill: "#111827" });
I(copy, { type: "text", content: "A gentle nudge at the times you choose keeps the streak alive.", fontSize: 16, width: "fill_container", textGrowth: "fixed-width", fill: "#374151" });
const times = I(copy, { type: "frame", name: "Reminder Times", layout: "horizontal", gap: 10 });
for (const time of ["08:00", "13:00", "21:00"]) {
  const pill = I(times, { type: "frame", name: "Time Pill", layout: "horizontal", padding: [10, 16, 10, 16], cornerRadius: 999, fill: "#EDE9FE" });
  I(pill, { type: "text", content: time, fontSize: 14, fontWeight: "600", fill: "#5B21B6" });
}

const footer = I(screen, {
  type: "frame", name: "Footer", layout: "vertical", width: "fill_container", gap: 20,
  alignItems: "center", padding: [24, 24, 32, 24]
});
const dots = I(footer, { type: "frame", name: "Progress Dots", layout: "horizontal", gap: 8, alignItems: "center" });
for (let index = 1; index <= 3; index++) {
  I(dots, {
    type: "ellipse", name: index === 3 ? "Dot Active" : "Dot",
    width: index === 3 ? 12 : 8, height: index === 3 ? 12 : 8,
    fill: index === 3 ? "#6D28D9" : "#D1D5DB"
  });
}
const next = I(footer, {
  type: "frame", name: "Continue Button", layout: "horizontal", width: "fill_container", height: 52,
  justifyContent: "center", alignItems: "center", cornerRadius: 12, fill: "#6D28D9"
});
I(next, { type: "text", content: "Continue", fontSize: 16, fontWeight: "700", fill: "#FFFFFF" });
