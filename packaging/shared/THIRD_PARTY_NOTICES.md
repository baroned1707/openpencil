# Third-party notices

OpenPencil ships some third-party content next to the program. This file is
the single list of what that content is, where it came from and under which
licence. It is staged into every distribution by `scripts/stage-notices.sh`
(macOS `Contents/Resources/notices`, Linux `/usr/share/doc/openpencil/notices`,
Windows `<install dir>\notices`, and the release archives), and the
**File > Third-party notices** menu item opens it.

Rules for maintainers:

- Every record in `crates/op-ai-skills/data/design-data.json` and
  `design-data-uiuxpm.json` carries a `source` object (`name`, `url`, `commit`, `licenseSpdx`, `retrieved`).
- Every `source.name` other than `OpenPencil seed` MUST have a level-2
  heading in this file with exactly that text, followed by the copyright line
  and the licence text. The test `every_non_seed_source_has_a_notice_section`
  fails otherwise. A heading inside an HTML comment does not count.
- Fonts keep their own notice files under `packaging/shared/fonts/`; they are
  staged by name pattern (`NOTICE.txt`, `OFL-*.txt`), so a new font only needs
  its `OFL-<family>.txt` and an entry in `NOTICE.txt`.

## Bundled third-party content

| Content | Origin | Licence | Notice files |
| --- | --- | --- | --- |
| Design fonts (Inter, Noto Sans SC, Noto Serif SC, Plus Jakarta Sans, Cormorant Garamond, DM Mono, DM Sans, DM Serif Display, Instrument Serif, JetBrains Mono, Manrope, Outfit, Space Grotesk, Archivo, Libre Caslon Text) | Google Fonts and the upstream font projects, see `NOTICE.txt` for each source | SIL Open Font License 1.1 | `NOTICE.txt`, `OFL-*.txt` |
| Design data: UI UX Pro Max (product types, palettes, rules, font pairings, style names) | https://github.com/nextlevelbuilder/ui-ux-pro-max-skill at commit `7436a6e0b6dc3add7c12d5b6ef1ccd3d410dd7fd` | MIT, Copyright (c) 2024 Next Level Builder | section "UI UX Pro Max (MIT)" below |
| Design data: OpenPencil seed | Written for OpenPencil from general design knowledge, no third-party dataset | MIT (this project) | none needed |

## OpenPencil seed

The six seed records in `design-data.json` (`source.name` = `OpenPencil seed`)
are original work of the OpenPencil project, released under the MIT licence of
this repository. They contain no third-party text.

## UI UX Pro Max (MIT)

OpenPencil's design-data table includes records derived from the **UI UX Pro
Max** skill data set, retrieved from its public repository:

- Repository: https://github.com/nextlevelbuilder/ui-ux-pro-max-skill
- Pinned commit: `7436a6e0b6dc3add7c12d5b6ef1ccd3d410dd7fd`
- Files used (`.claude/skills/ui-ux-pro-max/data/`): `products.csv`,
  `colors.csv`, `ui-reasoning.csv`, `typography.csv`, `styles.csv`
- Copyright (c) 2024 Next Level Builder

What was used: product types (names and search keywords), colour palettes (ten
colour roles per product type), reasoning rules and anti-patterns, font
pairings (family names only) and style names. These records are in
`crates/op-ai-skills/data/design-data-uiuxpm.json`; each carries the source
name `UI UX Pro Max (MIT)`, the repository URL, the pinned commit and the
licence `MIT`.

Modifications: the upstream CSV files were converted to JSON by
`scripts/import-ui-ux-pro-max.py`; columns were dropped (CSS, URL, code and
checklist columns, and the colour roles On Secondary, On Accent, Card
Foreground, Muted Foreground, On Destructive and Ring); the three per-product-type files were joined by product type; text was
normalised (control and invisible characters removed, whitespace collapsed,
long fields shortened at a word boundary); and records or list items that did
not pass validation were filtered out. Style names are kept only as
references, never as style descriptions.

Deliberately NOT included: any row or sentence that refers to Apple
documentation or the Human Interface Guidelines (the styles Liquid Glass,
Apple Native Experience and Spatial UI (VisionOS) are not referenced), the
Google Fonts and icon catalogs, the per-framework stack rules, and the other
upstream data files (charts, landing pages, UX guidelines, icons, motion).

The licence text of the upstream project, verbatim:

```text
MIT License

Copyright (c) 2024 Next Level Builder

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```
