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
- Every prompt skill adapted from third-party text is listed in the section
  "Adapted prompt skills" with its source files and pinned commits, and is
  marked as modified (Apache-2.0 section 4(b)). The tests in
  `crates/op-ai-skills/src/design_data_notices_tests.rs` check the list.
- Fonts keep their own notice files under `packaging/shared/fonts/`; they are
  staged by name pattern (`NOTICE.txt`, `OFL-*.txt`), so a new font only needs
  its `OFL-<family>.txt` and an entry in `NOTICE.txt`.

## Bundled third-party content

| Content | Origin | Licence | Notice files |
| --- | --- | --- | --- |
| Design fonts (Inter, Noto Sans SC, Noto Serif SC, Plus Jakarta Sans, Cormorant Garamond, DM Mono, DM Sans, DM Serif Display, Instrument Serif, JetBrains Mono, Manrope, Outfit, Space Grotesk, Archivo, Libre Caslon Text) | Google Fonts and the upstream font projects, see `NOTICE.txt` for each source | SIL Open Font License 1.1 | `NOTICE.txt`, `OFL-*.txt` |
| Design data: UI UX Pro Max (product types, palettes, rules, font pairings, style names, style definitions) | https://github.com/nextlevelbuilder/ui-ux-pro-max-skill at commit `7436a6e0b6dc3add7c12d5b6ef1ccd3d410dd7fd` | MIT, Copyright (c) 2024 Next Level Builder | section "UI UX Pro Max (MIT)" below |
| Prompt skills `design-craft`, `design-brief` (adapted, condensed, modified): Anthropic frontend-design and write-spec | https://github.com/anthropics/skills and https://github.com/anthropics/knowledge-work-plugins | Apache-2.0 | sections "Anthropic skills (frontend-design) and knowledge-work-plugins (write-spec) (Apache-2.0)" and "Apache License 2.0 (full text)" |
| Prompt skill `design-craft` (adapted, condensed, modified): Impeccable | https://github.com/pbakaus/impeccable, Copyright 2025 Paul Bakaus | Apache-2.0 | section "Impeccable (Apache-2.0)", with its NOTICE text |
| Prompt skill `design-brief` (adapted, condensed, modified): designer-skills | https://github.com/Owl-Listener/designer-skills | MIT, Copyright (c) 2026 MC Dean | section "Owl-Listener designer-skills (MIT)" |
| Icon glyphs (path data of the Lucide icon set, embedded in `iconify-catalog-core.json` and the first-party glyph table) | https://github.com/lucide-icons/lucide, packaged by `@iconify-json/lucide` | ISC | section "Lucide (ISC)" below |
| Icon glyphs (path data of the Feather icon set, embedded in `iconify-catalog-core.json`) | https://github.com/feathericons/feather, packaged by `@iconify-json/feather` | MIT | section "Feather (MIT)" below |
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
pairings (family names only), style names and style definitions (for each
style: its aliases and keywords, what it is best and not for, effects, accessibility notes, complexity,
light and dark mode support, era and the colour values named in the upstream
colour cells). The product records are in
`crates/op-ai-skills/data/design-data-uiuxpm.json` and the style definitions in
`crates/op-ai-skills/data/design-styles-uiuxpm.json`; each record in both files
carries the source name `UI UX Pro Max (MIT)`, the repository URL, the pinned
commit and the licence `MIT`.

Modifications: the upstream CSV files were converted to JSON by
`scripts/import-ui-ux-pro-max.py`; columns were dropped (CSS, URL, code and
checklist columns, the AI-prompt, framework and design-system-variable columns, and the colour roles On Secondary, On Accent, Card
Foreground, Muted Foreground, On Destructive and Ring); the three per-product-type files were joined by product type; text was
normalised (control and invisible characters removed, whitespace collapsed,
long fields shortened at a word boundary); and records or list items that did
not pass validation were filtered out. Style rows marked deprecated
upstream were not imported, and in the style definitions the colour values were
read out of the upstream colour cells as `#rrggbb`.

