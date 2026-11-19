const main = "n19";
const CARD = "#111A2E";
const BORDER = { thickness: 1, fill: [{ type: "solid", color: "#1E293B" }] };

// KPI row.
const kpis = I(main, { type: "frame", name: "KPI Row", layout: "horizontal", width: "fill_container", gap: 16 });
const metrics = [
  { label: "Revenue", value: "$482,300", delta: "+12.4% vs last month", good: true },
  { label: "Active clients", value: "128", delta: "+6 this month", good: true },
  { label: "Open invoices", value: "$38,940", delta: "9 overdue", good: false },
  { label: "Avg. retainer", value: "$3,768", delta: "+3.1% vs last month", good: true }
];
for (const metric of metrics) {
  const card = I(kpis, {
    type: "frame", name: "KPI Card", layout: "vertical", width: "fill_container", gap: 8,
    padding: 20, cornerRadius: 12, fill: CARD, stroke: BORDER
  });
  I(card, { type: "text", content: metric.label, fontSize: 13, fontWeight: "500", fill: "#94A3B8" });
  I(card, { type: "text", content: metric.value, fontSize: 28, fontWeight: "700", fill: "#F1F5F9" });
  I(card, { type: "text", content: metric.delta, fontSize: 13, fontWeight: "600", fill: metric.good ? "#4ADE80" : "#FCA5A5" });
}

// Revenue chart: one bar per month, tallest = 100%.
const chart = I(main, {
  type: "frame", name: "Revenue Chart", layout: "vertical", width: "fill_container", gap: 20,
  padding: 24, cornerRadius: 12, fill: CARD, stroke: BORDER
});
const chartHead = I(chart, { type: "frame", name: "Chart Header", layout: "horizontal", width: "fill_container", justifyContent: "space_between", alignItems: "center" });
I(chartHead, { type: "text", content: "Revenue by month", fontSize: 18, fontWeight: "600", fill: "#F1F5F9" });
I(chartHead, { type: "text", content: "Last 12 months", fontSize: 13, fill: "#94A3B8" });
const months = [
  ["Dec", 62], ["Jan", 70], ["Feb", 66], ["Mar", 78], ["Apr", 84], ["May", 80],
  ["Jun", 91], ["Jul", 88], ["Aug", 96], ["Sep", 102], ["Oct", 110], ["Nov", 118]
];
const bars = I(chart, { type: "frame", name: "Bars", layout: "horizontal", width: "fill_container", height: 170, gap: 18, alignItems: "end" });
for (const [month, value] of months) {
  const column = I(bars, { type: "frame", name: "Bar " + month, layout: "vertical", width: "fill_container", gap: 8, alignItems: "center", justifyContent: "end" });
  const fillBar = I(column, {
    type: "frame", name: "Bar Fill", layout: "vertical", width: 36, height: Math.round(value * 1.2),
    alignItems: "center", padding: [6, 0, 0, 0], cornerRadius: 6, fill: "#3B82F6"
  });
  I(fillBar, { type: "text", content: String(value), fontSize: 11, fontWeight: "600", fill: "#FFFFFF" });
  I(column, { type: "text", content: month, fontSize: 12, fill: "#94A3B8" });
}
