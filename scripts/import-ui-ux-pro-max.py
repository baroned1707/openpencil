#!/usr/bin/env python3
"""Import the MIT-licensed UI UX Pro Max design data into OpenPencil.

Usage (stdlib only, run isolated):
    python3 -I scripts/import-ui-ux-pro-max.py --src DIR --retrieved YYYY-MM-DD
    python3 -I scripts/import-ui-ux-pro-max.py --src DIR --check

DIR holds the five upstream CSV files at the pinned commit below
(`.claude/skills/ui-ux-pro-max/data/` of the upstream repository):
products.csv, colors.csv, ui-reasoning.csv, typography.csv, styles.csv.
The script refuses to run on any other bytes (sha256 pinned below), writes
crates/op-ai-skills/data/design-data-uiuxpm.json (a SEPARATE file; the seed
file design-data.json is never touched) and prints a summary. --check
regenerates in memory and exits 1 if the committed JSON differs. Output is
deterministic: sorted by id, no timestamps, fixed `retrieved` date.

Mapping (one OpenPencil record per upstream product type; the three
per-product-type CSVs are joined by normalised name, every one of the 192
joins must succeed or the script stops):

  id          "uiuxpm-" + slug(Product Type), at most 48 chars, made unique
  name        Product Type
  keywords    lower-cased split of `Keywords`, characters other than letters,
              digits, space and hyphen removed; at most 20, each 2 to 40 chars;
              generic words ("app", "platform", ...) and interface words
              ("table", "menu", ...) are dropped because a single keyword hit
              already reaches the search threshold
  platforms   always "web"; "mobile" for the whole words mobile/app/apps/ios/
              android in name or keywords; "slides" for pitch/deck/
              presentation; "desktop" for dashboard/admin/analytics/crm/erp
              in name or keywords when the Dashboard Style column is not
              empty / "N/A ..."
  pattern     ui-reasoning Recommended_Pattern, else products `Landing Page
              Pattern`; cut at a word boundary to 400 chars
  styles      products Primary Style (split on " + ") then Secondary Styles
              (split on ","), resolved case-insensitively by the embedded
              style-guide `name:` (kept as is), else by styles.csv Style
              Category / Style ID / Aliases -> "external:<style-id>"; unknown
              names and Apple-derived styles are dropped; at most 6
  palette     colors.csv Primary, On Primary, Secondary, Accent, Background,
              Foreground, Card, Muted, Border, Destructive as #rrggbb ("#abc"
              and a missing "#" are normalised). An `rgba(r,g,b,a)` /
              `rgba(r g b / a)` Border is alpha-composited over that record's
              own Background, per channel round(a*fg + (1-a)*bg), clamped, and
              emitted as #rrggbb. Any other value that is not a hex colour (or
              an rgba value in any other role) skips the record.
  typography  the typography.csv pairing whose `Mood/Style Keywords` share the
              most words with ui-reasoning Typography_Mood (ties: first row);
              if none shares a word, the pairing whose `Best For` shares the
              most words with the product name + keywords. heading/body are
              font NAMES only, mood is Typography_Mood. Still no match: the
              record is skipped (the loader needs non-empty fonts and a
              half-empty record would reject the whole file).
  rules       Decision_Rules JSON {condition: [action, ...]} flattened to
              {when, do}: `must_have` -> "always", `if_x_y` -> "if x y";
              `style:foo` -> "style: <name>", other `kind:text` keep their kind.
              At most 6 (loader limit) and when+do at most 300 chars. A field
              that does not parse yields no rules; a record without rules is
              skipped.
  avoid       Anti_Patterns split on " + ", ";" and ","; at most 5, each at
              most 160 chars (loader limits)
  source      {name "UI UX Pro Max (MIT)", url, commit PINNED_COMMIT,
              licenseSpdx "MIT", retrieved}
  notes       Key Considerations cut to 600 chars

Safety applied to every text field: control and zero-width/bidi format
characters are removed (Unicode Cc and Cf, which include U+200B-U+200F,
U+202A-U+202E, U+2066-U+2069), whitespace is collapsed, and a record whose
text contains an instruction-like phrase is skipped and listed. Anything that
mentions Apple, the Human Interface Guidelines or VisionOS is removed, and
the product type "Spatial Computing OS / App" (visionOS-style content) is not
imported (a list
item, or the sentence of a note) and the Apple-derived styles liquid-glass,
apple-native-experience and spatial-ui-visionos are never referenced.

Deliberately NOT read: google-fonts.csv, google-font-licenses.json,
phosphor-icons-upstream.json, stacks/, charts.csv, landing.csv, icons.csv,
ux-guidelines.csv, and the CSS / URL / checklist columns of typography.csv and
styles.csv.
"""

