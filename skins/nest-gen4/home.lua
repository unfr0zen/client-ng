-- The home screen as the 4th gen Nest Learning Thermostat's: black, with
-- a soft light behind the number in the colour of what the system is
-- doing (rose heating, blue cooling) while it runs; a faint ring open at
-- the bottom, temperatures rising clockwise along it, with the room's
-- white dot and number and the mode's symbol inside the target; the
-- target in large type, what the system is doing under it and the leaf
-- for a setpoint that saves energy.
-- From afar (Farsight), the number alone, larger, or the time.
--
-- It moves: the number's changed digits roll to new values (up as it
-- rises, down as it falls); turning the wheel brings up a scale of ticks
-- along the ring in place of the dots, lit from the room to the target,
-- which fades a moment after the wheel stops as the dots come back; the
-- marks glide to new temperatures, the light fades in and out and
-- crossfades between states, and coming home the ring draws in from its
-- ends as the rest fades up.
--
-- The light and the track are layers drawn once and painted in their
-- colours; the marks and the words are layers drawn again only when they
-- change; the scale and the number's digits are layers drawn once and
-- painted where they stand, so a still screen costs a few paints and
-- none once idle.
local anim = require("classic:anim")
local theme = require("theme")
local C, R, T, G, N, L, M = theme.colors, theme.ring, theme.ticks, theme.glow, theme.number, theme.label, theme.motion

local home = {}

local function polar(c, r, degrees)
  local a = math.rad(degrees)
  return c + r * math.sin(a), c - r * math.cos(a)
end

