---
name: ui
title: Interface design
description: Build or polish a UI with consistent spacing, type and controls, and a look of its own.
---
Design the interface deliberately. Do not ship framework defaults or the interchangeable "AI-made" look. What the user asks for and the product's existing design outrank everything below.

## Before writing code
- Look at what exists: the stack, the tokens or variables, the components, the screens next to the one you touch. Extend that system instead of starting a second one.
- Pick a direction and state it in one sentence (for example "dense and quiet like a tool, warm grey, one green accent"). Give it a point of view: a typeface, an accent colour, a shape language. Hold every decision to it. If the user names a style, use theirs.

## Define the system once, then use only it
Put these in variables or tokens. No one-off values afterwards.
- Spacing: one scale, e.g. 4, 8, 12, 16, 24, 32, 48. Related things sit closer than unrelated ones. The same kind of gap is the same everywhere.
- Type: one UI family (plus a monospace if needed), at most five sizes and two weights. Body 14–16px with line height about 1.5; headings about 1.2.
- Colour: neutrals for most of the surface. One accent, used only for the primary action and for selection. Separate colours for danger and success. Text contrast at least 4.5:1 (3:1 for large text and for borders that carry meaning). If the app has light and dark themes, both must work.
- Shape: one radius scale, one border colour, at most one shadow level.
- Controls: one button system (primary, secondary, quiet, danger) sharing height, padding, radius and font. One primary button per view. Inputs are as tall as buttons. Every interactive element has hover, active, focus-visible and disabled states. Hit targets at least 32px, 44px on touch.

## Layout and hierarchy
- Show importance through size, weight and position, not through more colour. Align to a grid, keep page margins constant, left-align text, keep lines under about 70 characters.
- Design the empty, loading, error and overflow states (long text, many items, a narrow window), not only the full happy path.
- It works from 360px wide upwards with no horizontal scroll: fluid widths, content wraps instead of shrinking.

## Leave out
- Purple or blue gradients, gradient text, glows, glass effects nobody asked for, emoji as icons, pills everywhere, a grid of cards for everything, everything centred, shadows on everything, decorative animation, hype copy.
- Explanatory tips, helper paragraphs and onboarding blurbs. If a control needs explaining, rename it or redesign it. Labels are short and in sentence case: "Save", "Model".

## Build quality
- Semantic HTML: `button` for actions, `a` for navigation, `label` tied to its input, headings in order. Everything is reachable by keyboard in a sensible order, the focus ring is visible, icon-only buttons have an accessible name.
- Transitions only on state changes and at most 200ms. Respect `prefers-reduced-motion`.
- No inline styles, no `!important`, no dead CSS. Keep every id, class and attribute that scripts or tests rely on. Behaviour stays the same unless a change was asked for.

## Before finishing
- Look at the result. Run the app or open the page headless and take a screenshot when the workspace allows it. Otherwise read the markup and styles from top to bottom.
- Hunt specifically for: a value off the spacing scale, a second button style, a control without its states, contrast below 4.5:1, a leftover inline style, breakage at 360px.
- In the summary give the direction you chose, what changed, and what you could not check visually.
