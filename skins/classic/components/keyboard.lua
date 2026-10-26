-- Stock's keyboard (nlEncoderWheelLayer): the page's characters round the
-- ring from about eight o'clock, each given an arc in proportion to its
-- width and `pad` units (`arc_letters` degrees for letters, `arc` for the
-- rest); SHIFT, SPACE and DELETE down the right; the page key, DONE and
-- CANCEL along the bottom; the text in a field in the middle. The
-- selected key sits on a blue disc and names itself: SHIFT, SPACE and
-- DELETE above the field, the page key, DONE and CANCEL in its place.
-- Geometry from stock's draw (0x11e62c), matched on nest-qemu.
local Component = require("components.component")
local icons = require("icons")
local theme = require("theme")

local F = theme.fonts
local Keyboard = Component:extend({
  centre = 240, radius = 216.5, start = -129.81, pad = 13.12, arc_letters = 221.35, arc = 231,
  blue = "#00a3d9", disc = 19, key_disc = 22,
  char = { font = F.label, size = 36, color = "#ffffff" }, char_dy = 12,
  names = { shift = "SHIFT", space = "SPACE", delete = "DELETE" },
  big = { page = "KEYBOARD", done = "DONE", cancel = "CANCEL" },
  -- The fixed keys, design units from the centre: their symbols' middles.
  -- Selected, their discs stay inside the glass.
  at = { shift = { 210, 34.5 }, space = { 199, 79 }, delete = { 168, 118.5 },
         page = { 78.5, 153.5 }, done = { 4.75, 203 }, cancel = { -84.25, 174.25 } },
  key_color = "#ffffff", dim = "#666666", page_size = 28, page_step = 15.5, page_gap = 4,
  -- The field: `width` by `height` in the blue, filled `fill`, its text
  -- `text_x` from the middle (at most `fits` shown), the placeholder in
  -- `placeholder`, the cursor a pixel wide; a caption `caption_dy` over it.
  field = { width = 324, height = 50, radius = 12, edge = 2, fill = "#004157", font = F.label, size = 32, color = "#ffffff",
            text_x = -147, placeholder_x = -143, placeholder = "#666666", text_dy = 9, cursor_x = -148, cursor_dy = -16, cursor = 32,
            fits = 290 },
  caption = { font = F.label, size = 28, color = "#8c8c8c", dy = -34 }, big_dy = 10,
  -- Opening (stock's draw with its layer's progress p, 0x12e62c): over
  -- `open` seconds it fades in, the characters turn `turn` degrees on to
  -- their places from `rise` further out, and the fixed keys come in from
  -- `spread` (481/445) times as far out.
  open = 0.4, turn = 90, rise = 36, spread = 481 / 445,
})

-- A fixed key's symbol at (x, y): SHIFT and DELETE are outlines, the shape
-- filled and its inside cut out in `bg` (stock's, 23 by 25.5 and 33 by 26,
-- 2.6 and 2.3 units thick); SPACE a bracket 3 thick.
function Keyboard:draw_key(gfx, name, x, y, color, bg)
  if name == "shift" then
    x = x + 0.3
    gfx:polygon({ x, y - 12, x + 11.6, y, x + 11.6, y + 3, x + 5.9, y + 3, x + 5.9, y + 13.5, x - 5.9, y + 13.5, x - 5.9, y + 3,
                  x - 11.6, y + 3, x - 11.6, y }, color)
    gfx:polygon({ x, y - 8.2, x + 8.3, y + 0.4, x + 3.3, y + 0.4, x + 3.3, y + 10.9, x - 3.3, y + 10.9, x - 3.3, y + 0.4, x - 8.3, y + 0.4 }, bg)
  elseif name == "space" then
    gfx:polygon({ x - 8.9, y - 4.2, x - 5.7, y - 4.2, x - 5.7, y + 1.95, x + 6.6, y + 1.95, x + 6.6, y - 4.2, x + 9.75, y - 4.2,
                  x + 9.75, y + 5, x - 8.9, y + 5 }, color)
  elseif name == "delete" then
    y = y + 0.7
    gfx:polygon({ x - 16.5, y, x - 5.7, y - 12.7, x + 17, y - 12.7, x + 17, y + 12.7, x - 5.7, y + 12.7 }, color)
    gfx:polygon({ x - 13.4, y, x - 4.6, y - 10.25, x + 14.6, y - 10.25, x + 14.6, y + 10.3, x - 4.6, y + 10.3 }, bg)
    gfx:lines({ x + 0.5, y - 5, x + 9.5, y + 5 }, 3.6, color)
    gfx:lines({ x + 0.5, y + 5, x + 9.5, y - 5 }, 3.6, color)
  elseif name == "done" then
    gfx:line(x - 21, y + 4, x - 1.5, y + 16, 9, color, true)
    gfx:line(x - 1.5, y + 16, x + 21.5, y - 17.7, 9, color, true)
  elseif name == "cancel" then
    icons.draw(gfx, "back", x, y, 56, { color = color })
  end
