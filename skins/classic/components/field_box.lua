-- A form's value in a rounded box, as stock's: the edge blue while
-- selected, the inside dark blue while being changed. Centred on (x, y),
-- `extra` wider than asked, its top `top` above y.
local Component = require("components.component")
local theme = require("theme")

local FieldBox = Component:extend({
  height = 51, top = 25, extra = 1, radius = 11, edge = 2,
  border = theme.menu.pill, idle_border = "#666666",
  changing = "#003d55", fill = theme.colors.menu,
  size = 32, color = "#ffffff", idle_color = "#6e6e6e", text_dy = 12,
})

function FieldBox:draw(gfx, x, y, w, text, selected, changing)
  local left, top = x - w / 2, y - self.top
  gfx:rect(left, top, w + self.extra, self.height, selected and self.border or self.idle_border, self.radius)
  gfx:rect(left + self.edge, top + self.edge, w + self.extra - 2 * self.edge, self.height - 2 * self.edge,
           (selected and changing) and self.changing or self.fill, self.radius - self.edge)
  gfx:text(text, x, y + self.text_dy, { font = theme.fonts.label, size = self.size, color = selected and self.color or self.idle_color, align = "center" })
end

return FieldBox
