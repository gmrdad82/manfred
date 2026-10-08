# The look

The tokens below are Manfred's own, `pub const` in `crates/ui/src/theme.rs`, with `manfred()` building its `Theme`; the fonts are in `assets/fonts/`. pfx's `pfx-editor-style` (pinned in the root `Cargo.toml` at the same rev as every other pfx crate) holds the `Theme` struct and the widgets that paint from it, and bundles no tokens, fonts or default theme.

Manfred's window has an "engineering blueprint" look: a deep navy field, one monospace family everywhere, thin borders, square corners, amber for the current thing, panels that carry a code and answer a key. The tokens below were sampled from the design mockups; tune them against window shots, never against memory.

## Colour tokens

| Token | Value | Use |
|---|---|---|
| `bg.base` | `#02264A` | window and panel fill |
| `bg.deep` | `#021A39` | title bar, status bar, text fields, the viewport's passepartout |
| `bg.chip` | `#0B3159` | chips, badges, inactive buttons, tabs at rest |
| `bg.selected` | `#123756` | the selected row when it is not the focused panel; hovered rows |
| `line.border` | `#244D6A` | 1 px borders on panels and widgets |
| `line.subtle` | `#1F3F5E` | separators, dotted guides, grid lines |
| `line.strong` | `#366288` | the focused panel's border |
| `text.primary` | `#DAE5F0` | body text |
| `text.code` | `#A9D0E4` | values, numbers, paths, names |
| `text.secondary` | `#93B3D0` | labels, panel titles |
| `text.muted` | `#6591AD` | hints, timestamps, units |
| `text.dim` | `#4F7293` | panel codes, disabled text |
| `accent` | `#F2BF58` | the selected row in the focused panel, the primary button, the active tab's underline, the current branch box, gizmo handles |
| `accent.text` | `#021A39` | text on `accent` |
| `ok` | `#5EB586` | success, additions, a green check |
| `ok.tint` | `#0F4247` | an added row's background |
| `danger` | `#E5667A` | errors, deletions |
| `danger.tint` | `#3B233A` | a removed row's background |
| `info` | `#74A6C4` | links, references, object names in prose |

Dark only. No gradients, no blur, no shadows anywhere.

## Typography

- **One family, monospace, for everything:** every label, value and heading is set in it. It is JetBrains Mono, in its Nerd Font build so the icons the panels need come with the font and no icon set is shipped. Regular and Bold are committed in `assets/fonts/` with their `SOURCE` line and `OFL.txt` (SIL Open Font License 1.1) and embedded with `include_bytes!`, so `--shot` renders the same on any machine and the window never depends on the fonts the system has installed.
- **Sizes in points at 1x:** `code` 11, `small` 12, `body` 13, `heading` 16, `title` 20. Line height 1.45.
- **Panel titles** are uppercase in `text.secondary`; their code (`B1`) sits left in `text.dim` with a thin box. egui has no letter spacing, so uppercase titles are set plain, or painted glyph by glyph in the header widget if the spacing matters.
- **Numbers** right-align in `text.code`; units follow in `text.muted`.

## Shape and spacing

- Corner radius 2 px on everything. Strokes 1 px. Pixel-align strokes to half pixels and keep `pixels_per_point` integer so 1 px stays 1 px.
- An 8 px grid: panel padding 10, item spacing 6, section gap 16.
- Window and popup shadows off. Separators are 1 px `line.subtle`; dotted guides use `dashed_line` with 2 px dashes.
- Focus is a `line.strong` border on the panel plus the accent row inside it; there is no focus ring on single widgets beyond egui's stroke in `accent`.

## Panels and keys

- Every panel has a code: `A` the top bar, `B1` outliner, `B2` cameras, `B3` lights and sky, `C1` viewport, `C2` properties, `C3` render, `D1` status. The code is painted in the header; pressing the code's letter then digit jumps focus there.
- A bottom hint bar lists the keys that work in the focused panel, each as a `bg.chip` key cap followed by its word.
- The status bar carries: recipe path, dirty state (never, by design, so it shows the last patch instead), the engine's frame time, the state of the optional GPU queue `pgpu` (`free`, `queued`, `play`), and the version, as `manfred vX.Y.Z`.

## egui mapping

- `Visuals`: `panel_fill` and `window_fill` = `bg.base`; `extreme_bg_color` = `bg.deep`; `faint_bg_color` = `bg.selected`; `widgets.noninteractive.bg_stroke` = `line.border`; `widgets.inactive.weak_bg_fill` = `bg.chip`; `widgets.hovered.bg_fill` = `bg.selected`; `widgets.active.bg_fill` = `accent` with `fg_stroke` = `accent.text`; `selection.bg_fill` = `accent`, `selection.stroke` = `accent.text`; `hyperlink_color` = `info`; `window_shadow` and `popup_shadow` = none; `window_stroke` = `line.border`; every `corner_radius` 2; `striped` rows with `faint_bg_color`.
- `Style`: `spacing.item_spacing` 6, `spacing.window_margin` 10, `spacing.button_padding` [8, 3]; `text_styles` to the five sizes above, all in the one family; `FontDefinitions` with the TTF under both `Proportional` and `Monospace`.
- Custom widgets, in pfx's `pfx-editor-style` (`widgets/`), shared by the editors built on it and painted from Manfred's `Theme`: panel header with code, tab bar with underline, key cap, status pill, dotted separator, numeric field with drag and unit.
- What egui gives for free and the theme keeps: hover, focus, tooltips, keyboard navigation, text editing with selection and clipboard, tables, scroll areas, colour picker, drag values.
- What egui does not do: letter spacing, ligatures, subpixel text, 3D. The viewport is an engine texture inside egui; nothing egui draws is in the scene.
