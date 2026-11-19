const INK = "#0B2545";
const MUTED = "#334E68";
const ACCENT = "#0A7E8C";

function slide(index, label, headline, bullets, dark) {
  const bg = dark ? INK : "#F7FAFC";
  const fg = dark ? "#FFFFFF" : INK;
  const sub = dark ? "#CBD5E1" : MUTED;
  const frame = I(null, {
    type: "frame", name: "Slide " + index, x: 0, y: (index - 1) * 1180, width: 1920, height: 1080,
    layout: "vertical", gap: 40, padding: [120, 140, 120, 140], fill: bg
  });
  const head = I(frame, { type: "frame", name: "Slide Head", layout: "vertical", width: "fill_container", gap: 20 });
  I(head, { type: "text", content: label, fontSize: 24, fontWeight: "700", letterSpacing: 3, fill: dark ? "#5EEAD4" : ACCENT });
  I(head, { type: "text", content: headline, fontSize: 72, fontWeight: "700", width: "fill_container", textGrowth: "fixed-width", fill: fg });
  const body = I(frame, { type: "frame", name: "Slide Body", layout: "vertical", width: "fill_container", gap: 28 });
  for (const bullet of bullets) {
    I(body, { type: "text", content: bullet, fontSize: 36, width: "fill_container", textGrowth: "fixed-width", fill: sub });
  }
  return frame;
}

slide(4, "TRACTION", "Traction", [
  "Three pilot towns live, 2.1 GWh delivered",
  "Diesel use down 71% in Port Alder",
  "Signed letters of intent from eleven more towns"
], false);
slide(5, "THE ASK", "The ask", [
  "Raising $6M to build 12 installations",
  "Break-even at 9 towns, forecast for Q3 2027",
  "Talk to us: team@tidewater.example"
], true);