import argparse
import csv
import hashlib
import json
import re
import sys
import unicodedata
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
OUTPUT = REPO_ROOT / "crates/op-ai-skills/data/design-data-uiuxpm.json"
STYLE_GUIDES = REPO_ROOT / "crates/op-ai-skills/skills/style-guides"

PINNED_COMMIT = "7436a6e0b6dc3add7c12d5b6ef1ccd3d410dd7fd"
SOURCE = {
    "name": "UI UX Pro Max (MIT)",
    "url": "https://github.com/nextlevelbuilder/ui-ux-pro-max-skill",
    "commit": PINNED_COMMIT,
    "licenseSpdx": "MIT",
}
INPUT_SHA256 = {
    "products.csv": "5756125e2b0e0d2360926f175d0975c8cd846c7589e5e2ce02631ff645e66361",
    "colors.csv": "8162429222bce22df62b564085946a30d07cc9722c58d0a3a494bd0d1d00841c",
    "ui-reasoning.csv": "f0774dd741cecdad5eec842034bd6d5e9ea50e9f220a1910069748daa0bb8e9a",
    "typography.csv": "321fc446e89024488ebae96dda93efc4d2307bd8bddb240857ad51364f6782c8",
    "styles.csv": "a9b6cadbd1cb9e7cb8398a3b445e0691d01e42ed5e5efa60a5d0c08c99c8100d",
}

ID_PREFIX = "uiuxpm-"
# Limits of crates/op-ai-skills/src/design_data.rs (one bad record rejects the file).
MAX_ID = 48
MAX_NAME = 80
MAX_KEYWORDS = 20  # loader allows 40; the brief asked for 20
MAX_KEYWORD = 40
MAX_PATTERN = 400
MAX_STYLES = 6
MAX_STYLE = 50
MAX_RULES = 6
MAX_RULE = 300  # `when` + `do` together
MAX_AVOID = 5
MAX_AVOID_ITEM = 160
MAX_NOTES = 600
MAX_FONT = 80
MAX_MOOD = 120

PALETTE_COLUMNS = [
    ("primary", "Primary"),
    ("onPrimary", "On Primary"),
    ("secondary", "Secondary"),
    ("accent", "Accent"),
    ("background", "Background"),
    ("foreground", "Foreground"),
    ("card", "Card"),
    ("muted", "Muted"),
    ("border", "Border"),
    ("destructive", "Destructive"),
]

# Product types whose subject is an Apple platform UI: not imported.
APPLE_PLATFORM_PRODUCTS = {"spatial computing os / app"}  # visionOS-style content
# Styles that come from Apple documentation: never referenced.
APPLE_STYLE_IDS = {"liquid-glass", "apple-native-experience", "spatial-ui-visionos"}
# `HIG` is case-sensitive on purpose: "hig" is inside "high".
APPLE_RE = re.compile(r"apple|human interface|visionos|(?-i:\bHIG\b)", re.IGNORECASE)
INSTRUCTION_RE = re.compile(
    r"ignore (all |any )?(previous|prior) instructions|system prompt|you are now|disregard",
    re.IGNORECASE,
)
# Words that must not be keywords: one hit already reaches the search
# threshold (design_data_search.rs MIN_SCORE), and these say nothing about a
# product type. Mirrors GENERIC_WORDS there plus a few corpus-wide fillers.
GENERIC_KEYWORDS = {
    "app", "apps", "the", "and", "for", "with", "page", "pages", "site",
    "sites", "website", "web", "screen", "screens", "design", "designs",
    "build", "make", "new", "ui", "ux", "user", "interface", "that", "this",
    "from", "into", "your", "our", "simple", "modern",
    "platform", "general", "tech", "tool", "tools", "service", "services",
    "system", "online", "digital", "software", "product", "business", "world",
    # Interface vocabulary: "a table of cards in a sidebar" is a layout
    # description, not a restaurant or a file manager.
    "table", "menu", "list", "grid", "filter", "search", "form", "card",
    "button", "sidebar", "header", "footer", "modal", "tab", "navigation",
    "layout",
}
TOKEN_STOPWORDS = {
    "and", "the", "with", "for", "typography", "font", "fonts", "type",
    "text", "design", "style", "feel", "look", "that", "from",
}


