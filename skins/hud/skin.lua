-- HUD: a heads-up display in the One Dark editor palette, set in
-- JetBrains Mono; the menus are Classic's.
local home = require("home")
local screens = require("classic:screens")

return {
  name = "HUD",
  api = 1,
  design = 480,
  fonts = {
    mono = "fonts/JetBrainsMono-Regular.ttf",
    mono_light = "fonts/JetBrainsMono-Light.ttf",
    mono_medium = "fonts/JetBrainsMono-Medium.ttf",
    -- Classic's, for its menus.
    regular = { "unit:AkkuratNest-Regular.ttf", "classic:fonts/Inter-Regular.ttf" },
    medium = { "unit:AkkuratNest-Medium.ttf", "classic:fonts/Inter-SemiBold.ttf" },
    bold = { "unit:AkkuratNest-Bold.ttf", "classic:fonts/Inter-SemiBold.ttf" },
  },
  draw = screens(home),
}
