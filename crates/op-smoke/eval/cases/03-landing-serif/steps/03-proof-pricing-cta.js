const SERIF = "DM Serif Display";
const page = "n1";

// Testimonials.
const proof = I(page, {
  type: "frame", name: "Testimonials", layout: "vertical", width: "fill_container", gap: 40,
  padding: [80, 80, 80, 80], fill: "#1C1917"
});
I(proof, { type: "text", content: "What customers say", fontFamily: SERIF, fontSize: 44, fill: "#FAF7F2" });
const quotes = I(proof, { type: "frame", name: "Quotes", layout: "horizontal", width: "fill_container", gap: 32 });
const quoteList = [
  { quote: "The Ethiopian Guji is the best filter coffee I have had at home.", who: "Marta, Seattle" },
  { quote: "Beans arrive within days of roasting and the notes card is a lovely touch.", who: "Daniel, Austin" },
  { quote: "Pausing a delivery before a trip took ten seconds. Rare for a subscription.", who: "Priya, Denver" }
];
for (const item of quoteList) {
  const card = I(quotes, { type: "frame", name: "Quote Card", layout: "vertical", width: "fill_container", gap: 20, padding: 28, cornerRadius: 16, fill: "#2A2522" });
  I(card, { type: "text", content: item.quote, fontFamily: SERIF, fontSize: 22, width: "fill_container", textGrowth: "fixed-width", fill: "#FAF7F2" });
  I(card, { type: "text", content: item.who, fontSize: 14, fill: "#D6D3D1" });
}

// Pricing.
const pricing = I(page, {
  type: "frame", name: "Pricing", layout: "vertical", width: "fill_container", gap: 40,
  padding: [80, 80, 80, 80]
});
I(pricing, { type: "text", content: "Simple pricing", fontFamily: SERIF, fontSize: 44, fill: "#1C1917" });
const plans = I(pricing, { type: "frame", name: "Plans", layout: "horizontal", width: "fill_container", gap: 24 });
const planList = [
  { name: "Taster", price: "$14", per: "per box, 250 g", perks: "One origin, rotating monthly" },
  { name: "Regular", price: "$26", per: "per box, 500 g", perks: "Two origins, free shipping" },
  { name: "Household", price: "$48", per: "per box, 1 kg", perks: "Three origins, free shipping, gift card" }
];
for (const plan of planList) {
  const card = I(plans, {
    type: "frame", name: "Plan Card", layout: "vertical", width: "fill_container", gap: 10, padding: 28,
    cornerRadius: 16, stroke: { thickness: 1, fill: [{ type: "solid", color: "#D6D3D1" }] }
  });
  I(card, { type: "text", content: plan.name, fontSize: 18, fontWeight: "600", fill: "#1C1917" });
  I(card, { type: "text", content: plan.price, fontFamily: SERIF, fontSize: 52, fill: "#9A3412" });
  I(card, { type: "text", content: plan.per, fontSize: 14, fill: "#57534E" });
  I(card, { type: "text", content: plan.perks, fontSize: 15, fill: "#44403C" });
}

// Closing call to action.
const closing = I(page, {
  type: "frame", name: "Closing Call To Action", layout: "vertical", width: "fill_container", gap: 20,
  alignItems: "center", padding: [96, 160, 96, 160], fill: "#9A3412"
});
I(closing, { type: "text", content: "Your next great cup is four days away.", fontFamily: SERIF, fontSize: 48, textAlign: "center", fill: "#FFFFFF" });
const closingButton = I(closing, { type: "frame", name: "Closing Button", layout: "horizontal", padding: [16, 32, 16, 32], cornerRadius: 999, fill: "#FAF7F2" });
I(closingButton, { type: "text", content: "Start your first box", fontSize: 16, fontWeight: "600", fill: "#1C1917" });
