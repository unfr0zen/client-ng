-- A line of text in one style (`font`, `size`, `color`), drawn as stock
-- 6.5 places text: centred on x, it starts at its width in whole pixels
-- halved, back from x, down to the whole pixel.
local Component = require("components.component")
local theme = require("theme")

local Text = Component:extend({ font = theme.fonts.label, size = 32, color = "#ffffff" })

-- The style the painter takes, with `color` in place of this one's.
function Text:style(color)
  return { font = self.font, size = self.size, color = color or self.color }
end

-- Where `text` starts centred on `x`, and its width, in this style.
function Text:centred(gfx, ui, text, x)
  local px = ui.face.scale or 1
  local w = math.floor(gfx:measure(text, self:style()) * px + 0.5) / px
  return math.floor((x - w / 2) * px) / px, w
end

-- `text` centred on `x` with its baseline on `y`; where it starts and its width.
function Text:draw_centred(gfx, ui, text, x, y, color)
  local left, w = self:centred(gfx, ui, text, x)
  gfx:text(text, left, y, self:style(color))
  return left, w
end

return Text
