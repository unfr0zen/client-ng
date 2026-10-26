-- Stock's Schedule screen: the week on a timeline under a fixed cursor,
-- 24 units an hour, its day named above and the cursor's time below. Each
-- setpoint is a bubble at its temperature's height; a triangle on each
-- midnight line and at the week's end carries the temperature then (none
-- on an empty schedule, which has no continuations to carry it, nor where
-- the engine leaves one out by a setpoint near it). The home dial sits
-- eight hours before Monday and as long after Sunday to leave by.
-- Pressing opens a menu over the dimmed timeline.
local theme = require("theme")
local icons = require("icons")
local anim = require("anim")

local M = theme.menu
local S = {}

local DAYS = { "MONDAY", "TUESDAY", "WEDNESDAY", "THURSDAY", "FRIDAY", "SATURDAY", "SUNDAY" }
local DAY, WEEK = 24 * 60, 7 * 24 * 60
local PX_PER_MIN = 24 / 60
-- The band's top and bottom lines, and the temperatures they stand for.
local TOP, BOTTOM = 106, 362
local T_TOP, T_BOTTOM = 35, 7
local TITLE = { font = theme.fonts.label, size = 35.7, color = "#ffffff", align = "center" }
local HOUR = { font = theme.fonts.label, size = 27.4, color = "#ffffff", align = "center" }
local BUBBLE = { font = theme.fonts.label, size = 27.4, color = "#ffffff", align = "center" }
local CHOICE = { font = theme.fonts.label, size = 35.7, align = "left" }
local LINE, CURSOR = "#505050", "#0bdbff"
-- The setpoints' bubbles and each day's starting triangle: orange heating,
-- blue cooling (a range's high end too).
local BUBBLE_FILL = { heat = "#f14f00", cool = "#0080ff" }
local TRIANGLE = { heat = "#f14f00", cool = "#006edb" }
local DIAL_AT, DIAL_R = -8 * 60, 145

local function celsius(t, unit)
  if unit == "F" then return (t - 32) * 5 / 9 end
  return t
end

-- The height for a temperature (shown unit): 9.14 units a degree, 7 °C
-- just over the bottom line.
local function height(t, unit)
  local c = anim.clamp(celsius(t, unit), T_BOTTOM, T_TOP)
  return 363.5 - (c - T_BOTTOM) * 9.14
end

-- A time as stock's GetFormattedTime 0x404 writes it: "07:30", or in
-- 12-hour time "7:30 AM" (libnlappcore puts a narrow no-break space
-- before AM, which the skin's fonts lack: a space stands in).
local function time_text(minute, twelve_hour)
  local hour, m = math.floor(minute / 60) % 24, minute % 60
  if twelve_hour then
    local half = tr(hour < 12 and "@string.time.AM|AM" or "@string.time.PM|PM")
    local h = hour % 12
    if h == 0 then h = 12 end
    return string.format("%d:%02d %s", h, m, half)
  end
  return string.format("%02d:%02d", hour, m)
end

-- An hour over the timeline as GetFormattedTime 0x2406 writes it: "04",
-- or in 12-hour time "4A" with the abbreviated AM or PM.
local function hour_text(hour, twelve_hour)
  if twelve_hour then
    local half = tr(hour < 12 and "@string.time.AM.abbr|A" or "@string.time.PM.abbr|P")
    local h = hour % 12
    if h == 0 then h = 12 end
    return h .. half
  end
  return string.format("%02d", hour)
end

local function leaf_shown(t, mode, unit)
  local c = celsius(t, unit)
  if mode == "heat" then return c <= 18.5 end
  if mode == "cool" then return c >= 26 end
  return false
end

-- A temperature in a bubble: whole degrees, a half as a raised 5. `ring`
-- colours its edge: the cursor's on the one under it, white while its
-- time is set. `cool` fills it blue.
local function bubble(gfx, x, y, t, unit, ring, cool)
  gfx:circle(x, y, 28.5, cool and BUBBLE_FILL.cool or BUBBLE_FILL.heat)
  if ring then gfx:ring(x, y, 27.25, 3.5, ring) end
  local whole, half = math.floor(t + 0.5), false
  if unit ~= "F" then
    local halves = math.floor(t * 2 + 0.5)
    whole, half = math.floor(halves / 2), halves % 2 == 1
  end
  local text = tostring(whole)
  local w = gfx:measure(text, BUBBLE)
  local base = y + 10
  gfx:text(text, x, base, BUBBLE)
  if half then
    gfx:text("5", x + w / 2 + 1, base - 9, { font = theme.fonts.label, size = 14, color = "#ffffff" })
  end
end

-- The home dial at either end of the week, in the home screen's colour,
-- with the target large and the clock under it.
local function dial(gfx, x, ui, background)
  local c = 239.5
  gfx:circle(x, c, DIAL_R, background)
  local t = ui.thermostat
  local whole, half
  if t.unit == "F" then
    whole = string.format("%d", math.floor(t.target + 0.5))
  else
    local halves = math.floor(t.target * 2 + 0.5)
    whole = string.format("%d", math.floor(halves / 2))
    half = halves % 2 == 1 and "5" or nil
  end
  local style = { font = theme.fonts.number, size = 101, color = "#ffffff", align = "center", spacing = 2 }
  local w = gfx:measure(whole, style)
  gfx:text(whole, x, c + 36.5, style)
  if half then
    gfx:text(half, x + w / 2 + 1, c + 1, { font = theme.fonts.label, size = 49, color = "#ffffff" })
  end
  local clock = ui.clock
  local hour = clock.hour
  if clock.twelve_hour then
    hour = hour % 12
    if hour == 0 then hour = 12 end
  end
  gfx:text(string.format(clock.twelve_hour and "%d:%02d" or "%02d:%02d", hour, clock.minute), x, 433,
           { font = theme.fonts.label, size = 31, color = M.title, align = "center" })
end

-- The timeline at `at` minutes, the day of minute `day` named over it
-- (none on a dial).
local function timeline(gfx, ui, sch, at, day)
  local t = ui.thermostat
  local twelve = ui.clock.twelve_hour
  -- The timeline moves by whole pixels, its lines staying sharp.
  local px = ui.face.scale or 1
  local origin = math.floor((240 - at * PX_PER_MIN) * px + 0.5) / px
  local function x_of(minute) return origin + minute * PX_PER_MIN end
  if day >= 0 then
    gfx:text(string.upper(tr(DAYS[math.floor(day / DAY) + 1])), 240, 66, TITLE)
  end
  -- The band runs from Monday 00:00 to the week's end.
  local left, right = math.max(0, x_of(0)), math.min(480, x_of(WEEK))
  gfx:line(left, TOP + 0.5, right, TOP + 0.5, 1, LINE)
  gfx:line(left, BOTTOM + 0.5, right, BOTTOM + 0.5, 1, LINE)
  local first = math.max(0, math.floor((at - 620) / 60))
  local last = math.min(WEEK / 60, math.ceil((at + 620) / 60))
  for h = first, last do
    local x = x_of(h * 60)
    if h % 4 == 0 and h < WEEK / 60 then
      gfx:text(hour_text(h % 24, twelve), x, 100, HOUR)
    end
    if h % 24 == 0 then
      if h < WEEK / 60 then gfx:line(x + 0.5, TOP, x + 0.5, BOTTOM + 1, 1, LINE) end
      local start = sch.starts[h / 24 + 1]
      if start ~= nil then
        local y = height(start, t.unit)
        gfx:polygon({ x - 10, y - 12, x + 14, y, x - 10, y + 12 }, t.mode == "cool" and TRIANGLE.cool or TRIANGLE.heat)
      end
    end
  end
  for _, p in ipairs(sch.points) do
    local x = x_of(p.minute)
    if x > -40 and x < 520 then
      local mark = not sch.editing and p.minute == sch.held and CURSOR or nil
      bubble(gfx, x, height(p.temp, t.unit), p.temp, t.unit, mark, t.mode == "cool")
      -- A range setpoint has its high end too.
      if p.high then bubble(gfx, x, height(p.high, t.unit), p.high, t.unit, mark, true) end
    end
  end
end

-- `background`: the home screen's colour, for the dial.
function S.draw(gfx, ui, background)
  local sch = ui.schedule
  local t = ui.thermostat
  gfx:clear(theme.colors.menu)
  -- The timeline where the wheel flung it, the time under it to the
  -- quarter.
  local at = sch.pending and sch.pending.minute or sch.cursor
  local pos = sch.pending and sch.pending.minute or sch.scroll
  timeline(gfx, ui, sch, pos, at)
  if sch.choices then
    if at >= 0 then gfx:text(time_text(at, ui.clock.twelve_hour), 240, 421, TITLE) end
    gfx:rect(0, 0, 480, 480, "#000000d9")
  elseif sch.pending then
    local y = height(sch.pending.temp, t.unit)
    gfx:rect(239, TOP, 2, BOTTOM - TOP + 1, "#ffffff")
    bubble(gfx, 240, y, sch.pending.temp, t.unit, (sch.editing == "time" or sch.editing == "high") and "#ffffff" or CURSOR, t.mode == "cool")
    if sch.pending.high then
      bubble(gfx, 240, height(sch.pending.high, t.unit), sch.pending.high, t.unit, sch.editing == "high" and CURSOR or "#ffffff", true)
    end
    local time = { font = TITLE.font, size = TITLE.size, align = "center", color = sch.editing == "time" and CURSOR or "#ffffff" }
    gfx:text(time_text(sch.pending.minute, ui.clock.twelve_hour), 240, 421, time)
    if leaf_shown(sch.pending.temp, t.mode, t.unit) then
      icons.draw(gfx, "leaf", 329, 405, 30, { p = 1 })
    end
  elseif at >= 0 then
    gfx:rect(239, TOP, 2, BOTTOM - TOP + 1, CURSOR)
    gfx:text(time_text(at, ui.clock.twelve_hour), 240, 421, TITLE)
  end
  for _, d in ipairs({ DIAL_AT, WEEK - DIAL_AT }) do
    if math.abs(d - pos) < 620 then
      dial(gfx, 240 + (d - pos) * PX_PER_MIN, ui, background)
    end
  end
  if sch.choices then
    for i, name in ipairs(sch.choices) do
      local text = string.upper(name)
      local base = 239 + (i - 1) * 48
      local w = gfx:measure(text, CHOICE)
      local on = i == sch.choice
      if on then gfx:rect(240 - w / 2 - 24, base - 35, w + 48, 48, M.pill, 24) end
      gfx:text(text, 240 - w / 2, base, { font = CHOICE.font, size = CHOICE.size, color = "#ffffff" })
    end
  end
  return false
end

return S
