-- A row of stock's form (Eco, Safety Temp, Early-On): a label, a gap and
-- a box `width` by `height`, the row centred on the face. With `snap` the
-- row is laid out in whole pixels; `label_at` "end" ends the label at the
-- gap, "start" starts it the row. A symbol, when the form has one, sits
-- in the box before the value.
local Component = require("components.component")
local theme = require("theme")
local icons = require("icons")

local FormRow = Component:extend({
  width = 150, height = 51, gap = 21, top = 26, radius = 11, edge = 2,
  label_at = "start", snap = true, text_dy = 11, icon_dy = 12,
  size = 32, border = theme.menu.pill, idle_border = "#666666", changing = "#003d55", fill = theme.colors.menu,
  color = "#ffffff", idle_color = "#6e6e6e",
  icon_x = 31.5, icon_size = 22.5, icon_shift = 20,
})

-- Stock's variants: with a symbol in the box, and a count (no colon).
FormRow.symbol = FormRow:extend({ width = 155, gap = 22, top = 25, label_at = "end" })
FormRow.count = FormRow:extend({ width = 152, height = 50, gap = 17, top = nil, radius = 9, label_at = "end", snap = false })

-- The row on `y`, centred on `c`: `name` in `label_color`, `value` in the
-- box, with `icon` before it when given.
function FormRow:draw(gfx, ui, c, y, name, value, label_color, selected, changing, icon)
  local px = ui.face.scale or 1
  local label = { font = theme.fonts.label, size = self.size, color = label_color, align = "right" }
  local lw = gfx:measure(name, label)
  if self.snap then lw = math.floor(lw * px + 0.5) / px end
  local x0 = c - (lw + self.gap + self.width) / 2 + lw + self.gap
  if self.snap then x0 = math.floor(x0 * px + 0.5) / px end
  local dy = icon and self.icon_dy or self.text_dy
  if self.label_at == "end" then
    gfx:text(name, x0 - self.gap, y + dy, label)
  else
    gfx:text(name, x0 - self.gap - lw, y + dy, { font = label.font, size = label.size, color = label.color })
  end
  local top = y - (self.top or self.height / 2)
  gfx:rect(x0, top, self.width, self.height, selected and self.border or self.idle_border, self.radius)
  gfx:rect(x0 + self.edge, top + self.edge, self.width - 2 * self.edge, self.height - 2 * self.edge,
           (selected and changing) and self.changing or self.fill, self.radius - self.edge)
  local color = selected and self.color or self.idle_color
  if icon then
    icons.draw(gfx, icon, x0 + self.icon_x, y, self.icon_size, { p = 1 })
    gfx:text(value, x0 + self.width / 2 + self.icon_shift, y + self.icon_dy, { font = theme.fonts.label, size = self.size, color = color, align = "center" })
  else
    gfx:text(value, x0 + self.width / 2, y + self.text_dy, { font = theme.fonts.label, size = self.size, color = color, align = "center" })
  end
end

return FormRow