Deliberately NOT included: any row or sentence that refers to Apple
documentation or the Human Interface Guidelines (the styles Liquid Glass,
Apple Native Experience and Spatial UI (VisionOS) are not imported or
referenced, and list items that mention Apple were removed from the remaining
style definitions), the
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

## Anthropic skills (frontend-design) and knowledge-work-plugins (write-spec) (Apache-2.0)

OpenPencil's prompt skills `design-craft` and `design-brief` were adapted from
two Apache-2.0 projects of Anthropic, PBC:

- https://github.com/anthropics/skills, file `skills/frontend-design/SKILL.md`,
  pinned commit `e4bb5d960d87126ff3b3a8af1a104b7db67560ba`. Licence file: `skills/frontend-design/LICENSE.txt`
  (Apache License, Version 2.0). The licence file carries no copyright line, so
  the work is attributed by its repository URL.
- https://github.com/anthropics/knowledge-work-plugins, file
  `product-management/skills/write-spec/SKILL.md`, pinned commit `e91a134584eb798bac7fb0379358006a2b63b965`.
  Licence file: `product-management/LICENSE` (Apache License, Version 2.0, with
  only the unfilled template copyright line), so the work is attributed by its
  repository URL.

Adapted and condensed by OpenPencil for a node-based editor; modified. The upstream text was rewritten in OpenPencil's own words and cut
down to the judgement that carries over to a node-based vector editor
(frames, auto-layout, fills, strokes, text, icon_font nodes, variables,
components). Everything specific to CSS, HTML, React, web search, file writing,
connectors and tools the agent does not have was removed. These files are
changed from the originals. The full licence text is in the section "Apache
License 2.0 (full text)" below.

## Impeccable (Apache-2.0)

OpenPencil's prompt skill `design-craft` was adapted in part from **Impeccable**
by Paul Bakaus.

- Repository: https://github.com/pbakaus/impeccable
- Pinned commit: `d41d9860b0094dcb68030657da63461212835aaa`
- Files used (under `skill/reference/`): `craft-floor.md`, `typeset.md`,
  `colorize.md`, `layout.md`, `critique-rubric.md`
- Copyright 2025 Paul Bakaus
- Licence: Apache License, Version 2.0 (full text below)

Adapted and condensed by OpenPencil for a node-based editor; modified. These files are changed from the originals: rewritten in OpenPencil's
own words, condensed, and stripped of everything about CSS, HTML, scripts, the
Impeccable command line and its live mode.

Excluded on purpose: `skill/reference/ios.md`, `skill/reference/android.md` and
every `*.native.md` reference (Apple Human Interface Guidelines and Material
Design material). Nothing from them was read into, or is present in, any
OpenPencil skill.

Impeccable's own NOTICE file, reproduced as the licence requires (every line
indented here only so that it stays out of this file's headings):

    # Third-Party Notices

    This project includes content derived from third-party work, used under the terms of its original license.

    ## Platform Design Skills

    The `skill/reference/ios.md` and `skill/reference/android.md` platform reference files are distilled from ehmo's `platform-design-skills` (Apple Human Interface Guidelines and Material Design 3 rules), rewritten in Impeccable's voice.

    **Original work:** https://github.com/ehmo/platform-design-skills
    **Original license:** MIT
    **Author:** ehmo

## Owl-Listener designer-skills (MIT)

OpenPencil's prompt skill `design-brief` was adapted in part from the
**designer-skills** collection by MC Dean.

- Repository: https://github.com/Owl-Listener/designer-skills
- Pinned commit: `9a6930cf84a822eb458624bd11c61aac5bbdf224`
- Files used: `ux-strategy/skills/design-brief/SKILL.md`,
  `ux-strategy/skills/information-architecture/SKILL.md`,
  `prototyping-testing/skills/user-flow-diagram/SKILL.md`
- Copyright (c) 2026 MC Dean

Adapted and condensed by OpenPencil for a node-based editor; modified. The upstream skills were rewritten in OpenPencil's own words and
condensed into one short brief workflow. The licence text of the upstream
project, verbatim:

