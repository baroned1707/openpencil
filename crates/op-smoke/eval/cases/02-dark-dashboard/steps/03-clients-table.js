const main = "n19";
const CARD = "#111A2E";

const table = I(main, {
  type: "frame", name: "Clients Table", layout: "vertical", width: "fill_container", gap: 0,
  padding: 24, cornerRadius: 12, fill: CARD, stroke: { thickness: 1, fill: [{ type: "solid", color: "#1E293B" }] }
});
I(table, { type: "text", content: "Recent clients", fontSize: 18, fontWeight: "600", fill: "#F1F5F9" });

const columns = [
  { key: "client", label: "Client", width: "fill_container" },
  { key: "plan", label: "Plan", width: 140 },
  { key: "status", label: "Status", width: 120 },
  { key: "mrr", label: "MRR", width: 120 },
  { key: "invoice", label: "Last invoice", width: 140 }
];
const rows = [
  { client: "Northwind Traders", plan: "Growth", status: "Active", mrr: "$4,200", invoice: "Nov 02" },
  { client: "Fabrikam Studio", plan: "Starter", status: "Active", mrr: "$1,150", invoice: "Nov 04" },
  { client: "Contoso Health", plan: "Scale", status: "Overdue", mrr: "$7,800", invoice: "Oct 18" },
  { client: "Adventure Works", plan: "Growth", status: "Active", mrr: "$3,900", invoice: "Nov 06" },
  { client: "Wingtip Toys", plan: "Starter", status: "Paused", mrr: "$950", invoice: "Sep 29" },
  { client: "Tailspin Air", plan: "Scale", status: "Active", mrr: "$8,640", invoice: "Nov 09" },
  { client: "Litware Labs", plan: "Growth", status: "Overdue", mrr: "$3,410", invoice: "Oct 21" },
  { client: "Proseware Legal", plan: "Starter", status: "Active", mrr: "$1,380", invoice: "Nov 11" }
];

const headerRow = I(table, { type: "frame", name: "Table Header", layout: "horizontal", width: "fill_container", padding: [14, 0, 14, 0] });
for (const column of columns) {
  const cell = I(headerRow, { type: "frame", name: "Header Cell", layout: "horizontal", width: column.width });
  I(cell, { type: "text", content: column.label, fontSize: 12, fontWeight: "600", fill: "#94A3B8" });
}
const statusColor = { Active: "#4ADE80", Overdue: "#FCA5A5", Paused: "#FCD34D" };
for (const row of rows) {
  const line = I(table, {
    type: "frame", name: "Table Row", layout: "horizontal", width: "fill_container", alignItems: "center",
    padding: [14, 0, 14, 0], stroke: { thickness: { bottom: 1 }, fill: [{ type: "solid", color: "#1E293B" }] }
  });
  for (const column of columns) {
    const cell = I(line, { type: "frame", name: "Cell", layout: "horizontal", width: column.width });
    const isStatus = column.key === "status";
    I(cell, {
      type: "text", content: row[column.key], fontSize: 14,
      fontWeight: column.key === "client" ? "600" : "400",
      fill: isStatus ? statusColor[row.status] : (column.key === "client" ? "#F1F5F9" : "#CBD5E1")
    });
  }
}
