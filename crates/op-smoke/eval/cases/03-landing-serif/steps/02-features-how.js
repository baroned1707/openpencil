const SERIF = "DM Serif Display";
const page = "n1";

// Features.
const features = I(page, {
  type: "frame", name: "Features", layout: "vertical", width: "fill_container", gap: 48,
  padding: [80, 80, 80, 80], fill: "#F1EBE0"
});
I(features, { type: "text", content: "Why Alder & Finch", fontFamily: SERIF, fontSize: 44, fill: "#1C1917" });
const featureRow = I(features, { type: "frame", name: "Feature Row", layout: "horizontal", width: "fill_container", gap: 32 });
const featureList = [
  { icon: "leaf", title: "Direct trade", body: "We pay farmers twice the commodity price and publish every invoice." },
  { icon: "flame", title: "Roasted to order", body: "Your beans go into the drum on Monday and into the post on Tuesday." },
  { icon: "package", title: "Compostable packaging", body: "Plant-based bags and cartons that go in the home compost bin." }
];
for (const feature of featureList) {
  const card = I(featureRow, { type: "frame", name: "Feature Card", layout: "vertical", width: "fill_container", gap: 12, padding: 28, cornerRadius: 16, fill: "#FAF7F2" });
  I(card, { type: "icon_font", iconFontName: feature.icon, width: 28, height: 28, fill: "#9A3412" });
  I(card, { type: "text", content: feature.title, fontFamily: SERIF, fontSize: 26, fill: "#1C1917" });
  I(card, { type: "text", content: feature.body, fontSize: 16, width: "fill_container", textGrowth: "fixed-width", fill: "#44403C" });
}

// How it works.
const how = I(page, {
  type: "frame", name: "How It Works", layout: "vertical", width: "fill_container", gap: 48,
  padding: [80, 80, 80, 80]
});
I(how, { type: "text", content: "How it works", fontFamily: SERIF, fontSize: 44, fill: "#1C1917" });
const steps = I(how, { type: "frame", name: "Steps", layout: "horizontal", width: "fill_container", gap: 32 });
const stepList = [
  { n: "01", title: "Pick your roast", body: "Choose light, medium or dark and tell us how you brew." },
  { n: "02", title: "Set the rhythm", body: "Weekly, fortnightly or monthly. Skip or pause in one tap." },
  { n: "03", title: "Brew and enjoy", body: "Fresh beans arrive with tasting notes from the roaster." }
];
for (const step of stepList) {
  const column = I(steps, { type: "frame", name: "Step", layout: "vertical", width: "fill_container", gap: 10 });
  I(column, { type: "text", content: step.n, fontFamily: SERIF, fontSize: 40, fill: "#9A3412" });
  I(column, { type: "text", content: step.title, fontSize: 20, fontWeight: "600", fill: "#1C1917" });
  I(column, { type: "text", content: step.body, fontSize: 16, width: "fill_container", textGrowth: "fixed-width", fill: "#44403C" });
}