def fail(message):
    sys.exit(f"import-ui-ux-pro-max: {message}")


# ---------------------------------------------------------------- text helpers


def clean(text):
    """Remove control/format characters, collapse whitespace."""
    text = "".join(
        " " if ch.isspace() else ch
        for ch in text
        if unicodedata.category(ch) not in ("Cc", "Cf") or ch.isspace()
    )
    return re.sub(r"\s+", " ", text).strip()


def truncate(text, limit):
    """Cut at a word boundary so the result is at most `limit` characters."""
    if len(text) <= limit:
        return text
    cut = text[:limit]
    if text[limit] != " " and " " in cut:
        cut = cut[: cut.rindex(" ")]
    return cut.rstrip(" ,;:-+/")


def mentions_apple(text):
    return bool(APPLE_RE.search(text))


def drop_apple_sentences(text):
    """Remove every sentence that mentions Apple / HIG / VisionOS."""
    sentences = re.split(r"(?<=[.!?])\s+", text)
    return " ".join(s for s in sentences if not mentions_apple(s)).strip()


def slug(text):
    return re.sub(r"[^a-z0-9]+", "-", text.lower()).strip("-")


def normalise_name(text):
    return re.sub(r"\s+", " ", text.strip().lower())


def words(text):
    """Lower-case words of >= 3 letters, a trailing plural `s` removed."""
    out = set()
    for raw in re.findall(r"[a-z]+", text.lower()):
        if len(raw) < 3 or raw in TOKEN_STOPWORDS:
            continue
        if raw.endswith("ies") and len(raw) > 4:
            raw = raw[:-3] + "y"
        elif raw.endswith("s") and len(raw) > 3:
            raw = raw[:-1]
        out.add(raw)
    return out


# ------------------------------------------------------------------ CSV input


def read_csv(directory, name):
    path = directory / name
    if not path.is_file():
        fail(f"missing input {path}")
    data = path.read_bytes()
    digest = hashlib.sha256(data).hexdigest()
    if digest != INPUT_SHA256[name]:
        fail(
            f"{name} has sha256 {digest}, expected {INPUT_SHA256[name]} "
            f"(the importer is pinned to upstream commit {PINNED_COMMIT})"
        )
    return list(csv.DictReader(data.decode("utf-8").splitlines(keepends=True)))


def embedded_style_guide_names():
    names = set()
    for path in sorted(STYLE_GUIDES.glob("*.md")):
        for line in path.read_text(encoding="utf-8").splitlines()[:8]:
            match = re.match(r"name:\s*['\"]?([^'\"]+?)['\"]?\s*$", line)
            if match:
                names.add(normalise_name(match.group(1)))
                break
    return names


def style_lookup(style_rows):
    """normalised Style Category / Style ID / alias -> (style id, display name)."""
    table = {}
    for row in style_rows:
        style_id = row["Style ID"].strip()
        entry = (style_id, clean(row["Style Category"]))
        keys = [row["Style Category"], style_id] + row["Aliases"].split("|")
        for key in keys:
            if key.strip():
                table.setdefault(normalise_name(key), entry)
    return table


# --------------------------------------------------------------------- fields


def normalise_hex(value):
    value = value.strip()
    if re.fullmatch(r"#?[0-9a-fA-F]{3}", value):
        value = value.lstrip("#")
        value = "".join(ch * 2 for ch in value)
    elif re.fullmatch(r"#?[0-9a-fA-F]{6}", value):
        value = value.lstrip("#")
    else:
        return None
    return "#" + value.lower()


RGBA_RE = re.compile(
    r"rgba\(\s*(\d{1,3})\s*[,\s]\s*(\d{1,3})\s*[,\s]\s*(\d{1,3})\s*[,/]\s*"
    r"(\d*\.?\d+)(%?)\s*\)",
    re.IGNORECASE,
)


def composite_over(value, background):
    """`rgba(...)` laid over the #rrggbb `background`; None if not rgba."""
    match = RGBA_RE.fullmatch(value.strip())
    if not match:
        return None
    channels = [int(match.group(i)) for i in (1, 2, 3)]
    alpha = float(match.group(4)) / (100 if match.group(5) else 1)
    if any(c > 255 for c in channels) or not 0 <= alpha <= 1:
        return None
    out = []
    for index, foreground in enumerate(channels):
        behind = int(background[1 + 2 * index : 3 + 2 * index], 16)
        mixed = int(alpha * foreground + (1 - alpha) * behind + 0.5)
        out.append(max(0, min(255, mixed)))
    return "#" + "".join(f"{c:02x}" for c in out)