-- Paint only the ring's band from a cached layer, leaving its clear centre
-- and corners out of the GPU's work.
local clip_sets = {}
local function ring_clips(c, inner, outer)
  local key = table.concat({ c, inner, outer }, ":")
  if clip_sets[key] then return clip_sets[key] end
  local pieces = {}
  local reach = outer / math.cos(math.rad(15))
  for i = 0, 9 do
    local a0, a1 = -150 + i * 30, -120 + i * 30
    local x0, y0 = polar(c, inner, a0)
    local x1, y1 = polar(c, reach, a0)
    local x2, y2 = polar(c, reach, a1)
    local x3, y3 = polar(c, inner, a1)
    pieces[#pieces + 1] = { x0, y0, x1, y1, x2, y2, x3, y3 }
  end
  clip_sets[key] = pieces
  return pieces
end

local function celsius(t, value)
  if t.unit == "F" then return (value - 32) * 5 / 9 end
  return value
end

-- The Pokémon skin's eco rule: a setpoint that saves energy.
local function eco(t)
  if t.mode == "heat" then return celsius(t, t.target) <= 18.5 end
  if t.mode == "cool" then return celsius(t, t.target) >= 26 end
  return false
end

local function running(t)
  return t.mode ~= "off" and (t.hvac == "heating" or t.hvac == "cooling")
end

-- The system's colours: its marks' (the target's tick and symbol) and
-- the light's.
local function accent(t)
  if t.hvac == "cooling" then return C.cool, C.cool_glow end
  return C.heat, C.heat_glow
end

-- Whole degrees, and the half in Celsius as the tenth after them.
local function digits(value, unit)
  if unit == "F" then return string.format("%d", math.floor(value + 0.5)), nil end
  local halves = math.floor(value * 2 + 0.5)
  return string.format("%d", math.floor(halves / 2)), halves % 2 == 1 and ".5" or nil
end

local function short(value, unit)
  local whole, half = digits(value, unit)
  return whole .. (half or "") .. "°"
end

-- The ring's angle for a temperature: rising clockwise, as the wheel turns.
local function angle(t, value)
  local f = anim.clamp((value - t.min) / (t.max - t.min), 0, 1)
  return R.from + f * (R.to - R.from)
end

-- What the words under the number say, and their colour.
local function status(t)
  if t.range then
    local name = t.range == "low" and "heat" or "cool"
    return (t.range_editing and "Press to set " or "Press to change ") .. name, t.range == "low" and C.heat or C.cool
  end
  if t.mode == "off" then return "Inside " .. short(t.current, t.unit), C.secondary end
  if t.hvac == "heating" then return "Heating", C.heat end
  if t.hvac == "cooling" then return "Cooling", C.cool end
  if t.mode == "heat-cool" then return "Heat • Cool", C.secondary end
  return t.mode == "cool" and "Cool" or "Heat", C.secondary
end

-- Symbols, `s` units across, centred on (x, y).

-- Heat: three wavy lines rising.
local function heat_symbol(g, x, y, s, color)
  local segs = {}
  for i = -1, 1 do
    local px, py
    for j = 0, 12 do
      local f = j / 12
      local qx = x + i * s * 0.32 + math.sin(f * 2 * math.pi) * s * 0.09
      local qy = y + s * 0.5 - f * s
      if px then
        segs[#segs + 1] = px; segs[#segs + 1] = py; segs[#segs + 1] = qx; segs[#segs + 1] = qy
      end
      px, py = qx, qy
    end
  end
  g:lines(segs, s * 0.1, color)
end

-- Cool: a snowflake of six spokes, each with a pair of barbs.
local function cool_symbol(g, x, y, s, color)
  local segs, r = {}, s * 0.5
  for i = 0, 5 do
    local a = 60 * i
    local ex, ey = polar(0, r, a)
    segs[#segs + 1] = x; segs[#segs + 1] = y; segs[#segs + 1] = x + ex; segs[#segs + 1] = y + ey
    local mx, my = polar(0, r * 0.62, a)
    for _, side in ipairs({ -45, 45 }) do
      local bx, by = polar(0, r * 0.3, a + side)
      segs[#segs + 1] = x + mx; segs[#segs + 1] = y + my
      segs[#segs + 1] = x + mx + bx; segs[#segs + 1] = y + my + by
    end
  end
  g:lines(segs, s * 0.09, color)
end

-- The leaf: two arcs meeting at the tip and the stem, leaning right, with
-- its vein cut out.
local function leaf_symbol(g, x, y, s, color)
  local pts, n = {}, 16
  local half = s * 0.5
  for side = 0, 1 do
    for i = 0, n do
      local f = side == 0 and i / n or 1 - i / n
      local along = -half + f * s
      local across = math.sin(f * math.pi) * s * 0.32 * (side == 0 and 1 or -1)
      -- Turned 40° clockwise from upright.
      local a = math.rad(40)
      pts[#pts + 1] = x + across * math.cos(a) - along * math.sin(a)
      pts[#pts + 1] = y + across * math.sin(a) + along * math.cos(a)
    end
  end
  g:polygon(pts, color)
  local a = math.rad(40)
  local function axis(along) return x - along * math.sin(a), y + along * math.cos(a) end
  local x1, y1 = axis(half * 0.75)
  local x2, y2 = axis(-half * 0.45)
  g:line(x1, y1, x2, y2, s * 0.08, C.bg, true)
end

-- Sets a run of the number's characters; with the rolling number below.
local set_run

-- The target's number: whole degrees centred on x, their digits in cells
-- of one width, so they stay put as the half comes and goes and as their
-- neighbours change, and the half smaller on their baseline after them.
local function big_number(g, x, value, unit, base, size, color)
  local whole, half = digits(value, unit)
  local w = set_run(g, whole, nil, size, color, base)
  local x0 = x - w / 2
  set_run(g, whole, x0, size, color, base)
  if half then set_run(g, half, x0 + w + 1, size * N.half, color, base) end
end

-- Where a symbol or the room's number stands for a dot at angle `a`:
-- `depth` inside the ring, further in where the ring runs upright, the
-- number being wider than tall.
local function label_at(c, a, depth)
  return polar(c, R.radius - depth - 14 * math.abs(math.sin(math.rad(a))), a)
end

local function hex(v)
  return string.format("%02x", math.floor(255 * anim.clamp(v, 0, 1) + 0.5))
end

-- A tick at angle `a` from `inner` to `outer` units out, added to `segs`.
local function tick(segs, c, a, inner, outer)
  local x0, y0 = polar(c, inner, a)
  local x1, y1 = polar(c, outer, a)
  segs[#segs + 1] = x0; segs[#segs + 1] = y0; segs[#segs + 1] = x1; segs[#segs + 1] = y1
end

-- Quads over the ring's band from `inner` to `outer` units out between
-- angles a0 and a1, at most 30 degrees each, as paint_layer's clips.
local function wedge_clips(c, inner, outer, a0, a1)
  local pieces, n = {}, math.max(1, math.ceil((a1 - a0) / 30))
  local step = (a1 - a0) / n
  local reach = outer / math.cos(math.rad(step / 2))
  for i = 0, n - 1 do
    local b0, b1 = a0 + i * step, a0 + (i + 1) * step
    local x0, y0 = polar(c, inner, b0)
    local x1, y1 = polar(c, reach, b0)
    local x2, y2 = polar(c, reach, b1)
    local x3, y3 = polar(c, inner, b1)
    pieces[#pieces + 1] = { x0, y0, x1, y1, x2, y2, x3, y3 }
  end
  return pieces
end

-- The scale the wheel shows, at opacity `s`: a tick every T.step degrees
-- along the ring, fine and pale, and bolder and white between the room
-- and the target (the range in heat-cool); the room's and the targets'
-- longer bars, the room's white and the targets' in their colours. Each
-- kind of tick is one layer, painted through wedges whose edges fall
-- between ticks.
local function scale(gfx, c, t, a, s)
  local inner, outer = R.radius - T.length, R.radius
  local function ticks(name, step, width)
    gfx:layer(name, table.concat({ step, T.length, width }, " "), function(g)
      local segs = {}
      for i = 0, math.floor((R.to - R.from) / step + 0.001) do tick(segs, c, R.from + i * step, inner, outer) end
      g:lines(segs, width, "#ffffff")
    end)
  end
  ticks("ticks", T.step, T.width)
  ticks("lit_ticks", T.step, T.lit_width)
  local lo, hi
  if t.mode == "heat-cool" then
    lo, hi = math.min(a.room, a.low), math.max(a.room, a.high)
  else
    lo, hi = math.min(a.room, a.set), math.max(a.room, a.set)
  end
  -- The lit stretch from the tick nearest `lo` to the one nearest `hi`,
  -- its edges half a step beyond them.
  local function nearest(at) return R.from + math.floor((at - R.from) / T.step + 0.5) * T.step end
  local e0, e1 = nearest(lo) - T.step / 2, nearest(hi) + T.step / 2
  local first, last = R.from - T.step / 2, R.to + T.step / 2
  local dim = C.tick .. hex(s * T.dim)
  local function paint(name, color, from, to)
    from, to = math.max(from, first), math.min(to, last)
    if to <= from then return end
    for _, clip in ipairs(wedge_clips(c, inner - 2, outer + 2, from, to)) do
      gfx:paint_layer(name, color, clip)
    end
  end
  paint("ticks", dim, first, e0)
  paint("lit_ticks", C.text .. hex(s), e0, e1)
  paint("ticks", dim, e1, last)
  local room = {}
  tick(room, c, a.room, inner - T.bar, outer)
  gfx:lines(room, T.bar_width, C.room .. hex(s))
  local function target(at, color)
    local segs = {}
    tick(segs, c, at, inner - T.bar, outer)
    gfx:lines(segs, T.bar_width + 3, C.bg .. hex(s))
    gfx:lines(segs, T.bar_width, color .. hex(s))
  end
  if t.mode == "heat-cool" then
    target(a.low, (t.range == "high") and C.dim or C.heat)
    target(a.high, (t.range == "low") and C.dim or C.cool)
  else
    target(a.set, t.mode == "cool" and C.cool or C.heat)
  end
end

-- Where the room's number and each symbol stand, `clear` units further
-- in than at rest: the number inside the room's dot, a symbol inside its
-- target's.
local function mark_spots(c, a, clear)
  local lx, ly = label_at(c, a.room, 24 + clear)
  local spots = { label = { lx, ly } }
  for _, k in ipairs({ "set", "low", "high" }) do
    local sx, sy = label_at(c, a[k], 26 + clear)
    spots[k] = { sx, sy }
  end
  return spots
end

-- Whether the symbol at `spot` would overlap the room's number at `label`
-- (wider than tall), with `margin` units to spare.
local function overlaps(spot, label, margin)
  return math.abs(spot[1] - label[1]) < 30 + margin and math.abs(spot[2] - label[2]) < 18 + margin
end

-- The room's number over the mode's symbol inside the target (the
-- range's ends in heat-cool), at the spots `p` from mark_spots, the
-- symbols at opacity `shown`.
local function marks(g, c, t, p, shown)
  local o = hex(shown)
  if t.mode == "heat-cool" then
    heat_symbol(g, p.low[1], p.low[2], 16, ((t.range == "high") and C.dim or C.heat) .. o)
    cool_symbol(g, p.high[1], p.high[2], 16, ((t.range == "low") and C.dim or C.cool) .. o)
  else
    local draw_symbol = t.mode == "cool" and cool_symbol or heat_symbol
    draw_symbol(g, p.set[1], p.set[2], 16, (t.mode == "cool" and C.cool or C.heat) .. o)
  end
  local label = { font = "figtree_medium", size = 17, color = C.room, align = "center" }
  g:text(short(t.current, t.unit), p.label[1], p.label[2] + 6, label)
end

-- The rolling number: each digit is drawn once into a layer of its own
-- and painted through a window round the number, so a changed digit can
-- slide out of it as the new one slides in.

local measures = {}
local function measure(g, text, size)
  local key = size .. " " .. text
  if not measures[key] then
    measures[key] = g:measure(text, { font = "figtree", size = size })
  end
  return measures[key]
end

-- The layer holding `ch` at `size`, white on clear, its baseline at
-- 0.85 of `size` down and `pad` units in: its name, pad, width and height.
local function glyph(g, ch, size)
  local name = "glyph " .. ch .. " " .. size
  local pad = math.ceil(size * 0.06)
  local w, h = math.ceil(measure(g, ch, size) + 2 * pad), math.ceil(size)
  g:layer(name, "1", function(l)
    l:text(ch, pad, size * 0.85, { font = "figtree", size = size, color = "#ffffff", align = "left" })
  end, w, h)
  return name, pad, w, h
end

-- `ch` at `size` with its left at x and its baseline at `base` moved down
-- by dy, in `color`, showing only between screen heights top and bottom.
local function paint_glyph(g, ch, x, base, size, color, dy, top, bottom)
  local name, pad, w, h = glyph(g, ch, size)
  local lx, ly = math.floor(x - pad + 0.5), math.floor(base - size * 0.85 + dy + 0.5)
  local y0, y1 = math.max(0, top - ly), math.min(h, bottom - ly)
  if y1 <= y0 then return end
  g:save()
  g:translate(lx, ly)
  g:paint_layer(name, color, { 0, y0, w, y0, w, y1, 0, y1 })
  g:restore()
end

-- The width every digit takes at `size`: N.cell of the widest's, each
-- digit centred in it, so a digit stands where it did whatever its
-- neighbours.
local function cell(g, size)
  local widest = 0
  for d = 0, 9 do widest = math.max(widest, measure(g, tostring(d), size)) end
  return widest * N.cell
end

-- How wide `text` is set with its digits in cells.
local function run_width(g, text, size)
  local w = 0
  for i = 1, #text do
    local ch = text:sub(i, i)
    w = w + (ch:match("%d") and cell(g, size) or measure(g, ch, size))
  end
  return w
end

-- The characters of `text` and their lefts from `x`, digits centred in
-- their cells.
local function lefts(g, text, size, x)
  local out, at = {}, x
  for i = 1, #text do
    local ch = text:sub(i, i)
    local w = measure(g, ch, size)
    if ch:match("%d") then
      local c = cell(g, size)
      out[i] = at + (c - w) / 2
      at = at + c
    else
      out[i] = at
      at = at + w
    end
  end
  return out
end

-- Sets `text` from `x` with its digits in cells, or with no `x` only
-- measures it: its width.
function set_run(g, text, x, size, color, base)
  if x then
    local style = { font = "figtree", size = size, color = color, align = "left" }
    for i, left in ipairs(lefts(g, text, size, x)) do g:text(text:sub(i, i), left, base, style) end
  end
  return run_width(g, text, size)
end

-- A run of characters rolling from `old` (left at ox) to `new` (left at
-- nx) by q (0 to 1), aligned from their last characters: those the same
-- glide across, those changed roll, up out of the window as the new ones
-- come up into it when `dir` is 1, down when -1.
local function roll_run(g, old, new, ox, nx, base, size, color, q, dir)
  local top, bottom = base - size * 0.8, base + size * 0.07
  local travel = bottom - top
  local ol, nl = lefts(g, old, size, ox), lefts(g, new, size, nx)
  local n = math.max(#old, #new)
  for i = 1, n do
    local oi, ni = #old - n + i, #new - n + i
    local oc = oi >= 1 and old:sub(oi, oi) or nil
    local nc = ni >= 1 and new:sub(ni, ni) or nil
    if oc == nc then
      paint_glyph(g, nc, ol[oi] + (nl[ni] - ol[oi]) * q, base, size, color, 0, top, bottom)
    else
      if oc and q < 1 then paint_glyph(g, oc, ol[oi], base, size, color, -dir * travel * q, top, bottom) end
      if nc then paint_glyph(g, nc, nl[ni], base, size, color, dir * travel * (1 - q), top, bottom) end
    end
  end
end

-- What each number shows and rolls from, by where it stands (set, low,
-- high). A new value rolls in at the wheel's pace: the roll takes as long
-- as the wait since the value before (M.roll after a pause, no quicker
-- than M.roll_min). A roll under way when the value changes again is
-- hurried to its end at that pace, never cut, and the next goes straight
-- to the latest value: the number keeps up with the wheel and stops with
-- it.
local rolls = {}

local function rolled(slot, value, unit, now)
  local function shown(v)
    local whole, half = digits(v, unit)
    return { whole, half or "" }
  end
  local r = rolls[slot]
  if not r then
    r = { value = value, target = value, from = shown(value), to = shown(value), at = -100, length = M.roll,
          pace = M.roll, changed = -100, dir = 1 }
    rolls[slot] = r
  end
  if value ~= r.target then
    r.pace = anim.clamp((now - r.changed) * 0.9, M.roll_min, M.roll)
    r.changed, r.target = now, value
    local elapsed = now - r.at
    if elapsed < r.length then
      r.length = math.min(r.length, elapsed + r.pace * (1 - elapsed / r.length))
    end
  end
  if now - r.at >= r.length and r.value ~= r.target then
    r.dir = r.target > r.value and 1 or -1
    r.from, r.to, r.value = r.to, shown(r.target), r.target
    r.at, r.length = now, r.pace
  end
  local p = anim.clamp((now - r.at) / r.length, 0, 1)
  return r, anim.ease_in_out(p), p < 1 or r.value ~= r.target
end

-- The target's number rolling to `value`: whole degrees centred on x,
-- the half smaller on their baseline after them. Whether it moves.
local function rolling_number(g, slot, x, value, unit, base, size, color, now)
  local r, q, moving = rolled(slot, value, unit, now)
  local small = size * N.half
  local ow, nw = run_width(g, r.from[1], size), run_width(g, r.to[1], size)
  local ox, nx = x - ow / 2, x - nw / 2
  roll_run(g, r.from[1], r.to[1], ox, nx, base, size, color, q, r.dir)
  roll_run(g, r.from[2], r.to[2], ox + ow + 1, nx + nw + 1, base, small, color, q, r.dir)
  return moving
end

-- The middle's number, or the range, or Off, at opacity `e`, the numbers
-- rolling to new values. Whether they move.
local function number_text(g, c, t, e, now)
  local o = hex(e)
  if t.mode == "off" then
    g:text(tr("Off"), c, N.base - 6, { font = "figtree", size = N.size * 0.82, color = C.text .. o, align = "center" })
    return false
  elseif t.mode == "heat-cool" then
    local function tone(side) return ((t.range and t.range ~= side) and C.dim or C.text) .. o end
    local low = rolling_number(g, "low", c - 96, t.low, t.unit, N.range_base, N.range_size, tone("low"), now)
    local high = rolling_number(g, "high", c + 96, t.high, t.unit, N.range_base, N.range_size, tone("high"), now)
    g:circle(c, N.range_base - N.range_size * 0.34, 4, C.secondary .. o)
    return low or high
  end
  return rolling_number(g, "set", c, t.target, t.unit, N.base, N.size, C.text .. o, now)
end

-- The words under the number and the leaf.
local function words(g, c, t)
  local text, color = status(t)
  g:text(tr(text), c, L.y, { font = "figtree_medium", size = L.size, color = color, align = "center" })
  if eco(t) then leaf_symbol(g, c, theme.leaf.y, theme.leaf.size, C.leaf) end
end

local function glow_layer(gfx, c)
  local key = table.concat({ G.radius, G.steps }, " ")
  gfx:layer("glow", key, function(g)
    g:clear(C.bg)
    g:radial_glow(c, c + 6, G.radius, G.steps)
  end)
end

-- The light's colour and strength (1 while the system runs, else 0),
-- crossfading from where they stood when they last changed.
local glow = { color = nil, strength = 0, from_color = nil, from_strength = 0, at = -100 }

local function shade(color, amount)
  local function channel(i)
    return math.max(0, math.min(255, math.floor(tonumber(color:sub(i, i + 1), 16) * amount + 0.5)))
  end
  return string.format("#%02x%02x%02x", channel(2), channel(4), channel(6))
end

local function glow_now(now)
  local p = anim.ease_in_out((now - glow.at) / M.glow)
  local color = glow.color
  if glow.strength == 0 then
    color = glow.from_color
  elseif glow.from_strength > 0 then
    color = anim.mix_color(glow.from_color, glow.color, p)
  end
  return color, anim.mix(glow.from_strength, glow.strength, p), p < 1
end

-- The light behind the number while the system runs; `strength` scales
-- it. Whether it moves.
local function paint_glow(gfx, ui, c, strength)
  local t = ui.thermostat
  local on = running(t)
  local _, light = accent(t)
  local want, level = on and light or (glow.color or light), on and 1 or 0
  if glow.color == nil then
    glow.color, glow.from_color, glow.strength, glow.from_strength = want, want, level, level
  elseif want ~= glow.color or level ~= glow.strength then
    glow.from_color, glow.from_strength = glow_now(ui.now)
    glow.color, glow.strength, glow.at = want, level, ui.now
  end
  local color, shown, fading = glow_now(ui.now)
  if shown > 0.001 then
    glow_layer(gfx, c)
    -- Only the light's square: the layer is black beyond it, as the
    -- screen is cleared.
    local r = G.radius + 4
    gfx:paint_layer("glow", shade(color, shown * strength), { c - r, c + 6 - r, c + r, c + 6 - r, c + r, c + 6 + r, c - r, c + 6 + r })
  end
  return fading
end

-- The track, drawn in from both ends by `e` (0 to 1), at opacity `o`.
local function track(gfx, c, t, e, o)
  local color = (t.mode == "off" and C.track_off or C.track) .. hex(o)
  if e >= 1 then
    gfx:layer("track", "1", function(g) g:arc(c, c, R.radius, R.from, R.to, R.width, "#ffffff") end)
    for _, clip in ipairs(ring_clips(c, R.radius - 8, R.radius + 8)) do
      gfx:paint_layer("track", color, clip)
    end
    return
  end
  local half = (R.to - R.from) / 2 * e
  gfx:arc(c, c, R.radius, R.from, R.from + half, R.width, color)
  gfx:arc(c, c, R.radius, R.to - half, R.to, R.width, color)
end

-- How much of the scale shows: all of it while the wheel turns and for
-- M.hold seconds after, then fading out over M.fade. Whether it moves.
local function scale_shown(ui)
  local turned = ui.changed.turned
  local since = ui.now - turned
  if turned < 0 or since >= M.hold + M.fade then return 0, false end
  if since < M.hold then return 1, true end
  return 1 - anim.ease_in_out((since - M.hold) / M.fade), true
end

-- The angles drawn, gliding to where the thermostat puts them.
local follow = {
  set = anim.follower(M.dot, 0.05), low = anim.follower(M.dot, 0.05),
  high = anim.follower(M.dot, 0.05), room = anim.follower(M.room, 0.05),
}

local function angles(t, now)
  local a, moving = {}, false
  for k, value in pairs({ set = t.target, low = t.low, high = t.high, room = t.current }) do
    local m
    a[k], m = follow[k](angle(t, value), now)
    moving = moving or m
  end
  return a, moving
end

-- The marks' spots, in by T.clear while the scale shows (by `s`) and
-- gliding there, and the symbols faded to T.under and level with the
-- room's number while one is under it, back once it is a few units clear.
local glide = { clear = anim.follower(M.dot, 0.05), label = anim.follower(M.dot, 0.01) }
local covered = false

local function spots(c, t, a, s, now)
  local clear, moving = glide.clear(T.clear * s, now)
  local p = mark_spots(c, a, clear)
  local near = false
  for _, k in ipairs(t.mode == "heat-cool" and { "low", "high" } or { "set" }) do
    near = near or overlaps(p[k], p.label, covered and 6 or 0)
  end
  covered = near
  local shown, m = glide.label(covered and T.under or 1, now)
  -- Nearing the number, a symbol comes level with its digits' middle:
  -- all the way within T.level units across or under it, none from twice
  -- that.
  local covering = (1 - shown) / (1 - T.under)
  for _, k in ipairs({ "set", "low", "high" }) do
    local dx, dy = math.abs(p[k][1] - p.label[1]), math.abs(p[k][2] - p.label[2])
    local near = dy < 30 and anim.clamp(2 - dx / T.level, 0, 1) or 0
    local f = math.max(covering, near * near * (3 - 2 * near))
    p[k][2] = p[k][2] + (p.label[2] - p[k][2]) * f
  end
  return p, shown, moving or m
end

-- The number at opacity `e`, rolling to new values. Whether it moves.
local function paint_number(gfx, ui, c, t, e)
  return number_text(gfx, c, t, e, ui.now)
end

local function face(gfx, ui, c)
  local t = ui.thermostat
  -- Coming home, the ring draws in from its ends and the rest fades up.
  local e = ui.screen == "home" and anim.ease_out((ui.now - ui.changed.screen - M.intro_from) / M.intro) or 1
  local animating = e < 1
  animating = paint_glow(gfx, ui, c, e) or animating
  if t.mode ~= "off" then
    local a, moving = angles(t, ui.now)
    local s, showing = scale_shown(ui)
    if s < 1 then track(gfx, c, t, e, 1 - s) end
    if s > 0 then scale(gfx, c, t, a, s * e) end
    -- The room's and the target's dots at rest; the scale's ticks stand
    -- for them while it shows.
    if s < 1 then
      local o = hex((1 - s) * e)
      local function at_rest(at, color)
        local x, y = polar(c, R.radius, at)
        gfx:circle(x, y, R.room_dot, color .. o)
      end
      if t.mode == "heat-cool" then
        at_rest(a.low, (t.range == "high") and C.dim or C.heat)
        at_rest(a.high, (t.range == "low") and C.dim or C.cool)
      else
        at_rest(a.set, t.mode == "cool" and C.cool or C.heat)
      end
      at_rest(a.room, C.room)
    end
    local p, shown, gliding = spots(c, t, a, s, ui.now)
    moving = moving or gliding
    if moving or s > 0 or e < 1 then
      gfx:save()
      gfx:alpha(e)
      marks(gfx, c, t, p, shown)
      gfx:restore()
    else
      local key = table.concat({ t.mode, t.unit, t.current, t.range or "",
                                 string.format("%.1f %.1f %.1f %.1f %.1f %.1f %.1f %.1f %.2f", p.label[1], p.label[2],
                                               p.set[1], p.set[2], p.low[1], p.low[2], p.high[1], p.high[2], shown) }, "|")
      gfx:layer("marks", key, function(g) marks(g, c, t, p, shown) end)
      for _, clip in ipairs(ring_clips(c, R.radius - 95, R.radius + 20)) do
        gfx:paint_layer("marks", "#ffffff", clip)
      end
    end
    animating = animating or moving or showing
  else
    track(gfx, c, t, e, 1)
  end
  animating = paint_number(gfx, ui, c, t, e) or animating
  local wkey = table.concat({ status(t), tostring(eco(t)) }, "|")
  gfx:layer("words", wkey, function(g)
    g:translate(-90, -325)
    words(g, c, t)
  end, 300, 135)
  gfx:save()
  gfx:translate(90, 325)
  gfx:paint_layer("words", "#ffffff" .. hex(e))
  gfx:restore()
  return animating
end

function home.background()
  return C.bg
end

-- Draws the home screen; `look` shrinks (`scale`), darkens (`light`) or
-- narrows (`squash`) it, for the menus' transitions.
function home.draw(gfx, ui, look)
  look = look or {}
  local scale, light, squash = look.scale or 1, look.light or 1, look.squash or 1
  local c = ui.face.size / 2
  gfx:clear(C.bg)
  gfx:save()
  if scale < 1 or squash < 1 then
    gfx:translate(c, c)
    gfx:scale(scale * squash, scale)
    gfx:translate(-c, -c)
  end
  local animating = face(gfx, ui, c)
  gfx:restore()
  if light < 1 then
    gfx:circle(c, c, c + 10, string.format("#000000%02x", math.floor((1 - light) * 255 + 0.5)))
  end
  return animating
end

local function two(n) return string.format("%02d", n) end

-- The time as Farsight's clock: the hour over the minutes, the room's
-- temperature and humidity beside them.
local function clock_face(g, c, t, clock)
  local hour = clock.hour
  if clock.twelve_hour then hour = (hour + 11) % 12 + 1 end
  local style = { font = "figtree_light", size = 150, color = C.text, align = "right" }
  g:text(two(hour), c + 28, c - 8, style)
  g:text(two(clock.minute), c + 28, c + 128, style)
  local side = { font = "figtree_medium", size = 22, color = C.secondary }
  g:text(short(t.current, t.unit), c + 50, c - 22, side)
  if t.humidity then g:text(string.format("%d%%", math.floor(t.humidity + 0.5)), c + 50, c + 14, side) end
end

-- Farsight from afar: the target (or the room's temperature) alone and
-- larger over the light, Off, or the time.
function home.farsight(gfx, ui)
  local c = ui.face.size / 2
  local t = ui.thermostat
  gfx:clear(C.bg)
  if t.farsight == "digital_clock" or t.farsight == "analog_clock" then
    local clock = ui.clock
    local key = table.concat({ clock.hour, clock.minute, tostring(clock.twelve_hour), t.current, t.unit, tostring(t.humidity) }, "|")
    gfx:layer("far", key, function(g) clock_face(g, c, t, clock) end)
    gfx:paint_layer("far", "#ffffff")
    return false
  end
  local animating = paint_glow(gfx, ui, c, 1.15)
  local key = table.concat({ t.farsight, t.mode, t.target, t.current, t.unit, t.low, t.high }, "|")
  gfx:layer("far", key, function(g)
    local size = N.size * theme.far
    if t.farsight == "current_temp" then
      big_number(g, c, t.current, t.unit, c + size * 0.36, size, C.text)
    elseif t.mode == "off" then
      g:text(tr("Off"), c, c + size * 0.3, { font = "figtree", size = size * 0.82, color = C.text, align = "center" })
    elseif t.mode == "heat-cool" then
      local s = N.range_size * theme.far
      big_number(g, c - 92, t.low, t.unit, c + s * 0.36, s, C.text)
      big_number(g, c + 92, t.high, t.unit, c + s * 0.36, s, C.text)
      g:circle(c, c, 5, C.secondary)
    else
      big_number(g, c, t.target, t.unit, c + size * 0.36, size, C.text)
    end
  end)
  gfx:paint_layer("far", "#ffffff")
  return animating
end

return home