```text
MIT License

Copyright (c) 2026 MC Dean

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

## Lucide (ISC)

The `icon_font` glyphs drawn with `iconFontFamily` `lucide` (the icon picker and
every icon node) use path data from the **Lucide** icon set, embedded in
`crates/op-editor-ui/assets/iconify-catalog-core.json` and, for a few glyphs of
the editor's own chrome, copied into `crates/op-editor-ui/src/widgets/icons_data/`.

- Repository: https://github.com/lucide-icons/lucide
- Packaged for OpenPencil by `@iconify-json/lucide` (`packages/scripts/generate-iconify-catalog.mjs`)
- Used unmodified: only the path data, re-encoded as Iconify `d` strings

The licence text of the upstream project, verbatim (copied from the `LICENSE`
file shipped by `lucide-react`):

```text
ISC License

Copyright (c) for portions of Lucide are held by Cole Bemis 2013-2022 as part of Feather (MIT). All other copyright (c) for Lucide are held by Lucide Contributors 2022.

Permission to use, copy, modify, and/or distribute this software for any
purpose with or without fee is hereby granted, provided that the above
copyright notice and this permission notice appear in all copies.

THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
```

## Feather (MIT)

The `icon_font` glyphs drawn with `iconFontFamily` `feather` use path data from
the **Feather** icon set, embedded in
`crates/op-editor-ui/assets/iconify-catalog-core.json`. A `lucide` glyph name
that Lucide no longer has falls back to the Feather glyph of the same name.

- Repository: https://github.com/feathericons/feather
- Packaged for OpenPencil by `@iconify-json/feather`
- Copyright (c) Cole Bemis (the upstream copyright year is not recorded here)

The MIT licence applies to the Feather path data:

```text
The MIT License (MIT)

Copyright (c) Cole Bemis

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

## Adapted prompt skills

Every skill below lives in `crates/op-ai-skills/skills/domains/`. Sources are
listed as repository path at the pinned commit. "Adapted and condensed by OpenPencil for a node-based editor; modified." applies to each.

### design-craft

- `skills/frontend-design/SKILL.md` at anthropics/skills `e4bb5d960d87126ff3b3a8af1a104b7db67560ba` (Apache-2.0)
- `skill/reference/craft-floor.md` at pbakaus/impeccable `d41d9860b0094dcb68030657da63461212835aaa` (Apache-2.0)
- `skill/reference/typeset.md` at pbakaus/impeccable `d41d9860b0094dcb68030657da63461212835aaa` (Apache-2.0)
- `skill/reference/colorize.md` at pbakaus/impeccable `d41d9860b0094dcb68030657da63461212835aaa` (Apache-2.0)
- `skill/reference/layout.md` at pbakaus/impeccable `d41d9860b0094dcb68030657da63461212835aaa` (Apache-2.0)
- `skill/reference/critique-rubric.md` at pbakaus/impeccable `d41d9860b0094dcb68030657da63461212835aaa` (Apache-2.0)

Adapted and condensed by OpenPencil for a node-based editor; modified.

### design-brief

- `ux-strategy/skills/design-brief/SKILL.md` at Owl-Listener/designer-skills `9a6930cf84a822eb458624bd11c61aac5bbdf224` (MIT)
- `ux-strategy/skills/information-architecture/SKILL.md` at Owl-Listener/designer-skills `9a6930cf84a822eb458624bd11c61aac5bbdf224` (MIT)
- `prototyping-testing/skills/user-flow-diagram/SKILL.md` at Owl-Listener/designer-skills `9a6930cf84a822eb458624bd11c61aac5bbdf224` (MIT)
- `product-management/skills/write-spec/SKILL.md` at anthropics/knowledge-work-plugins `e91a134584eb798bac7fb0379358006a2b63b965` (Apache-2.0)

Adapted and condensed by OpenPencil for a node-based editor; modified.

### design-wcag-quickcheck

No third-party text was copied or adapted. Criterion numbers and thresholds
refer to WCAG 2.2 (W3C); this skill is an independent summary. The
specification is at https://www.w3.org/TR/WCAG22/.

## Apache License 2.0 (full text)