def build_keywords(raw):
    out = []
    for part in raw.split(","):
        keyword = re.sub(r"[^\w \-]", "", clean(part).lower(), flags=re.UNICODE)
        keyword = re.sub(r"[_]", "", keyword)
        keyword = re.sub(r"\s+", " ", keyword).strip()
        if (
            len(keyword) < 2
            or len(keyword) > MAX_KEYWORD
            or keyword in GENERIC_KEYWORDS
            or mentions_apple(keyword)
            or keyword in out
        ):
            continue
        out.append(keyword)
    return out[:MAX_KEYWORDS]


def has_dashboard(text):
    text = text.strip()
    return bool(text) and not re.match(r"n/?a\b", text, re.IGNORECASE)


def build_platforms(name, raw_keywords, dashboard_style):
    tokens = set(re.findall(r"[a-z0-9]+", " ".join([name, raw_keywords]).lower()))
    platforms = ["web"]
    if tokens & {"mobile", "app", "apps", "ios", "android"}:
        platforms.append("mobile")
    if tokens & {"pitch", "deck", "presentation"}:
        platforms.append("slides")
    if has_dashboard(dashboard_style) and tokens & {
        "dashboard", "admin", "analytics", "crm", "erp",
    }:
        platforms.append("desktop")
    return platforms


class StyleResolver:
    def __init__(self, guide_names, lookup):
        self.guide_names = guide_names
        self.lookup = lookup
        self.unresolved = {}
        self.apple_dropped = 0
        self.external = 0
        self.embedded = 0

    def resolve(self, raw):
        """Return the record style entry for one upstream style name, or None."""
        key = normalise_name(clean(raw))
        if not key:
            return None
        if key in self.guide_names:
            self.embedded += 1
            return key
        entry = self.lookup.get(key)
        if entry is None:
            self.unresolved[key] = self.unresolved.get(key, 0) + 1
            return None
        style_id, _ = entry
        if style_id in APPLE_STYLE_IDS:
            self.apple_dropped += 1
            return None
        value = "external:" + style_id
        if len(value) > MAX_STYLE:
            return None
        self.external += 1
        return value

    def display_name(self, style_id):
        for style_key, (found_id, display) in self.lookup.items():
            if found_id == style_id:
                return display
        return None


def build_styles(resolver, primary, secondary):
    names = [part for part in primary.split(" + ")] + secondary.split(",")
    out = []
    for name in names:
        value = resolver.resolve(name)
        if value and value not in out:
            out.append(value)
    return out[:MAX_STYLES]


def humanise_condition(condition):
    if condition == "must_have":
        return "always"
    text = condition[3:] if condition.startswith("if_") else condition
    prefix = "if " if condition.startswith("if_") else ""
    return prefix + text.replace("_", " ")


def humanise_action(resolver, action):
    """`kind:text` -> "kind: text"; None when the action must be dropped."""
    kind, sep, value = action.partition(":")
    if not sep:
        return clean(action) or None
    kind, value = kind.strip().lower(), value.strip()
    if not value:
        return None
    if kind == "style":
        if value in APPLE_STYLE_IDS:
            return None
        display = resolver.display_name(value)
        if display is None:
            return None
        return f"style: {display}"
    return f"{kind}: {value.replace('-', ' ') if kind == 'constraint' else value}"


def build_rules(resolver, raw):
    try:
        parsed = json.loads(raw)
    except (ValueError, TypeError):
        return []
    if not isinstance(parsed, dict):
        return []
    rules = []
    for condition, actions in parsed.items():
        if not isinstance(actions, list):
            continue
        when = clean(humanise_condition(str(condition)))
        for action in actions:
            if not isinstance(action, str):
                continue
            do = humanise_action(resolver, clean(action))
            if not do or mentions_apple(do) or mentions_apple(when):
                continue
            if len(when) + len(do) > MAX_RULE:
                continue
            rules.append({"when": when, "do": do})
    return rules[:MAX_RULES]


