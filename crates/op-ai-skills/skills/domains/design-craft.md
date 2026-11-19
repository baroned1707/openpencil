---
name: design-craft
description: Quality floor and pre-finish self-review for new screens and pages - one direction, type, spacing, colour roles, real content, states, and what reads as generic AI output
phase: [generation]
trigger:
  keywords: [screen, page, app, dashboard, landing, ui, ux, interface, website, web app, saas, mobile, homepage, mockup, wireframe, form, login, signup, checkout, onboarding, settings, 界面, 页面, 应用]
priority: 12
budget: 1300
category: domain
---

CRAFT FLOOR AND SELF-REVIEW (screens and pages; decks, cards and logos follow their own contracts. The user's design.md, pinned style and existing screens always outrank this.)

DIRECTION. Before the first batch, choose ONE memorable idea for this product (a type voice, a colour move, or a layout structure) and carry it through every section. Spend boldness in one place and keep the rest quiet. Draw it from the subject and its audience, not from habit. Then ask: is this the generic default for this kind of product? Name one decision that only fits THIS product (a word, a layout, a colour move), and replace the default if you cannot.

TYPE. At most 3 sizes inside one section, with clearly different steps (about 1.25x or more) and a weight change doing part of the work. One family, or two that are clearly distinct. Line height is a ratio: about 1.1-1.2 for display text, 1.4-1.6 for body. Keep body lines to roughly 45-75 characters. Do not accent a single headline word, and do not default to tracked all-caps labels.

SPACING. Pick a base unit (8, or 4 for dense UI) and take every gap and padding from its multiples. Tight inside a group, generous between groups, more room above a heading than below. Padding lives on frames; never fake it with empty rectangles or spacer text.

COLOUR. Assign roles before painting: background, surface, text, muted text, accent, semantic (success, warning, error). One accent, spent on the primary action and the state that matters. Muted text is a tint of the text hue, not a flat grey. Contrast floors: 4.5:1 body text, 3:1 large text, icons and control borders. Meaning is never carried by colour alone.

CONTENT. No lorem ipsum. Labels name the action ("Save changes", not "Submit"). Names, dates and numbers are believable and differ from row to row. Wherever data appears, design the empty, loading and error state as well, each saying what the user can do next.

AVOID by default: a card around every element; three identical cards under a centred hero; purple-to-pink gradients on white; gradient text; emoji or text glyphs as icons (use icon_font); decorative glass, blur and stacked soft shadows; big number plus small label plus sparkline as the stock stat; 01/02/03 markers on content that is not a sequence; an eyebrow label above every heading. Use one only when the brief earns it.

BEFORE YOU FINISH, build the wrapper frame first, then the sections, and check each section with snapshot_layout. Take one full get_screenshot after the first complete build (when screenshots exist), fix what it shows with targeted edits, and take at most one more. Read fontIssues for fonts: it must be empty, and the family must match the brief.
1. Squint test: primary element, secondary element and groups are identifiable in that order.
2. No section has more than 3 type sizes, and gaps are multiples of the base unit.
3. Every text and background pair meets the contrast floors; no state is colour-only.
4. Content is real and varied; empty, loading and error states exist where data appears.
5. No clipped or collapsed nodes; interactive targets are at least 24x24 px (44 on touch screens).
6. Nothing from the AVOID list survived by default. If the prompt names accessibility, also apply the accessibility quick check.
Fix what fails, then move on; do not re-audit sections that passed. If one issue survives two fixes, stop patching: re-plan that part.
