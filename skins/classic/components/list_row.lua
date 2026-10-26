-- An entry of stock's lists: its words in capitals or not, the set one
-- ticked, a symbol before some (Use Eco's leaf, Cancel's arrow), a
-- network's marks before its name; the highlighted one white on a pill.
-- The row is centred on its width in whole pixels, from the whole pixel
-- before, and set a pixel under its baseline (stock 6.5's).
local Component = require("components.component")
local theme = require("theme")
local icons = require("icons")

local M = theme.menu
local ListRow = Component:extend({
  font = theme.fonts.label, size = 35.7, color = M.option, on_color = "#ffffff",
  -- The capital height the symbols and the pill sit by.
  cap = 0.729,
  -- The pill: its top 9 over the capitals, reaching 24 past plain words,
  -- 14 round a tick or a symbol.
  pill_over = 9, pill_left = 24, pill_left_marked = 14,
  -- A leaf before an answer, its width and gap; Cancel's arrow.
  leaf_width = 27, leaf_gap = 18, back_size = 25, back_gap = 37,
  -- A network's marks: tick, star, lock (highlighted only), Wi-Fi.
  mark_tick = 41, mark_star = 33, mark_lock = 33, mark_wifi = 32, mark_size = 22, wifi_size = 26,
  -- A network's row at most this wide, its pill this far inside the face.
  widest = 360, inside = 46,
})

-- A tick, as stock marks the option that is set, its left at x on the
-- baseline y, `h` tall.
function ListRow:tick(gfx, x, y, h, color)
  local w = h * 0.12
  gfx:line(x + w, y - h * 0.45, x + h * 0.36, y - w * 0.5, w * 1.6, color, true)
  gfx:line(x + h * 0.36, y - w * 0.5, x + h * 1.02, y - h + w, w * 1.6, color, true)
end

-- The row's words on baseline `base`, centred on `c`. `o`: `tick`,
-- `icon`, `marks` (connected, saved, secured, level) and `on`.
function ListRow:draw(gfx, ui, c, base, text, o)
  local style = { font = self.font, size = self.size }
  local mark = self.size * self.cap
  local icon, marks = o.icon, o.marks
  -- Stock 6.5 ticks a row with Akkurat's own check mark.
  if o.tick then text = "\u{2713} " .. text end
  local w = gfx:measure(text, style) + (icon == "back" and self.back_gap or (icon and self.leaf_width + self.leaf_gap or 0))
  if marks then
    local locked = marks.secured and o.on
    local extra = (marks.connected and self.mark_tick or 0) + (marks.saved and self.mark_star or 0) + (locked and self.mark_lock or 0) + self.mark_wifi
    -- A name too long for the face is cut short with an ellipsis,
    -- character by character: the row `widest` at most, and highlighted,
    -- its pill inside the face where it is.
    local most = self.widest
    if o.on then
      local far = math.max(math.abs(base - mark - self.pill_over - c), math.abs(base - mark + 39 - c))
      most = math.min(most, 2 * math.sqrt(math.max(0, c * c - far * far)) - self.inside)
    end
    if gfx:measure(text, style) + extra > most then
      repeat
        text = text:gsub("[%z\1-\127\194-\244][\128-\191]*$", "")
      until text == "" or gfx:measure(text .. "\u{2026}", style) + extra <= most
      text = text .. "\u{2026}"
    end
    w = gfx:measure(text, style) + extra
  end
  local px = ui.face.scale or 1
  w = math.floor(w * px + 0.5) / px
  local x = math.floor((c - w / 2) * px) / px
  local color = self.color
  local pill = self.registry().pill
  if o.on then
    local left = (icon or o.tick) and self.pill_left_marked or self.pill_left
    pill:extend({ left = left }):draw(gfx, x, base - mark - self.pill_over, w)
    color = self.on_color
  end
  if marks then
    if marks.connected then
      self:tick(gfx, x, base, mark, color)
      x = x + self.mark_tick
    end
    if marks.saved then
      icons.draw(gfx, "star", x + 11, base - mark / 2 + 1, self.mark_size, { color = color })
      x = x + self.mark_star
    end
    -- Stock's lock shows on the highlighted row only.
    if marks.secured and o.on then
      icons.draw(gfx, "lock", x + 11, base - mark / 2, self.mark_size, { color = color })
      x = x + self.mark_lock
    end
    icons.draw(gfx, "wifi", x + 11, base - mark / 2 - 2, self.wifi_size, { color = color, level = marks.level })
    x = x + self.mark_wifi
  end
  if icon == "back" then
    -- Cancel's disc in the text's colour, before it.
    icons.draw(gfx, "undo", x + self.back_size / 2, base - mark / 2, self.back_size, { color = color, bg = o.on and pill.color or "#000000" })
    x = x + self.back_gap
  elseif icon then
    -- The symbol keeps its colours, highlighted or not.
    icons.draw(gfx, icon, x + self.leaf_width / 2, base - mark / 2, self.leaf_width * 64 / 58.7, { p = 1, bg = o.on and pill.color or nil })
    x = x + self.leaf_width + self.leaf_gap
  end
  gfx:text(text, x, base + 1 / px, { font = self.font, size = self.size, color = color })
end

return ListRow
