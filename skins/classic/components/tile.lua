-- One of Settings' tiles, as stock's: a circle with its symbol and lines
-- (the name, then what it says), DONE's ring round the target, or Back's.
local Component = require("components.component")
local theme = require("theme")
local icons = require("icons")

local M = theme.menu
local function fit(gfx, line, style, width)
  local paint = style:style()
  if gfx:measure(line, paint) <= width then return line end
  local dots = "\u{2026}"
  if gfx:measure(dots, paint) > width then return "" end
  repeat
    line = line:gsub("[%z\1-\127\194-\244][\128-\191]*$", "")
  until line == "" or gfx:measure(line .. dots, paint) <= width
  return line .. dots
end

local Tile = Component:extend({
  radius = M.circle, color = M.circle_color,
  -- The lines centred on `middle`, `step` apart: the name, then the value.
  middle = 248, step = M.carousel_line,
  name = { font = theme.fonts.label, size = 35, color = "#ffffff" },
  value = { font = theme.fonts.body, size = 31, color = M.circle_text },
  -- A symbol over the name, or a plugin's picture.
  symbol_y = 150, symbol_size = 47.5, symbol_color = "#bfeaff",
  picture_y = 105, picture_size = 80,
  -- A network's name after the Wi-Fi symbol.
  wifi_dx = 15, wifi_dy = 10, wifi_size = 24, wifi_shift = 14,
  -- DONE: a ring round the target (or, the system running, a disc of the
  -- home screen's colour), a half as a raised 5, the leaf under it.
  done_radius = M.done_radius, done_width = M.done_width, done_color = M.done_color,
  number = { font = theme.fonts.number, size = 97.4, base = 275 },
  half = { font = theme.fonts.label, size = 48, base = 241, shift = 10 },
  leaf_y = 338, leaf_size = 29.5,
  -- Back: a ring and the arrow, in the options' grey.
  back_width = 6, back_icon = 64, back_color = M.option,
})

-- DONE centred on (x, c): the target `whole`, with a raised 5 when `half`.
function Tile:draw_done(gfx, x, c, whole, half, leaf, home_color)
  if home_color ~= "#000000" then
    gfx:circle(x, c, self.radius, home_color)
  else
    gfx:ring(x, c, self.done_radius, self.done_width, self.done_color)
  end
  local big = { font = self.number.font, size = self.number.size, color = "#ffffff", align = "center" }
  local w = gfx:measure(whole, big)
  gfx:text(whole, x - (half and self.half.shift or 0), self.number.base, big)
  if half then
    gfx:text("5", x - self.half.shift + w / 2 + 1, self.half.base, { font = self.half.font, size = self.half.size, color = "#ffffff" })
  end
  if leaf then icons.draw(gfx, "leaf", x, self.leaf_y, self.leaf_size, { p = 1 }) end
end

-- Back centred on (x, c).
function Tile:draw_back(gfx, x, c)
  gfx:ring(x, c, self.radius - self.back_width / 2, self.back_width, self.back_color)
  icons.draw(gfx, "back", x, c, self.back_icon, { color = self.back_color })
end

-- An entry centred on (x, c): `lines` (the first `named` its name; a table
-- { text, wifi = true } a network), under `symbol` or a plugin's `picture`.
function Tile:draw(gfx, ui, x, c, lines, named, symbol, picture)
  gfx:circle(x, c, self.radius, self.color)
  if picture then
    gfx:image(picture, x - self.picture_size / 2, self.picture_y, self.picture_size, self.picture_size)
  elseif symbol then
    local icon = icons[symbol .. "_tile"] and symbol .. "_tile" or symbol
    icons.draw(gfx, icon, x, self.symbol_y, self.symbol_size, { color = self.symbol_color, bg = self.color })
  end
  local text = self.registry().text
  local base = self.middle - (#lines - 1) * self.step / 2 + (picture and 18 or 0)
  for j, line in ipairs(lines) do
    local style = text:extend(j <= named and self.name or self.value)
    local y = base + (j - 1) * self.step
    local far = math.max(math.abs(y - style.size * 0.8 - c), math.abs(y + style.size * 0.2 - c))
    local width = 2 * math.sqrt(math.max(0, self.radius * self.radius - far * far)) - 24
    if type(line) == "table" then
      local label = fit(gfx, line.text, style, width - 54)
      local w = gfx:measure(label, style:style())
      icons.draw(gfx, "wifi", x - w / 2 - self.wifi_dx, y - self.wifi_dy, self.wifi_size, { color = self.value.color, bg = self.color })
      style:draw_centred(gfx, ui, label, x + self.wifi_shift, y)
    else
      style:draw_centred(gfx, ui, fit(gfx, line, style, width), x, y)
    end
  end
end

return Tile
