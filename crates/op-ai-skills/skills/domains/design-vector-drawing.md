---
name: design-vector-drawing
description: Drawing icons, logo marks and small illustrations from shapes and short paths - library first, a written plan, one style per set, zoomed checks
phase: [generation]
trigger:
  keywords: [logo, logos, logomark, logo mark, brand mark, emblem, mascot, monogram, pictogram, glyph, favicon, app icon, icon set, icon pack, icon family, set of icons, set of 4 icons, set of 5 icons, set of 6 icons, set of 8 icons, set of 10 icons, set of 12 icons, set of 16 icons, set of 20 icons, set of 24 icons, custom icon, custom icons, draw an icon, draw icons, design an icon, design icons, create an icon, create icons, illustration, illustrations, svg, vector art, vector graphic, vector graphics, vector illustration, vector icon, vector icons, 图标设计, 绘制图标, 插画, 矢量]
priority: 22
budget: 650
category: domain
---

VECTOR DRAWING (icons, marks, small illustrations; build from nodes, never trace or copy an existing brand's mark)

1. LIBRARY FIRST. If Lucide or Feather has the glyph, use icon_font with the real name (iconIssues lists near misses). Draw only what they lack.
2. PLAN IN WORDS before any node: size, margin, silhouette, one idea. Work out every coordinate from the grid; do not guess.
3. ICON = 24 px box, 20x20 live area (2 px margin), 2 px stroke, one corner radius, one colour. About 12 shapes at most. Overlap contact points; no tangents. Nodes have no cap or join setting, so check line ends in the zoom and cap an ugly one with a small filled ellipse.
4. SHAPES FIRST: rectangle, ellipse, line, polygon, then a path with few commands. A path d is in the node's own px from its top-left corner. A bigger illustration gets more shapes, one focal point and a limited palette.
5. ONE STYLE per set (stroke, radius, margin, fill or outline), written down once and reused. Build 5 at a time, then one screenshot of the batch.
6. CHECK ZOOMED: get_screenshot with scale 8-16 on the small node. Silhouette reads, nothing clipped, even stroke, centred in the live area, layoutIssues empty. At most 2 correction rounds, one change each, then stop and say what is left.
7. LOGO = simple geometric mark plus a wordmark in real text nodes, readable in one colour.
8. import_svg drops transform, text, use and gradients (a gradient fill draws black). Read skipped in its result and redraw those parts.
