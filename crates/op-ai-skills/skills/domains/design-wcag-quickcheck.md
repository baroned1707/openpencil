---
name: design-wcag-quickcheck
description: Ten accessibility checks, by WCAG 2.2 criterion number, that can be judged on a vector design
phase: [generation]
trigger:
  keywords: [accessibility, accessible, a11y, wcag, contrast, screen reader, colour blind, color blind, colorblind, 无障碍]
priority: 16
budget: 500
category: domain
---

ACCESSIBILITY QUICK CHECK (WCAG 2.2 level AA; criterion numbers and thresholds only. Judge each on the nodes and fix before finishing.)

1. 1.4.3 Text contrast: body text 4.5:1 against its real background; large text (24px and up, or bold 19px and up) 3:1. Test text over images and gradients at the worst spot.
2. 1.4.11 Control contrast: borders of inputs and buttons, meaningful icons, chart marks and focus rings 3:1 against what is behind them.
3. 1.4.1 Colour alone: every status, error, selection and chart series also has a label, icon, shape or pattern.
4. 2.5.8 Target size: clickable targets 24x24 px at least, or spaced so a 24px circle fits; make touch targets 44 px (advice from 2.5.5). A small icon may sit in a larger hit frame.
5. 1.4.4 and 1.4.12 Resize: text must survive 200% scaling and looser spacing, so no fixed-height text frames and nothing clipped.
6. 3.3.1 and 3.3.2 Inputs: a visible label on every field (a placeholder is not a label), format hints, and error text that names the problem and the fix.
7. 2.4.7 Focus: design or note a visible focus ring on each interactive component; keep reading order top to bottom, left to right.
8. 1.1.1 Names: icon-only buttons and meaningful images get a name stating the action ("Close", "Search").
9. 3.2.6 Help: help or contact stays in the same place on every screen.
10. 2.5.7 Dragging: anything done by dragging (slider, reorder, map pan) has a single-tap alternative.

State any check you could not satisfy in one line.
