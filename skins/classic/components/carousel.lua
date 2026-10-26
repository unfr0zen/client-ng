-- Settings' carousel, as stock's: tiles `step` apart, the selected one in
-- the middle, the menu's name above them and the face's black rim over.
local Component = require("components.component")
local theme = require("theme")

local M = theme.menu
local Carousel = Component:extend({
  step = M.carousel_step,
  title = { font = theme.fonts.label, size = 31.5, color = M.title }, title_y = M.carousel_title,
  rim = theme.dial.rim, rim_width = theme.dial.rim_width, rim_color = "#000000",
})

-- The menu's name, a pixel under `title_y`, faded by `alpha`.
function Carousel:draw_title(gfx, ui, name, alpha)
  gfx:save()
  gfx:alpha(alpha or 1)
  self.registry().text:extend(self.title):draw_centred(gfx, ui, name, ui.face.size / 2, self.title_y + 1 / (ui.face.scale or 1))
  gfx:restore()
end

function Carousel:draw_rim(gfx, ui)
  local c = ui.face.size / 2
  gfx:ring(c, c, self.rim, self.rim_width, self.rim_color)
end

return Carousel
