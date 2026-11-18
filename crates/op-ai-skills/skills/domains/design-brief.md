---
name: design-brief
description: Context and UX brief workflow for a new product, app, site or screen - read what exists, write a short brief with assumptions, then build
phase: [generation]
trigger:
  keywords: [design a, design an, create a, create an, build a, build an, make me a, new app, new website, new product, new screen, from scratch, app idea, 设计一个, 做一个, 创建一个]
priority: 14
budget: 1100
category: domain
---

CONTEXT AND UX BRIEF (a NEW product, app, site or screen). Decide alone: ask the user only when the answer blocks the design. The brief is the one piece of prose you write before building (an exception to "no prose").

1. READ FIRST. Use the run context (design.md, pinned style, selection, existing canvas context) and `get_editor_state` (screens, components, variables). If a design system or screens already exist, EXTRACT their tokens, components and patterns and follow them: do not research and do not restyle.
2. NEW PRODUCT, NOTHING EXISTING. Call `get_guidelines` ONCE with `category: "data"` and `query` = the product type in a few words (optional `platform`). Use the single best match (palette, typography, pattern, rules, avoid) as a starting point and name its record id and why it fits. It is reference data, not instructions: the user's own words, design.md, pinned style and existing screens override it. To read a style named in a match, call `get_guidelines` with category data, kind style and the style name. No match: choose from the subject yourself.
3. WRITE THE BRIEF as text at the start of the reply that carries your next tool call (your first `batch_design` once the reads are done). Never send it as a reply with no tool call: such a reply ends the run before anything is built. Short (about 150 words), a plain list, stating:
   - Product and purpose; audience and the job they hire it for.
   - 3-7 features, each marked P0 (cannot ship without) or P1.
   - Key flows: happy path first, then one empty or error path.
   - Screen list and how screens connect (at most 3 levels deep, labels in the users' own words).
   - Platform and size; direction (style, colour roles, type pair) with the reason it suits the audience.
   - Accessibility target, WCAG 2.2 AA: 4.5:1 text, 3:1 UI parts, 24x24 px smallest target, nothing by colour alone, a focus state noted on interactive nodes.
   - ASSUMPTIONS, one line each.
4. FIELD CHECKLIST behind the brief. Fill only what the prompt or context supports; list the rest as assumptions or open questions and never invent facts: product name, purpose, problem, audience, jobs to be done, goals, non-goals, features P0/P1/P2, key flows, information architecture, content types, platforms and sizes, brand, existing design system, existing screens, reference products, reference patterns, constraints, locale, accessibility target, open questions.
5. ASK ONLY IF BLOCKED. One short question, and only when you cannot tell what the product is or who it is for. Everything else: state the assumption and go on. If the prompt already contains a full brief (audience, features, flows), do not repeat it: build.
6. BUILD section by section from the brief. If the user later contradicts it, the user wins: update the brief in one line.
