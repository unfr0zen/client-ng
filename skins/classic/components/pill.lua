-- The highlight behind a selected entry: a rounded bar `height` tall,
-- reaching `left` and `right` past the text it is behind.
local Component = require("components.component")
local theme = require("theme")

local Pill = Component:extend({ height = 48, radius = 24, left = 24, right = 24, color = theme.menu.pill })

-- Behind a text starting at `x`, `w` wide, the bar's top on `top`.
function Pill:draw(gfx, x, top, w)
  gfx:rect(x - self.left, top, w + self.left + self.right, self.height, self.color, self.radius)
end

return Pill
