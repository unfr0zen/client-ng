-- Nest 4th gen: the 2024 Nest Learning Thermostat's home screen, black
-- with a glow in the colour of what the system is doing, set in Figtree;
-- the menus are Classic's.
local home = require("home")
local screens = require("classic:screens")

return {
  name = "NEST 4TH GEN",
  api = 1,
  design = 480,
  fonts = {
    figtree_light = "fonts/Figtree-Light.ttf",
    figtree = "fonts/Figtree-Regular.ttf",
    figtree_medium = "fonts/Figtree-Medium.ttf",
    -- Classic's, for its menus.
    regular = { "unit:AkkuratNest-Regular.ttf", "classic:fonts/Inter-Regular.ttf" },
    medium = { "unit:AkkuratNest-Medium.ttf", "classic:fonts/Inter-SemiBold.ttf" },
    bold = { "unit:AkkuratNest-Bold.ttf", "classic:fonts/Inter-SemiBold.ttf" },
  },
  draw = screens(home),
}
