const root = I(null, {
  type: "frame", name: "Settings", width: 390, height: 844, layout: "vertical",
  fill: "$--background", clipContent: true
});

const header = I(root, {
  type: "frame", name: "Header", layout: "horizontal", width: "fill_container", alignItems: "center",
  padding: [8, 20, 8, 20]
});
I(header, { type: "text", content: "Settings", fontSize: 28, fontWeight: "700", fill: "$--foreground" });

const profileWrap = I(root, { type: "frame", name: "Profile Wrap", layout: "vertical", width: "fill_container", padding: [8, 20, 8, 20] });
const profile = I(profileWrap, {
  type: "frame", name: "Profile Card", layout: "horizontal", width: "fill_container", gap: 14,
  alignItems: "center", padding: 16, cornerRadius: 16, fill: "$--card",
  stroke: { thickness: 1, fill: [{ type: "solid", color: "$--border" }] }
});
const avatar = I(profile, {
  type: "frame", name: "Avatar", layout: "horizontal", width: 56, height: 56, cornerRadius: 28,
  justifyContent: "center", alignItems: "center", fill: "$--primary"
});
I(avatar, { type: "text", content: "AL", fontSize: 20, fontWeight: "700", fill: "$--primary-foreground" });
const who = I(profile, { type: "frame", name: "Who", layout: "vertical", gap: 2 });
I(who, { type: "text", content: "Ada Lovelace", fontSize: 18, fontWeight: "700", fill: "$--foreground" });
I(who, { type: "text", content: "ada@lovelace.dev", fontSize: 14, fill: "$--muted-foreground" });
