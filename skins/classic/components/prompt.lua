-- The lines over a list: the prompt (Bold 32, white) and lines laid out
-- with it set lighter, each a pixel under its place and centred as stock
-- 6.5's rows. A line is { text, baseline, colour, size }; lines above
-- `top` are not drawn.
local Component = require("components.component")
local theme = require("theme")

local Prompt = Component:extend({ font = theme.fonts.label, light = theme.fonts.body, size = 32, color = "#ffffff", top = 20 })

-- `lines` moved up by `offset`, those past the `bold`th set lighter.
function Prompt:draw(gfx, ui, c, lines, bold, offset)
  local px = ui.face.scale or 1
  local text = self.registry().text
  for i, p in ipairs(lines) do
    local base = p[2] - offset + 1 / px
    if base > self.top then
      text:extend({ font = i > bold and self.light or self.font, size = p[4] or self.size, color = p[3] or self.color })
          :draw_centred(gfx, ui, p[1], c, base)
    end
  end
end

return Prompt
