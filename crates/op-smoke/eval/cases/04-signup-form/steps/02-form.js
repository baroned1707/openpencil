const root = "n1";
const body = I(root, {
  type: "frame", name: "Form Body", layout: "vertical", width: "fill_container", gap: 24,
  padding: [32, 24, 32, 24]
});

const intro = I(body, { type: "frame", name: "Intro", layout: "vertical", gap: 8 });
I(intro, { type: "text", content: "Create your account", fontSize: 28, fontWeight: "700", fill: "#111827" });
I(intro, { type: "text", content: "Track your orders and save your favourite shops.", fontSize: 15, fill: "#4B5563" });

const fields = I(body, { type: "frame", name: "Fields", layout: "vertical", width: "fill_container", gap: 18 });
function field(label, placeholder, error) {
  const group = I(fields, { type: "frame", name: label + " Field", layout: "vertical", width: "fill_container", gap: 6 });
  I(group, { type: "text", content: label, fontSize: 14, fontWeight: "600", fill: "#111827" });
  I(group, {
    type: "text_input", name: label + " Input", placeholder: placeholder, width: "fill_container", height: 48,
    cornerRadius: 10, fill: "#F9FAFB",
    stroke: { thickness: error ? 2 : 1, fill: [{ type: "solid", color: error ? "#B91C1C" : "#D1D5DB" }] }
  });
  if (error) {
    const line = I(group, { type: "frame", name: "Error Line", layout: "horizontal", gap: 6, alignItems: "center" });
    I(line, { type: "icon_font", iconFontName: "circle-alert", width: 16, height: 16, fill: "#B91C1C" });
    I(line, { type: "text", content: error, fontSize: 13, fill: "#B91C1C" });
  }
}
field("Full name", "Ada Lovelace");
field("Email", "ada@lovelace.dev", "Enter a valid email address");
field("Password", "At least 8 characters");

const terms = I(body, { type: "frame", name: "Terms Row", layout: "horizontal", gap: 10, alignItems: "center" });
I(terms, { type: "frame", name: "Terms Checkbox", layout: "horizontal", width: 24, height: 24, cornerRadius: 6,
  stroke: { thickness: 2, fill: [{ type: "solid", color: "#6B7280" }] } , children: [] });
I(terms, { type: "text", content: "I agree to the Terms and Privacy Policy", fontSize: 14, fill: "#374151" });

const submit = I(body, {
  type: "frame", name: "Sign Up Button", layout: "horizontal", width: "fill_container", height: 52,
  justifyContent: "center", alignItems: "center", cornerRadius: 12, fill: "#1D4ED8"
});
I(submit, { type: "text", content: "Sign up", fontSize: 16, fontWeight: "700", fill: "#FFFFFF" });

const footer = I(body, { type: "frame", name: "Sign In Row", layout: "horizontal", width: "fill_container", gap: 6, justifyContent: "center" });
I(footer, { type: "text", content: "Already have an account?", fontSize: 14, fill: "#4B5563" });
I(footer, { type: "text", content: "Log in", fontSize: 14, fontWeight: "700", fill: "#1D4ED8" });
