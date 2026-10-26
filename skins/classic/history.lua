-- Stock's History (nlEnergyView, nlEnergyHistoryLayer): a row of day
-- cards with the live thermostat at each end, oldest on the left; each
-- card its date, its leaf, and a bar of heat and of cool run time, full at
-- 12 hours. A day's detail page scrolls: its totals, the day's runs on a
-- 24-hour line, the reason and its message, DONE. Positions and colours
-- are stock's (0x135130, 0x133604, 0x130884), matched on nest-qemu.
local icons = require("icons")
local anim = require("anim")
local theme = require("theme")

local M = {}
local F = theme.fonts
local C = 240
local HEAT = { bg = "#3c1400", fill = "#f14f00" }
local COOL = { bg = "#002040", fill = "#0080ff" }
local GREY = "#505050"
-- The reasons' symbols in the unit font (stock's 0x130c90: manual,
-- weather, Seasonal Savings, Cool to Dry), and each one's top in ems.
local GLYPHS = {
  [0] = { text = "\u{E007}", top = 0.710 },
  [1] = { text = "\u{E091}", top = 0.592 },
  [4] = { text = "\u{E053}", top = 0.710 },
  [5] = { text = "\u{E047}", top = 0.710 },
}
-- A card's neighbours sit two thirds of the screen away; the cards lie
-- where the wheel flung them (`ui.history.pos`), and the bars grow from
-- the last card's over BARS once another comes to the middle.
local SLOT = 320
local BARS = 0.3
-- Opening, the view scrolls from the thermostat at the right end to
-- yesterday over OPEN seconds (stock's 0x133e90), its bars from nothing.
local OPEN = 1.0
-- The detail page's scroll offset at each wheel step.
local SCROLL = { 0, 174, 434 }
local FADE = 0.3

local TITLE = { font = F.strong, size = 31, color = "#ffffff", align = "center" }
local LABEL = { font = F.strong, size = 31, color = "#ffffff" }
local LINE = { font = F.label, size = 24, color = "#9b9b9b", align = "center" }
local BODY = { font = F.strong, size = 30, color = "#ffffff", align = "center" }

-- The title, with the leaf before it when the day earned one.
local function title(gfx, day, y)
  local w = gfx:measure(day.title, TITLE)
  local x = C + (day.leaf and 20 or 0)
  if day.leaf then
    icons.draw(gfx, "leaf", x - w / 2 - 22, y - 11, 26, { color = "#16ff02" })
  end
  gfx:text(day.title, x, y, TITLE)
end

-- One bar: the background pill, the fill from the left (74 px at least),
-- the waves glyph and the time, right-aligned.
local function bar(gfx, y, value, text, colors, mode)
  local x0, w = 34, 412
  gfx:rect(x0, y - 32, w, 64, colors.bg, 32)
  local fill = math.min(w, math.max(74, value / 43200 * w + 74))
  gfx:rect(x0, y - 32, fill, 64, colors.fill, 32)
  icons.draw(gfx, "mode", 70, y, 30, { mode = mode, color = "#ffffff" })
  local tw = gfx:measure(text, LABEL)
  gfx:text(text, 426 - tw, y + 11, LABEL)
end

-- A card at horizontal offset `dx`; `grown` (0 to 1) brings the bars from
-- `from`'s values to the day's.
local function card(gfx, ui, day, dx, grown, from)
  local h = ui.history
  gfx:save()
  gfx:translate(dx, 0)
  title(gfx, day, 88)
  local y = 172
  if day.missing then
    gfx:text("Info Missing", C, 124, { font = F.strong, size = 28, color = "#ffffff", align = "center" })
  end
  local heat = from and anim.mix(from.heat, day.heat, grown) or day.heat
  local cool = from and anim.mix(from.cool, day.cool, grown) or day.cool
  if h.usage == -1 then
    gfx:rect(34, y - 32 + 18, 412, 64, GREY, 32)
  end
  if h.usage == 0 or h.usage == 2 then
    bar(gfx, y + 18, heat, day.heat_text, HEAT, "heat")
  end
  if h.usage == 1 or h.usage == 2 then
    bar(gfx, h.usage == 2 and y + 118 or y + 18, cool, day.cool_text, COOL, "cool")
  end
  gfx:restore()
end

-- The live thermostat at an end: a disc in the home screen's colour with
-- the target, an arrow toward the days and DONE.
local function puck(gfx, ui, home, dx, left)
  local t = ui.thermostat
  gfx:save()
  gfx:translate(dx, 0)
  gfx:circle(C, C, 144, home.background(ui))
  gfx:ring(C, C, 141, 6, "#5a5a5a")
  local target = t.mode == "heat-cool" and t.low or t.target
  local whole = math.floor(target + 0.001)
  gfx:text(tostring(whole), C, C + 36, { font = F.number, size = 100, color = "#ffffff", align = "center" })
  if target - whole >= 0.25 then
    local w = gfx:measure(tostring(whole), { font = F.number, size = 100 })
    gfx:text("5", C + w / 2 + 4, C - 8, { font = F.number, size = 40, color = "#ffffff" })
  end
  gfx:text(left and ">" or "<", left and 420 or 60, 252, { font = F.label, size = 36, color = "#ffffff", align = "center" })
  gfx:text("DONE", C, 432, { font = F.strong, size = 27, color = "#ffffff", align = "center" })
  gfx:restore()
end

-- A run row's rounded end cut again: black outside the half disc on
-- (`cx`, `y`), 32 across each way, out to the row's end at `edge`; `dir`
-- -1 for the left end, 1 for the right.
local function corners(gfx, edge, cx, y, dir)
  local pts = { edge, y - 32, cx, y - 32 }
  for i = 0, 16 do
    local a = math.rad(270 + dir * 180 * i / 16)
    pts[#pts + 1] = cx + 32 * math.cos(a)
    pts[#pts + 1] = y + 32 * math.sin(a)
  end
  pts[#pts + 1] = cx
  pts[#pts + 1] = y + 32
  pts[#pts + 1] = edge
  pts[#pts + 1] = y + 32
  gfx:polygon(pts, "#000000")
end

-- The detail page, scrolled by `offset` pixels.
local function detail(gfx, ui, day, d, offset)
  local h = ui.history
  gfx:save()
  gfx:translate(0, -offset)
  title(gfx, day, 88)
  local y = 130
  local function total(text, mode, color)
    local w = gfx:measure(text, { font = F.strong, size = 32 })
    icons.draw(gfx, "mode", C - w / 2 - 6, y - 11, 28, { mode = mode, color = color })
    gfx:text(text, C + 18, y, { font = F.strong, size = 32, color = color, align = "center" })
    y = y + 32
  end
  if h.usage == 0 or h.usage == 2 then total(d.heat_text, "heat", HEAT.fill) end
  if h.usage == 1 or h.usage == 2 then total(d.cool_text, "cool", COOL.fill) end
  -- The day's runs, a row per kind, midnight to midnight: square-ended,
  -- whole pixels, cut off at the row's end; where one reaches into a
  -- rounded end, that end is cut round again in the page's black
  -- (stock's 0x130884).
  local rows = {}
  if h.usage == -1 then rows = { { colors = { bg = GREY, fill = GREY } } } end
  if h.usage == 0 or h.usage == 2 then rows[#rows + 1] = { colors = HEAT, heat = true } end
  if h.usage == 1 or h.usage == 2 then rows[#rows + 1] = { colors = COOL, heat = false } end
  local ry = 240
  local scale = 412 / 86400
  for _, r in ipairs(rows) do
    gfx:rect(34, ry - 32, 412, 64, r.colors.bg, 32)
    local left, right = false, false
    for _, c in ipairs(d.cycles) do
      if c.heat == r.heat then
        local x0 = math.floor(c.start * scale)
        local x1 = math.floor(x0 + c.length * scale)
        if x1 < x0 or x1 > 412 then x1 = 412 end
        gfx:rect(34 + x0, ry - 32, x1 - x0, 64, r.colors.fill, 0)
        left = left or x0 < 32
        right = right or x1 > 380
      end
    end
    if left then corners(gfx, 34, 66, ry, -1) end
    if right then corners(gfx, 446, 414, ry, 1) end
    ry = ry + 100
  end
  -- The hours under the rows as GetFormattedTime 0x2406 writes them.
  local axis = ry - 100 + 68
  local twelve = ui.clock.twelve_hour
  local A, P = tr("@string.time.AM.abbr|A"), tr("@string.time.PM.abbr|P")
  local labels = twelve and { "12" .. A, "6" .. A, "12" .. P, "6" .. P, "12" .. A } or { "00", "06", "12", "18", "00" }
  for i, label in ipairs(labels) do
    gfx:text(label, 46 + (i - 1) * 97, axis, LINE)
  end
  -- The reason's symbol on a grey disc, the message, DONE. Eco and Away
  -- are both the leaf (stock's 0x130c90); the others are the unit font's
  -- symbols at 63, centred on their advance, the baseline half their top
  -- under the disc's middle, or drawn ones where the font lacks them.
  local my = axis + 94
  if d.reason then
    gfx:circle(C, my, 40, GREY)
    local glyph = GLYPHS[d.reason]
    local style = { font = F.strong, size = 63, color = "#ffffff" }
    if glyph and gfx:has_glyphs(glyph.text, style) then
      local w = math.floor(gfx:measure(glyph.text, style))
      gfx:text(glyph.text, C - math.floor(w / 2), my + math.floor(math.ceil(glyph.top * 63) / 2), style)
    else
      local symbol = ({ [0] = "tools", [1] = "sun", [2] = "leaf", [3] = "leaf", [4] = "autoschedule", [5] = "dry" })[d.reason]
      if symbol then icons.draw(gfx, symbol, C, my, 44, { color = "#ffffff" }) end
    end
    my = my + 88
  end
  local lines = require("alert").words(gfx, d.message, BODY, 400)
  for i, l in ipairs(lines) do
    gfx:text(l, C, my + (i - 1) * 38, BODY)
  end
  local by = my + #lines * 38 + 34
  local done = { font = F.strong, size = 32, color = "#ffffff", align = "center" }
  local w = gfx:measure("DONE", done)
  gfx:rect(C - w / 2 - 24, by - 36, w + 48, 48, theme.menu.pill, 24)
  gfx:text("DONE", C, by, done)
  gfx:restore()
end

-- The card in the middle before the last one, for its bars to grow
-- from, and when the opening scroll began.
local shown_at, moved_at, last_at = nil, -100, nil
local opened, opened_at, span = nil, -100, BARS
local opened_detail, detail_at = false, -100

function M.draw(gfx, ui, home)
  local h = ui.history
  local n = #h.days
  gfx:clear("#000000")
  if opened ~= ui.changed.screen and not h.detail and not opened_detail then
    opened, opened_at = ui.changed.screen, ui.now
    shown_at, last_at, moved_at, span = n + 1, h.at, ui.now, OPEN
  elseif h.at ~= last_at then
    shown_at, last_at, moved_at, span = last_at or h.at, h.at, ui.now, BARS
  end
  local t = anim.clamp((ui.now - opened_at) / OPEN, 0, 1)
  local pos = t < 1 and anim.mix(n + 1, h.pos, anim.ease_in_out(t)) or h.pos
  local function day(i) return h.days[i] end
  if h.detail then
    if not opened_detail then opened_detail, detail_at = true, ui.now end
    local d = h.detail
    local offset = SCROLL[d.scroll + 1] or SCROLL[#SCROLL]
    gfx:save()
    gfx:alpha(anim.clamp((ui.now - detail_at) / FADE, 0, 1))
    detail(gfx, ui, day(h.at), d, offset)
    gfx:restore()
    return ui.now - detail_at < FADE
  end
  opened_detail = false
  local grown = anim.clamp((ui.now - moved_at - (span - BARS)) / BARS, 0, 1)
  for slot = math.floor(pos) - 1, math.ceil(pos) + 1 do
    local dx = (slot - pos) * SLOT
    if math.abs(dx) < SLOT then
      -- Alpha multiplies within its save, so each slot has its own.
      gfx:save()
      gfx:alpha(1 - math.abs(dx) / SLOT)
      if slot == 0 then
        puck(gfx, ui, home, dx, true)
      elseif slot == n + 1 then
        puck(gfx, ui, home, dx, false)
      elseif day(slot) then
        local from = slot == h.at and (day(shown_at) or { heat = 0, cool = 0 }) or nil
        card(gfx, ui, day(slot), dx, grown, from)
      end
      gfx:restore()
    end
  end
  return t < 1 or grown < 1
end

return M
