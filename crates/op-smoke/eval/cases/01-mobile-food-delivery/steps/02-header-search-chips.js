const root = "n1";

// Greeting header: address on the left, cart button on the right.
const header = I(root, {
  type: "frame", name: "Header", layout: "horizontal", width: "fill_container",
  justifyContent: "space_between", alignItems: "center", padding: [8, 20, 8, 20]
});
const greeting = I(header, { type: "frame", name: "Greeting", layout: "vertical", gap: 2 });
I(greeting, { type: "text", content: "Deliver to", fontSize: 13, fill: "#4B5563" });
I(greeting, { type: "text", content: "Elm Street 24, Portland", fontSize: 17, fontWeight: "700", fill: "#111827" });
const cart = I(header, {
  type: "frame", name: "Cart Button", layout: "horizontal", width: 44, height: 44,
  justifyContent: "center", alignItems: "center", cornerRadius: 22, fill: "#F3F4F6"
});
I(cart, { type: "icon_font", iconFontName: "shopping-bag", width: 22, height: 22, fill: "#111827" });

// Search field.
const searchWrap = I(root, {
  type: "frame", name: "Search", layout: "horizontal", width: "fill_container", padding: [8, 20, 8, 20]
});
const search = I(searchWrap, {
  type: "frame", name: "Search Field", layout: "horizontal", width: "fill_container", height: 48,
  alignItems: "center", gap: 10, padding: [0, 16, 0, 16], cornerRadius: 14, fill: "#F3F4F6"
});
I(search, { type: "icon_font", iconFontName: "search", width: 20, height: 20, fill: "#4B5563" });
I(search, { type: "text", content: "Search restaurants or dishes", fontSize: 15, fill: "#4B5563" });

// Category chips.
const chips = I(root, {
  type: "frame", name: "Categories", layout: "horizontal", width: "fill_container",
  gap: 10, padding: [8, 20, 8, 20]
});
const categories = ["Burgers", "Pizza", "Sushi", "Salads"];
categories.forEach((label, index) => {
  const selected = index === 0;
  const chip = I(chips, {
    type: "frame", name: "Chip " + label, layout: "horizontal", height: 36, alignItems: "center",
    padding: [0, 16, 0, 16], cornerRadius: 18, fill: selected ? "#E11D48" : "#F3F4F6"
  });
  I(chip, {
    type: "text", content: label, fontSize: 14, fontWeight: "600",
    fill: selected ? "#FFFFFF" : "#374151"
  });
});