The Apache License, Version 2.0 text below applies to the Anthropic and
Impeccable sections above (copied from the licence file of anthropics/skills
`skills/frontend-design/LICENSE.txt` at `e4bb5d960d87126ff3b3a8af1a104b7db67560ba`). The copyright holders
are the ones named in those sections.

```text
                                 Apache License
                           Version 2.0, January 2004
                        http://www.apache.org/licenses/

   TERMS AND CONDITIONS FOR USE, REPRODUCTION, AND DISTRIBUTION

   1. Definitions.

      "License" shall mean the terms and conditions for use, reproduction,
      and distribution as defined by Sections 1 through 9 of this document.

      "Licensor" shall mean the copyright owner or entity authorized by
      the copyright owner that is granting the License.

      "Legal Entity" shall mean the union of the acting entity and all
      other entities that control, are controlled by, or are under common
      control with that entity. For the purposes of this definition,
      "control" means (i) the power, direct or indirect, to cause the
      direction or management of such entity, whether by contract or
      otherwise, or (ii) ownership of fifty percent (50%) or more of the
      outstanding shares, or (iii) beneficial ownership of such entity.

      "You" (or "Your") shall mean an individual or Legal Entity
      exercising permissions granted by this License.

      "Source" form shall mean the preferred form for making modifications,
      including but not limited to software source code, documentation
      source, and configuration files.

      "Object" form shall mean any form resulting from mechanical
      transformation or translation of a Source form, including but
      not limited to compiled object code, generated documentation,
      and conversions to other media types.

      "Work" shall mean the work of authorship, whether in Source or
      Object form, made available under the License, as indicated by a
      copyright notice that is included in or attached to the work
      (an example is provided in the Appendix below).

      "Derivative Works" shall mean any work, whether in Source or Object
      form, that is based on (or derived from) the Work and for which the
      editorial revisions, annotations, elaborations, or other modifications
      represent, as a whole, an original work of authorship. For the purposes
      of this License, Derivative Works shall not include works that remain
      separable from, or merely link (or bind by name) to the interfaces of,
      the Work and Derivative Works thereof.

      "Contribution" shall mean any work of authorship, including
      the original version of the Work and any modifications or additions
      to that Work or Derivative Works thereof, that is intentionally
      submitted to Licensor for inclusion in the Work by the copyright owner
      or by an individual or Legal Entity authorized to submit on behalf of
      the copyright owner. For the purposes of this definition, "submitted"
      means any form of electronic, verbal, or written communication sent
      to the Licensor or its representatives, including but not limited to
      communication on electronic mailing lists, source code control systems,
      and issue tracking systems that are managed by, or on behalf of, the
      Licensor for the purpose of discussing and improving the Work, but
      excluding communication that is conspicuously marked or otherwise
      designated in writing by the copyright owner as "Not a Contribution."

      "Contributor" shall mean Licensor and any individual or Legal Entity
      on behalf of whom a Contribution has been received by Licensor and
      subsequently incorporated within the Work.

   2. Grant of Copyright License. Subject to the terms and conditions of
      this License, each Contributor hereby grants to You a perpetual,
      worldwide, non-exclusive, no-charge, royalty-free, irrevocable
      copyright license to reproduce, prepare Derivative Works of,
      publicly display, publicly perform, sublicense, and distribute the
      Work and such Derivative Works in Source or Object form.

   3. Grant of Patent License. Subject to the terms and conditions of
      this License, each Contributor hereby grants to You a perpetual,
      worldwide, non-exclusive, no-charge, royalty-free, irrevocable
      (except as stated in this section) patent license to make, have made,
      use, offer to sell, sell, import, and otherwise transfer the Work,
      where such license applies only to those patent claims licensable
      by such Contributor that are necessarily infringed by their
      Contribution(s) alone or by combination of their Contribution(s)
      with the Work to which such Contribution(s) was submitted. If You
      institute patent litigation against any entity (including a
      cross-claim or counterclaim in a lawsuit) alleging that the Work
      or a Contribution incorporated within the Work constitutes direct
      or contributory patent infringement, then any patent licenses
      granted to You under this License for that Work shall terminate
      as of the date such litigation is filed.

   4. Redistribution. You may reproduce and distribute copies of the
      Work or Derivative Works thereof in any medium, with or without
      modifications, and in Source or Object form, provided that You
      meet the following conditions:

      (a) You must give any other recipients of the Work or
          Derivative Works a copy of this License; and

      (b) You must cause any modified files to carry prominent notices
          stating that You changed the files; and

      (c) You must retain, in the Source form of any Derivative Works
          that You distribute, all copyright, patent, trademark, and
          attribution notices from the Source form of the Work,
          excluding those notices that do not pertain to any part of
          the Derivative Works; and

      (d) If the Work includes a "NOTICE" text file as part of its
          distribution, then any Derivative Works that You distribute must
          include a readable copy of the attribution notices contained
          within such NOTICE file, excluding those notices that do not
          pertain to any part of the Derivative Works, in at least one
          of the following places: within a NOTICE text file distributed
          as part of the Derivative Works; within the Source form or
          documentation, if provided along with the Derivative Works; or,
          within a display generated by the Derivative Works, if and
          wherever such third-party notices normally appear. The contents
          of the NOTICE file are for informational purposes only and
          do not modify the License. You may add Your own attribution
          notices within Derivative Works that You distribute, alongside
          or as an addendum to the NOTICE text from the Work, provided
          that such additional attribution notices cannot be construed
          as modifying the License.

      You may add Your own copyright statement to Your modifications and
      may provide additional or different license terms and conditions
      for use, reproduction, or distribution of Your modifications, or
      for any such Derivative Works as a whole, provided Your use,
      reproduction, and distribution of the Work otherwise complies with
      the conditions stated in this License.

   5. Submission of Contributions. Unless You explicitly state otherwise,
      any Contribution intentionally submitted for inclusion in the Work
      by You to the Licensor shall be under the terms and conditions of
      this License, without any additional terms or conditions.
      Notwithstanding the above, nothing herein shall supersede or modify
      the terms of any separate license agreement you may have executed
      with Licensor regarding such Contributions.

   6. Trademarks. This License does not grant permission to use the trade
      names, trademarks, service marks, or product names of the Licensor,
      except as required for reasonable and customary use in describing the
      origin of the Work and reproducing the content of the NOTICE file.

   7. Disclaimer of Warranty. Unless required by applicable law or
      agreed to in writing, Licensor provides the Work (and each
      Contributor provides its Contributions) on an "AS IS" BASIS,
      WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or
      implied, including, without limitation, any warranties or conditions
      of TITLE, NON-INFRINGEMENT, MERCHANTABILITY, or FITNESS FOR A
      PARTICULAR PURPOSE. You are solely responsible for determining the
      appropriateness of using or redistributing the Work and assume any
      risks associated with Your exercise of permissions under this License.

   8. Limitation of Liability. In no event and under no legal theory,
      whether in tort (including negligence), contract, or otherwise,
      unless required by applicable law (such as deliberate and grossly
      negligent acts) or agreed to in writing, shall any Contributor be
      liable to You for damages, including any direct, indirect, special,
      incidental, or consequential damages of any character arising as a
      result of this License or out of the use or inability to use the
      Work (including but not limited to damages for loss of goodwill,
      work stoppage, computer failure or malfunction, or any and all
      other commercial damages or losses), even if such Contributor
      has been advised of the possibility of such damages.

   9. Accepting Warranty or Additional Liability. While redistributing
      the Work or Derivative Works thereof, You may choose to offer,
      and charge a fee for, acceptance of support, warranty, indemnity,
      or other liability obligations and/or rights consistent with this
      License. However, in accepting such obligations, You may act only
      on Your own behalf and on Your sole responsibility, not on behalf
      of any other Contributor, and only if You agree to indemnify,
      defend, and hold each Contributor harmless for any liability
      incurred by, or claims asserted against, such Contributor by reason
      of your accepting any such warranty or additional liability.

   END OF TERMS AND CONDITIONS
```
