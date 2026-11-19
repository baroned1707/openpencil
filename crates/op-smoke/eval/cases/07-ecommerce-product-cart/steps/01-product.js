const screen = I(null, {
  type: "frame", name: "Product Detail", x: 0, y: 0, width: 390, height: 844,
  layout: "vertical", fill: "#FFFFFF", clipContent: true
});

// Hero photo with a label on top of it.
const hero = I(screen, {
  type: "frame", name: "Product Hero", layout: "vertical", width: "fill_container", height: 320,
  padding: 16, fill: [{ type: "image", url: "https://images.example.test/rain-jacket-hero.jpg", mode: "fill" }]
});
const badge = I(hero, { type: "frame", name: "Arrival Badge", layout: "horizontal", padding: [6, 12, 6, 12], cornerRadius: 999, fill: "#FFFFFF" });
I(badge, { type: "text", content: "New arrival", fontSize: 12, fontWeight: "700", fill: "#111827" });
I(hero, { type: "text", content: "Waterproof, windproof, packable", fontSize: 15, fontWeight: "600", fill: "#FFFFFF" });

const info = I(screen, { type: "frame", name: "Product Info", layout: "vertical", width: "fill_container", gap: 10, padding: [20, 20, 12, 20] });
I(info, { type: "text", content: "Trailhead Rain Jacket", fontSize: 24, fontWeight: "700", fill: "#111827" });
const priceRow = I(info, { type: "frame", name: "Price Row", layout: "horizontal", gap: 10, alignItems: "center" });
I(priceRow, { type: "text", content: "$189.00", fontSize: 22, fontWeight: "700", fill: "#111827" });
I(priceRow, { type: "text", content: "4.7 (212 reviews)", fontSize: 14, fill: "#4B5563" });
I(info, {
  type: "text", content: "A three-layer shell with taped seams, a stowable hood and pit zips for steep climbs.",
  fontSize: 15, width: "fill_container", textGrowth: "fixed-width", fill: "#374151"
});

const sizes = I(info, { type: "frame", name: "Size Picker", layout: "vertical", gap: 8 });
I(sizes, { type: "text", content: "Size", fontSize: 14, fontWeight: "600", fill: "#111827" });
const sizeRow = I(sizes, { type: "frame", name: "Size Options", layout: "horizontal", gap: 8 });
for (const size of ["S", "M", "L", "XL"]) {
  const selected = size === "M";
  const chip = I(sizeRow, {
    type: "frame", name: "Size Chip " + size, layout: "horizontal", width: 52, height: 44,
    justifyContent: "center", alignItems: "center", cornerRadius: 10,
    fill: selected ? "#14532D" : "#FFFFFF",
    stroke: { thickness: 1, fill: [{ type: "solid", color: selected ? "#14532D" : "#9CA3AF" }] }
  });
  I(chip, { type: "text", content: size, fontSize: 15, fontWeight: "600", fill: selected ? "#FFFFFF" : "#111827" });
}

const bar = I(screen, {
  type: "frame", name: "Purchase Bar", layout: "horizontal", width: "fill_container", gap: 12,
  alignItems: "center", padding: [12, 20, 28, 20]
});
const favourite = I(bar, {
  type: "frame", name: "Favourite Button", layout: "horizontal", width: 52, height: 52, cornerRadius: 12,
  justifyContent: "center", alignItems: "center", stroke: { thickness: 1, fill: [{ type: "solid", color: "#9CA3AF" }] }
});
I(favourite, { type: "icon_font", iconFontName: "heart", width: 22, height: 22, fill: "#111827" });
const add = I(bar, {
  type: "frame", name: "Add To Cart Button", layout: "horizontal", width: "fill_container", height: 52,
  justifyContent: "center", alignItems: "center", cornerRadius: 12, fill: "#14532D"
});
I(add, { type: "text", content: "Add to cart", fontSize: 16, fontWeight: "700", fill: "#FFFFFF" });
