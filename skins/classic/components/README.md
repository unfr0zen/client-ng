# Classic's menu components

Each component is a table of parameters (sizes, colours, fonts, matched on stock 6.5) plus draw methods. A variant overrides parameters and inherits the rest:

```lua
local Pill = require("classic:components.pill")
local Square = Pill:extend({ radius = 4, color = "#ff8800" })
```

Table parameters are replaced whole by `extend`, not merged.

## Restyling from another skin

The menus look components up in `components.lua` on every draw, so replacing one there (in `skin.lua`) restyles it everywhere:

```lua
local components = require("classic:components")
components.pill = components.pill:extend({ color = "#e5c07b" })
components.tile = components.tile:extend({ color = "#3e4451", symbol_color = "#61afef" })
components.list.picks = { question = 75, per_row = 12, rows = 5, gap = 68, last_question = 407, last = 374, top = 82, bare = 77 }
```

Components that draw others (a button's pill, a list's rows) take them from the registry unless a variant names its own (`pill = ...`).

## The components

| Name | Draws | Main parameters |
| --- | --- | --- |
| `text` | a line in one style, centred as stock places text | `font`, `size`, `color` |
| `pill` | the highlight behind a selected entry | `height`, `radius`, `left`, `right`, `color` |
| `button` | a choice's label, on its pill while selected | `size`, `color`, `off`, `rise`, `snap` |
| `heading` | a heading among a list's rows (a `text`) | as `text` |
| `prompt` | the lines over a list, detail lines lighter | `font`, `light`, `size`, `color` |
| `list_row` | an entry of a list: tick, symbol, a network's marks, ellipsis, pill | `size`, `color`, `on_color`, `pill_*`, `mark_*`, `widest` |
| `list` | a list screen; its layouts (`list.layouts`, picked by `list.order`) each with its numbers (`list.picks`, `list.answers`, `list.network`, ...) | `first`, `row`, `prompt_*`, `bottom`, `section`, `ring`, `keep*` |
| `tile` | one of Settings' tiles, DONE's and Back's | `radius`, `color`, `name`, `value`, `middle`, `step`, `symbol_*`, `done_*`, `number` |
| `carousel` | Settings' title, tile step and rim | `step`, `title`, `title_y`, `rim*` |
| `ring` | the top menu: symbols round the face, the middle, Back's clock, weather and room | `slots`, `radius*`, `icon*`, `title`, `sub`, `clock`, `weather`, `room` |
| `field_box` | a form's value in its box | `height`, `top`, `radius`, `border`, `fill`, `changing` |
| `form_row` | a form's row (`form_row.symbol`, `form_row.count` for stock's variants) | `width`, `height`, `gap`, `top`, `radius`, `label_at` |
| `confirm_ring` | the confirm page's ring (Reset > Restart) | `radius`, `width`, `angle`, `track`, `fill` |
| `dial` | the fan timer, setting and running | `radius`, `width`, `angle`, `value`, `hint`, `back_base`, `stop_base` |
| `info_page` | a page of text over the menus | `text`, `line`, `middle`, `symbol`, `button`, `picture*` |
| `keyboard` | stock's keyboard (its keys' symbols in `draw_key`) | `radius`, `start`, `pad`, `arc*`, `at`, `field`, `caption` |

- `component.lua`: the base all extend. `format.lua`: temperatures and time.
- Units are the 480 design units; pixel snapping goes through `ui.face.scale`, so variants hold on the gen 2's smaller face.