def build_avoid(raw):
    out = []
    for part in re.split(r" \+ |;|,", raw):
        item = clean(part)
        if not item or mentions_apple(item):
            continue
        item = truncate(item, MAX_AVOID_ITEM)
        if item and item not in out:
            out.append(item)
    return out[:MAX_AVOID]


def best_typography(mood, evidence, pairings):
    """Pick the font pairing for a product type.

    First choice: the pairing whose `Mood/Style Keywords` share the most words
    with Typography_Mood (ties: first row). When no pairing shares a word, the
    fallback is the pairing whose `Best For` shares the most words with the
    product's name and keywords (ties: first row). Still nothing: None.
    """
    for column, text in (("Mood/Style Keywords", mood), ("Best For", evidence)):
        wanted = words(text)
        best, best_score = None, 0
        for pairing in pairings:
            score = len(wanted & words(pairing[column]))
            if score > best_score:
                best, best_score = pairing, score
        if best is not None:
            return best, column
    return None, None


def relative_luminance(hex_color):
    channels = []
    for index in (1, 3, 5):
        value = int(hex_color[index : index + 2], 16) / 255
        channels.append(value / 12.92 if value <= 0.03928 else ((value + 0.055) / 1.055) ** 2.4)
    return 0.2126 * channels[0] + 0.7152 * channels[1] + 0.0722 * channels[2]


def contrast(first, second):
    high, low = sorted((relative_luminance(first), relative_luminance(second)), reverse=True)
    return (high + 0.05) / (low + 0.05)


# ---------------------------------------------------------------- the importer


def build(src, retrieved):
    products = read_csv(src, "products.csv")
    colors = read_csv(src, "colors.csv")
    reasoning = read_csv(src, "ui-reasoning.csv")
    pairings = read_csv(src, "typography.csv")
    style_rows = read_csv(src, "styles.csv")

    color_by_name = {normalise_name(r["Product Type"]): r for r in colors}
    reasoning_by_name = {normalise_name(r["UI_Category"]): r for r in reasoning}
    if len(color_by_name) != len(colors) or len(reasoning_by_name) != len(reasoning):
        fail("duplicate product-type names in colors.csv or ui-reasoning.csv")
    unmatched = [
        r["Product Type"]
        for r in products
        if normalise_name(r["Product Type"]) not in color_by_name
        or normalise_name(r["Product Type"]) not in reasoning_by_name
    ]
    if unmatched or len(products) != len(colors) or len(products) != len(reasoning):
        fail(f"product join failed ({len(products)}/{len(colors)}/{len(reasoning)}): {unmatched}")

    resolver = StyleResolver(embedded_style_guide_names(), style_lookup(style_rows))
    records, skipped, used_ids = [], [], set()
    below_aa = 0
    composited = 0
    fallback_pairings = 0

    for product in products:
        raw_name = clean(product["Product Type"])
        key = normalise_name(product["Product Type"])
        color, reason = color_by_name[key], reasoning_by_name[key]

        def skip(why):
            skipped.append((raw_name, why))

        if not raw_name or mentions_apple(raw_name):
            skip("name empty or Apple-related")
            continue
        if key in APPLE_PLATFORM_PRODUCTS:
            skip("subject is an Apple platform UI (visionOS-style)")
            continue
        palette = {}
        bad_role = None
        for role, column in PALETTE_COLUMNS:
            value = normalise_hex(color[column])
            if value is None and role == "border":
                value = composite_over(color[column], palette["background"])
                if value is not None:
                    composited += 1
            if value is None:
                bad_role = f"{column}={color[column].strip()!r}"
                break
            palette[role] = value
        if bad_role:
            skip(f"palette value is not a hex colour: {bad_role}")
            continue

        pattern = truncate(
            drop_apple_sentences(
                clean(reason["Recommended_Pattern"]) or clean(product["Landing Page Pattern"])
            ),
            MAX_PATTERN,
        )
        notes = truncate(drop_apple_sentences(clean(product["Key Considerations"])), MAX_NOTES)
        keywords = build_keywords(product["Keywords"])
        rules = build_rules(resolver, reason["Decision_Rules"])
        mood = truncate(clean(reason["Typography_Mood"]), MAX_MOOD)
        pairing, pairing_basis = best_typography(
            reason["Typography_Mood"], raw_name + " " + product["Keywords"], pairings
        )
        if pairing_basis == "Best For":
            fallback_pairings += 1

        missing = [
            label
            for label, present in [
                ("pattern", pattern),
                ("notes", notes),
                ("keywords", keywords),
                ("rules", rules),
                ("typography mood", mood),
                ("typography pairing (no word in common)", pairing),
            ]
            if not present
        ]
        if missing:
            skip("empty " + ", ".join(missing))
            continue
        heading, body = clean(pairing["Heading Font"]), clean(pairing["Body Font"])
        if not heading or not body or len(heading) > MAX_FONT or len(body) > MAX_FONT:
            skip("typography pairing has no usable font names")
            continue

        base = slug(raw_name)[: MAX_ID - len(ID_PREFIX)].strip("-")
        record_id, counter = ID_PREFIX + base, 2
        while record_id in used_ids:
            suffix = f"-{counter}"
            record_id = ID_PREFIX + base[: MAX_ID - len(ID_PREFIX) - len(suffix)] + suffix
            counter += 1

        record = {
            "id": record_id,
            "name": truncate(raw_name, MAX_NAME),
            "keywords": keywords,
            "platforms": build_platforms(
                raw_name, product["Keywords"], clean(product["Dashboard Style (if applicable)"])
            ),
            "pattern": pattern,
            "styles": build_styles(
                resolver,
                product["Primary Style Recommendation"],
                product["Secondary Styles"],
            ),
            "palette": palette,
            "typography": {"heading": heading, "body": body, "mood": mood},
            "rules": rules,
            "avoid": build_avoid(reason["Anti_Patterns"]),
            "source": dict(SOURCE, retrieved=retrieved),
            "notes": notes,
        }
        flattened = json.dumps(record, ensure_ascii=False)
        hit = INSTRUCTION_RE.search(flattened)
        if hit:
            skip(f"instruction-like text {hit.group(0)!r}")
            continue
        if mentions_apple(flattened):
            skip("Apple-related text survived the scrub")
            continue
        if contrast(palette["onPrimary"], palette["primary"]) < 4.5 or contrast(
            palette["foreground"], palette["background"]
        ) < 4.5:
            below_aa += 1
        used_ids.add(record_id)
        records.append(record)

    records.sort(key=lambda r: r["id"])
    return records, skipped, resolver, below_aa, fallback_pairings, composited


