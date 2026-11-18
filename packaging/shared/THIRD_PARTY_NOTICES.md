# Third-party notices

OpenPencil ships some third-party content next to the program. This file is
the single list of what that content is, where it came from and under which
licence. It is staged into every distribution by `scripts/stage-notices.sh`
(macOS `Contents/Resources/notices`, Linux `/usr/share/doc/openpencil/notices`,
Windows `<install dir>\notices`, and the release archives), and the
**File > Third-party notices** menu item opens it.

Rules for maintainers:

- Every record in `crates/op-ai-skills/data/design-data.json` carries a
  `source` object (`name`, `url`, `commit`, `licenseSpdx`, `retrieved`).
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
| Design data: OpenPencil seed | Written for OpenPencil from general design knowledge, no third-party dataset | MIT (this project) | none needed |

## OpenPencil seed

The six seed records in `design-data.json` (`source.name` = `OpenPencil seed`)
are original work of the OpenPencil project, released under the MIT licence of
this repository. They contain no third-party text.

<!--
Added in the next step (import of the UI UX Pro Max dataset). Not claimed yet:
no record uses this source, so nothing is attributed until it is imported.
Replace this comment with the real section: the heading below, the upstream
copyright line, the pinned commit and the full MIT licence text.

## UI UX Pro Max
-->