end

-- The keyboard `ui.keyboard` (keys, at, page, page_label, text,
-- placeholder), `t` seconds after it opened (open when not given).
function Keyboard:draw(gfx, ui, t)
  local C = self.centre
  local function polar(angle, r)
    local a = math.rad(angle)
    return C + r * math.sin(a), C - r * math.cos(a)
  end
  local k = ui.keyboard
  local keys = k.keys
  local selected = keys[k.at]
  local NAMES, BIG, CHAR = self.names, self.big, self.char
  local p = t and math.max(0, math.min(1, t / self.open)) or 1
  gfx:clear("#000000")
  gfx:save()
  gfx:alpha(p)
  local spread = self.spread - (self.spread - 1) * p
  -- The characters, each in its share of the arc.
  local chars = {}
  for i, key in ipairs(keys) do
    if not BIG[key] and not NAMES[key] then chars[#chars + 1] = i end
  end
  local advances, widths, total = {}, {}, 0
  for n, i in ipairs(chars) do
    -- Its advance and `pad` more (measure's width alone: math.max would
    -- take its height too).
    advances[n] = (gfx:measure(keys[i], CHAR))
    widths[n] = advances[n] + self.pad
    total = total + widths[n]
  end
  local arc = (k.page == 0 or k.page == 4) and self.arc_letters or self.arc
  local angle = self.start + self.turn * (1 - p)
  for n, i in ipairs(chars) do
    local span = widths[n] * arc / total
    local mid = angle + span / 2
    local x, y = polar(mid, self.radius + self.rise * (1 - p))
    if i == k.at then gfx:circle(x, y, self.disc, self.blue) end
    gfx:save()
    gfx:translate(x, y)
    gfx:rotate(mid)
    gfx:text(keys[i], -advances[n] / 2, self.char_dy, CHAR)
    gfx:restore()
    angle = angle + span
  end
  -- The fixed keys, the selected one on its disc.
  for _, name in ipairs({ "shift", "space", "delete", "page", "done", "cancel" }) do
    local present = false
    for _, key in ipairs(keys) do if key == name then present = true end end
    if present then
      local at = self.at[name]
      local x, y = C + at[1] * spread, C + at[2] * spread
      local on = selected == name
      local big = name == "page" or name == "done" or name == "cancel"
      if on and not big then gfx:circle(x, y, self.key_disc, self.blue) end
      local color = (on or not big) and self.key_color or self.dim
      if name == "page" then
        local style = { font = F.label, size = self.page_size }
        local label = k.page_label:match("^(%S+)")
        local tx = x - gfx:measure(label, style) / 2
        for c in label:gmatch("[%z\1-\127\194-\244][\128-\191]*") do
          gfx:text(c, tx, y + self.page_step, { font = F.label, size = self.page_size, color = color })
          tx = tx + gfx:measure(c, style) + self.page_gap
          y = y + self.page_step
        end
      else
        self:draw_key(gfx, name, x, y, on and big and self.key_color or (big and self.dim or self.key_color), on and self.blue or "#000000")
      end
    end
  end
  -- The field, or what the big key does in its place.
  local f, cap = self.field, self.caption
  local fy = C
  local caption = { font = cap.font, size = cap.size, color = cap.color, align = "center" }
  if BIG[selected] then
    local over = selected == "page" and k.page_label or (selected == "done" and k.text or nil)
    if over and over ~= "" then gfx:text(over, C, fy + cap.dy, caption) end
    gfx:text(BIG[selected], C, fy + self.big_dy, { font = F.strong, size = 32, color = "#ffffff", align = "center" })
  else
    if NAMES[selected] then gfx:text(NAMES[selected], C, fy + cap.dy, caption) end
    gfx:rect(C - f.width / 2, fy - f.height / 2, f.width, f.height, self.blue, f.radius)
    gfx:rect(C - f.width / 2 + f.edge, fy - f.height / 2 + f.edge, f.width - 2 * f.edge, f.height - 2 * f.edge, f.fill, f.radius - f.edge)
    local field = { font = f.font, size = f.size, color = f.color }
    local px = ui.face.scale or 1
    if k.text == "" then
      gfx:rect(C + f.cursor_x, fy + f.cursor_dy, 1 / px, f.cursor, f.color)
      gfx:text(k.placeholder, C + f.placeholder_x, fy + f.text_dy, { font = f.font, size = f.size, color = f.placeholder })
    else
      local shown = k.text
      while gfx:measure(shown, field) > f.fits and #shown > 1 do shown = shown:sub(2) end
      if shown ~= k.text then shown = "..." .. shown end
      gfx:text(shown, C + f.text_x, fy + f.text_dy, field)
      gfx:rect(math.floor((C + f.cursor_x + gfx:measure(shown, field)) * px) / px, fy + f.cursor_dy, 1 / px, f.cursor, f.color)
    end
  end
  gfx:restore()
  return p < 1
end

return Keyboard
