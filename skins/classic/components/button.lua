-- A choice's label, on a pill while selected: stock's DONE, MORE INFO,
-- CANCEL and the like. `rise` is the pill's top above the baseline; with
-- `snap`, the label starts where stock's do (see Text), else at its exact
-- half width back from the middle.
local Component = require("components.component")
local theme = require("theme")

local Button = Component:extend({ size = 35.7, color = "#ffffff", off = theme.menu.option, rise = 36, snap = true })

-- `label` centred on `x`, its baseline on `base`, on a pill while `on`.
function Button:draw(gfx, ui, label, x, base, on)
  local parts = self.registry()
  local text = (self.text or parts.text):extend({ size = self.size, color = self.color })
  local left, w
  if self.snap then
    left, w = text:centred(gfx, ui, label, x)
  else
    w = gfx:measure(label, text:style())
    left = x - w / 2
  end
  if on then (self.pill or parts.pill):draw(gfx, left, base - self.rise, w) end
  gfx:text(label, left, base, text:style(on and self.color or self.off))
end

return Button