def render(records):
    return json.dumps(records, indent=2, ensure_ascii=False) + "\n"


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--src", required=True, type=Path, help="directory with the five CSV files")
    parser.add_argument(
        "--retrieved",
        help="fixed retrieval date, YYYY-MM-DD (required to write; --check defaults "
        "to the date in the committed JSON)",
    )
    parser.add_argument("--check", action="store_true", help="exit 1 if the committed JSON differs")
    args = parser.parse_args()
    retrieved = args.retrieved
    if retrieved is None and args.check and OUTPUT.is_file():
        retrieved = json.loads(OUTPUT.read_text(encoding="utf-8"))[0]["source"]["retrieved"]
    if not retrieved or not re.fullmatch(r"\d{4}-\d{2}-\d{2}", retrieved):
        fail("--retrieved YYYY-MM-DD is required")

    records, skipped, resolver, below_aa, fallback_pairings, composited = build(args.src, retrieved)
    text = render(records)

    print(f"pinned commit      {PINNED_COMMIT}")
    print(f"records written    {len(records)}")
    print(f"records skipped    {len(skipped)}")
    for name, why in skipped:
        print(f"  skip {name}: {why}")
    print(
        f"styles             {resolver.external} external, {resolver.embedded} embedded, "
        f"{resolver.apple_dropped} Apple-derived dropped, "
        f"{sum(resolver.unresolved.values())} unresolved"
    )
    for name, count in sorted(resolver.unresolved.items()):
        print(f"  unresolved style {name!r} x{count}")
    print(f"font pairing       {fallback_pairings} records matched through `Best For` (no mood word shared)")
    print(f"palette            {composited} borders composited over Background")
    print(f"below WCAG AA 4.5  {below_aa} records (onPrimary/primary or foreground/background)")
    print(f"output size        {len(text.encode('utf-8')) / 1024:.1f} KiB")

    if args.check:
        committed = OUTPUT.read_bytes() if OUTPUT.is_file() else b""
        if committed != text.encode("utf-8"):
            print(f"DRIFT: {OUTPUT} differs from the regenerated output", file=sys.stderr)
            return 1
        print("checked: committed JSON matches the pinned source")
        return 0
    OUTPUT.write_text(text, encoding="utf-8")
    print(f"wrote {OUTPUT}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
