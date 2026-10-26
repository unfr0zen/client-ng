-- Classic: the stock thermostat's look, redrawn in vectors and the unit's own typeface.
local home = require("home")
local screens = require("screens")

return {
  name = "Classic",
  api = 1,
  -- Drawn for a face 480 units across; the engine scales it to the panel.
  design = 480,
  -- Stock's Akkurat Nest, from the unit itself; Inter where it is absent
  -- (it is not ours to ship).
  fonts = {
    regular = { "unit:AkkuratNest-Regular.ttf", "fonts/Inter-Regular.ttf" },
    medium = { "unit:AkkuratNest-Medium.ttf", "fonts/Inter-SemiBold.ttf" },
    bold = { "unit:AkkuratNest-Bold.ttf", "fonts/Inter-SemiBold.ttf" },
  },
  draw = screens(home),
}
