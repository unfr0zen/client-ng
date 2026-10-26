-- The home screen, after the stock dial: a ring of ticks over the whole
-- panel's colour, the target's marker and the room temperature's tick, the
-- ticks between them lit, the target in large type and what the system is
-- doing above it. The ticks are drawn once into a layer and painted in each
-- state's colour, which keeps a frame cheap.
local theme = require("theme")
local anim = require("anim")
local icons = require("icons")
local alert = require("alert")
local D, P, F = theme.dial, theme.states, theme.fonts

local home = {}

-- Akkurat's capital and figure height, in ems.
local CAP = 0.729

local function polar(c, r, degrees)
  local a = math.rad(degrees)
  return c + r * math.sin(a), c - r * math.cos(a)
end

-- `color` (#rrggbb) at opacity `a`.
local function faded(color, a)
  return string.format("%s%02x", color, math.floor(anim.clamp(a, 0, 1) * 255 + 0.5))
end

local function celsius(t, unit)
  if unit == "F" then return (t - 32) * 5 / 9 end
  return t
end

-- The ring's angle for a temperature in Celsius: on the scale, midway
-- between the two ticks nearest (a mark there covers a tick beside it).
local function angle(c)
  local a = D.scale.at20 + (anim.clamp(c, D.min, D.max) - 20) * D.scale.per
  a = D.from + (math.floor((a - D.from) / D.step) + 0.5) * D.step
  return anim.clamp(a, D.from, D.to)
end

-- The room's bar: a tick before its place.
local function room_angle(c)
  return angle(c) - D.step
end

-- Convex pieces covering the band between radii r0 and r1 from angle a0 to
-- a1, as clip polygons for painting the ticks' layer: narrow, so that
-- little but the band is drawn (each pixel drawn costs the GPU).
local function band(c, a0, a1, r0, r1)
  local pieces = {}
  local n = math.max(1, math.ceil((a1 - a0) / 15))
  for i = 0, n - 1 do
    local b0, b1 = a0 + (a1 - a0) * i / n, a0 + (a1 - a0) * (i + 1) / n
    local ro = r1 / math.cos(math.rad((b1 - b0) / 2))
    local x1, y1 = polar(c, r0, b0)
    local x2, y2 = polar(c, ro, b0)
    local x3, y3 = polar(c, ro, b1)
    local x4, y4 = polar(c, r0, b1)
    pieces[#pieces + 1] = { x1, y1, x2, y2, x3, y3, x4, y4 }
  end
  return pieces
end

-- A bar along the radius at an angle, widening outwards like stock's
-- marker and current tick.
local function bar(gfx, c, degrees, r0, r1, color, shape)
  local a = math.rad(degrees)
  local ux, uy = math.sin(a), -math.cos(a)
  local px, py = -uy, ux
  shape = shape or D.marker
  local w0, w1 = (shape.k * r0 + shape.b) / 2, (shape.k * r1 + shape.b) / 2
  gfx:polygon({
    c + ux * r0 - px * w0, c + uy * r0 - py * w0,
    c + ux * r1 - px * w1, c + uy * r1 - py * w1,
    c + ux * r1 + px * w1, c + uy * r1 + py * w1,
    c + ux * r0 + px * w0, c + uy * r0 + py * w0,
  }, color)
end

-- The ticks in white, in one layer: painted in a colour, they take it.
local function layers(gfx, c)
  local key = table.concat({ D.from, D.to, D.step, D.inner, D.outer, D.tick_width, D.rim, D.rim_width }, " ")
  gfx:layer("dial", key, function(g)
    local segs = {}
    for a = D.from, D.to + 0.001, D.step do
      local x1, y1 = polar(c, D.inner, a)
      local x2, y2 = polar(c, D.outer, a)
      segs[#segs + 1] = x1; segs[#segs + 1] = y1; segs[#segs + 1] = x2; segs[#segs + 1] = y2
    end
    g:lines(segs, D.tick_width, "#ffffff")
  end)
end

-- The first and last ticks whose angles lie strictly between a0 and a1.
local function between(a0, a1)
  return math.floor((a0 - D.from) / D.step + 1e-6) + 1, math.ceil((a1 - D.from) / D.step - 1e-6) - 1
end

-- Paints the ticks whose angles lie strictly between a0 and a1.
local function paint_ticks(gfx, c, a0, a1, color)
  local first, last = between(a0, a1)
  if last < first then return end
  local b0 = D.from + (first - 0.5) * D.step
  local b1 = D.from + (last + 0.5) * D.step
  for _, piece in ipairs(band(c, b0, b1, D.inner - 2, D.outer + 2)) do
    gfx:paint_layer("dial", color, piece)
  end
end

-- All the ticks drawn as lines, those strictly between a0 and a1 in `lit`:
-- sharp under a zoom, where the layer's pixels would show enlarged.
local function tick_lines(gfx, c, a0, a1, color, lit)
  local first, last = between(a0, a1)
  local segs = { [color] = {}, [lit] = {} }
  local i = 0
  for a = D.from, D.to + 0.001, D.step do
    local s = segs[(i >= first and i <= last) and lit or color]
    local x1, y1 = polar(c, D.inner, a)
    local x2, y2 = polar(c, D.outer, a)
    s[#s + 1] = x1; s[#s + 1] = y1; s[#s + 1] = x2; s[#s + 1] = y2
    i = i + 1
  end
  for tone, s in pairs(segs) do
    if #s > 0 then gfx:lines(s, D.tick_width, tone) end
  end
end

-- `gfx` setting its text `k` times larger under a 1 / k scale: drawn under
-- a k-times zoom, letters are then made at the size they show rather than
-- enlarged from their own, and symbols drawn as shapes, not from a layer.
local function sharp(gfx, k)
  local function sized(style)
    local s = {}
    for key, v in pairs(style or {}) do s[key] = v end
    s.size = (s.size or 24) * k
    if s.spacing then s.spacing = s.spacing * k end
    if s.outline then s.outline_width = (s.outline_width or 2) * k end
    return s
  end
  local proxy = { sharp = true }
  function proxy.text(_, text, x, y, style)
    gfx:save()
    gfx:translate(x, y)
    gfx:scale(1 / k)
    local w = gfx:text(text, 0, 0, sized(style))
    gfx:restore()
    return w / k
  end
  function proxy.measure(_, text, style)
    local w, h = gfx:measure(text, sized(style))
    return w / k, h / k
  end
  return setmetatable(proxy, { __index = function(t, name)
    local f = function(_, ...) return gfx[name](gfx, ...) end
    rawset(t, name, f)
    return f
  end })
end

-- A temperature as its whole part and, in Celsius, a half shown as a small 5.
local function parts(t, unit)
  if unit == "F" then
    return string.format("%d", math.floor(t + 0.5)), nil
  end
  local halves = math.floor(t * 2 + 0.5)
  return string.format("%d", math.floor(halves / 2)), (halves % 2 == 1) and "5" or nil
end

-- The marker's sweep: from where it was drawn to where it is going.
local sweep = { from = nil, to = nil, start = 0, length = 0 }

local function marker_at(target, now, turned)
  if sweep.to == nil then
    sweep.from, sweep.to = target, target
  elseif target ~= sweep.to then
    local p = anim.ease_in_out((now - sweep.start) / math.max(sweep.length, 1e-3))
    sweep.from = anim.mix(sweep.from, sweep.to, p)
    sweep.to, sweep.start = target, now
    -- The wheel moves the marker at once, as stock's does; a change from
    -- elsewhere sweeps to it.
    sweep.length = turned and 0 or D.sweep
    if turned then sweep.from = target end
  end
  local p = anim.ease_in_out((now - sweep.start) / math.max(sweep.length, 1e-3))
  return anim.mix(sweep.from, sweep.to, p), p < 1
end

local function palette(t, target)
  local state = P[t.hvac] or P.idle
  -- How far the room has to go, so past the target it stays near.
  local away = target - celsius(t.current, t.unit)
  if t.hvac == "cooling" then away = -away end
  local p = anim.clamp(away / state.span, 0, 1)
  return {
    bg = anim.mix_color(state.bg.near, state.bg.far, p),
    tick = anim.mix_color(state.tick.near, state.tick.far, p),
    lit = state.lit, current = state.current, status = state.status,
  }
end

-- The room temperature beside its tick, over the ticks, outlined in the
-- background so the ticks stop short of its letters.
-- The label of `value` centred on (x, y), `k` times its size; by `lone`
-- (0 to 1, 0 when nil) its whole part alone is, the half beyond it.
local function label_at(gfx, t, value, colors, x, y, k, lone)
  local whole, half = parts(value, t.unit)
  local big = { font = F.label, size = 34 * k, color = colors.current, outline = colors.bg, outline_width = 3 * k }
  local small = { font = F.label, size = 17 * k, color = colors.current, outline = colors.bg, outline_width = 3 * k }
  local w = gfx:measure(whole, big)
  local hw = half and gfx:measure(half, small) or 0
  local x0 = x - (w + hw * (1 - (lone or 0))) / 2
  local base = y + big.size * CAP / 2
  gfx:text(whole, x0, base, big)
  if half then
    gfx:text(half, x0 + w, base - big.size * CAP + small.size * CAP, small)
  end
end

local function current_label(gfx, c, t, a_current, side, colors, value)
  local lx, ly = polar(c, D.label_radius, a_current + side * D.label_angle)
  label_at(gfx, t, value or t.current, colors, lx, ly, 1)
end

-- How long until the target, as stock words it (nltemplayer): minutes
-- rounded up to the next five.
local function time_to_target(m)
  if m <= 8 then return "UNDER 10 MIN" end
  if m <= 54 then return string.format("IN %d MIN", (math.floor(m / 5) + 1) * 5) end
  if m <= 59 then return "IN 1 HR" end
  if m <= 84 then return string.format("IN 1 HR %d MIN", (math.floor((m - 60) / 5) + 1) * 5) end
  if m <= 105 then return "IN 1½ HR" end
  return "IN 2+ HR"
end

-- The line over the number, the first that applies in stock's order
-- (0x1d3644 tries 0x1d0f8c's cases): Safety Temperatures running, the
-- far-end hint, Emergency heat as the mode, aux heat running, Early-On,
-- Cool to Dry, Airwave, a second or third stage, the time to the target,
-- alternate heat, then what the system does or is set to.
local function status_text(t)
  if t.safety then return "SAFETY" end
  if t.press_to == "cool" then return "PRESS TO COOL" end
  if t.press_to == "heat" then return "PRESS TO HEAT" end
  if t.emergency then return "EMERGENCY HEAT" end
  if t.aux then return "AUX. HEAT" end
  if t.preconditioning == "heat" then return "PREHEATING" end
  if t.preconditioning == "cool" then return "PRECOOLING" end
  if t.drying then return "COOL TO DRY" end
  if t.airwave then return "AIRWAVE" end
  if t.hvac == "heating" and t.stage > 1 then return "HEAT STAGE " .. t.stage end
  if t.alt == 2 then return "ALT. HEAT STAGE 2" end
  if t.hvac == "cooling" and t.stage > 1 then return "COOL STAGE " .. t.stage end
  if t.time_to_target and t.stage == 1 and (t.hvac == "heating" or t.hvac == "cooling") then
    return time_to_target(t.time_to_target)
  end
  if t.alt == 1 then return "ALT. HEAT" end
  if t.hvac == "heating" then return "HEATING" end
  if t.hvac == "cooling" then return "COOLING" end
  if t.mode == "cool" then return "COOL SET TO" end
  return "HEAT SET TO"
end

-- The target in large type, a half as a raised 5, centred on `x`, at
-- opacity `a` and `k` times its size (1 when nil).
local function number(gfx, x, value, unit, base, a, k)
  local whole, half = parts(value, unit)
  local white = faded("#ffffff", a or 1)
  k = k or 1
  -- Stock 6.5 centres it exactly, not on a whole pixel.
  local style = { font = F.number, size = 169 * k, color = white, align = "center", spacing = 0.15 * k, snap = false }
  local w = gfx:measure(whole, style)
  gfx:text(whole, x, base, style)
  if half then
    gfx:text(half, x + w / 2 + k, base - 58 * k, { font = F.label, size = 81 * k, color = white })
  end
end

-- The leaf: a heat target at or under its threshold, a cool target at or
-- over (the device bucket's `leaf_threshold_heat` and `_cool`), as the
-- engine decides it.
local function leaf_shown(t)
  return t.leaf_on
end

-- The setpoint the system works towards, in Celsius.
local function setpoint(t)
  local target = celsius(t.target, t.unit)
  if t.mode == "heat-cool" then
    local cur, low, high = celsius(t.current, t.unit), celsius(t.low, t.unit), celsius(t.high, t.unit)
    target = (cur > high) and high or low
  end
  return target
end

-- The panel's colour on the home screen, for what is drawn over it.
function home.background(ui)
  return palette(ui.thermostat, setpoint(ui.thermostat)).bg
end

-- What follows from the thermostat's state alone, worked out again only
-- when it changes: a frame then costs little more than painting.
local memo = {}

local function state_of(t)
  local key = table.concat({ t.mode, t.hvac, t.stage, tostring(t.leaf_on), tostring(t.sunblock), tostring(t.time_to_target), t.target, t.current, t.unit, t.low, t.high,
                             tostring(t.can_heat), tostring(t.can_cool), t.range or "",
                             tostring(t.range_editing), tostring(t.eco), t.starts_in or "", t.delayed_for or "",
                             tostring(t.safety), tostring(t.system_issue) }, "|")
  if memo.key ~= key then
    local target = setpoint(t)
    local cur = celsius(t.current, t.unit)
    memo = { key = key, target = target, a_current = room_angle(cur), colors = palette(t, target) }
  end
  return memo
end

-- Stock's heat-cool: both ends of the range in one line, the heating one
-- first, over their markers. At rest a label names each end, or what the
-- system is doing while it runs; once the wheel picks an end the other
-- dims and the press is prompted for, and while it changes only its label
-- shows.
local DIM = "59"

-- The numbers and the dot between them show at opacity `core`, the
-- markers and words at `words` (1 when nil); with `far`, the numbers alone
-- (Farsight's heat-cool view, stock's temperature layer without the dial).
local function range(g, c, t, colors, far, core, words)
  local picked, editing = t.range, t.range_editing
  local function tone(color, side)
    if picked and picked ~= side then return color .. DIM end
    return color
  end
  g:save()
  g:alpha(words or 1)
  if not far then
    bar(g, c, angle(celsius(t.low, t.unit)), D.marker_inner, D.outer, tone(P.target, "low"))
    bar(g, c, angle(celsius(t.high, t.unit)), D.marker_inner, D.outer, tone(P.target, "high"))
  end
  g:restore()

  -- The picked end is set larger and bolder.
  local ends = {}
  for i, side in ipairs({ "low", "high" }) do
    local whole, half = parts(t[side], t.unit)
    local size = picked == side and 108 or 99
    local big = { font = picked == side and F.strong or F.label, size = size }
    local small = { font = big.font, size = size * 0.49 }
    local w = g:measure(whole, big)
    ends[i] = { side = side, whole = whole, half = half, w = w, big = big, small = small,
                width = w + (half and g:measure(half, small) + 2 or 0) }
  end
  -- The dot between them only fits when the first has no half.
  local gap = ends[1].half and 14 or 42
  local x = c - (ends[1].width + gap + ends[2].width) / 2
  ends[1].x, ends[2].x = x, x + ends[1].width + gap
  g:save()
  g:alpha(core or 1)
  for _, e in ipairs(ends) do
    local color = tone("#ffffff", e.side)
    g:text(e.whole, e.x, 275, { font = e.big.font, size = e.big.size, color = color })
    if e.half then
      g:text(e.half, e.x + e.w + 2, 275 - (e.big.size - e.small.size) * CAP,
             { font = e.small.font, size = e.small.size, color = color })
    end
  end
  if not ends[1].half then g:circle(x + ends[1].width + gap / 2, 240, 5, "#ffffff") end
  g:restore()
  if far then return end

  g:save()
  g:alpha(words or 1)
  local label = { font = F.label, size = 25, color = colors.status, align = "center" }
  local strong = { font = F.strong, size = 28, color = colors.status, align = "center" }
  local names = { low = "HEAT", high = "COOL" }
  if editing then
    local e = picked == "low" and ends[1] or ends[2]
    g:text(names[picked] .. " TO", e.x + 4, 180, { font = F.strong, size = 28, color = colors.status })
  elseif picked or t.hvac == "idle" then
    for _, e in ipairs(ends) do
      local style = picked == e.side and strong or label
      style.color = tone(colors.status, e.side)
      g:text(names[e.side], e.x + e.w / 2, picked == e.side and 180 or 185, style)
    end
  else
    g:text(status_text(t), c, 186, label)
  end
  if picked then
    g:text(editing and "PRESS TO SET" or "PRESS TO CHANGE", c, 315,
           { font = F.label, size = 25.5, color = colors.status, align = "center" })
  end
  g:restore()
end

-- Off: the system the wires give over a large OFF, at opacity `a`.
local function off_text(g, c, t, a)
  local system = (t.can_heat and t.can_cool) and "HEAT • COOL" or (t.can_cool and "COOL" or "HEAT")
  g:text(system, c, 196, { font = F.label, size = 27, color = faded(P.idle.status, a or 1), align = "center" })
  g:text("OFF", c, 266, { font = F.strong, size = 71, color = faded("#ffffff", a or 1), align = "center" })
end

-- Eco: the word in the middle, at opacity `a`.
local function eco_text(g, c, a)
  g:text("ECO", c, 265, { font = F.strong, size = 71, color = faded("#ffffff", a or 1), align = "center" })
end

-- Everything on the home screen but the ticks and the target's marker:
-- the current tick and its label, the type and the leaf. The room's
-- tick and label are `room`'s to draw, with the dial; `status` false
-- leaves the words over the number out.
-- The room's label goes when it reads as the target (stock on nest-qemu:
-- 18.87 under a 19 target, no label).
local function label_hidden(t)
  local room_w, room_h = parts(t.current, t.unit)
  local target_w, target_h = parts(t.target, t.unit)
  return room_w == target_w and room_h == target_h and not t.eco and t.mode ~= "off" and t.mode ~= "heat-cool"
end

local function room(g, c, t, m, side, bare)
  local colors = m.colors
  bar(g, c, m.a_current, D.inner, D.outer, colors.current, D.room)
  if not bare and not label_hidden(t) then current_label(g, c, t, m.a_current, side, colors) end
end

-- The middle of the face at opacity `figure` (1 when nil): the number,
-- Off's or Eco's words, or the range's numbers with its markers and
-- words, the words at `fade`. The wheel changes it at every detent, so
-- it is drawn as it is, not from a layer.
local function middle(g, c, t, m, figure, fade)
  if t.eco then
    eco_text(g, c, figure)
  elseif t.mode == "off" then
    off_text(g, c, t, figure)
  elseif t.mode == "heat-cool" then
    range(g, c, t, m.colors, false, figure, fade)
  else
    number(g, c, t.target, t.unit, 300, figure)
  end
end

-- The words over and under the middle and the symbol under it, at
-- opacity `fade`, and Eco's label; `issue` is the opacity of the
-- system-issue symbol (`fade` when nil). `live` draws the symbol as
-- shapes, not from its layer.
local function words(g, c, t, m, side, fan, status, fade, live, issue)
  local colors = m.colors
  local tone = faded(colors.status, fade or 1)
  if t.eco then
    -- Stock's Eco: the Eco temperature labelled by its marker as the
    -- room's is.
    g:save()
    g:alpha(fade or 1)
    current_label(g, c, t, angle(celsius(t.target, t.unit)), -side, colors, t.target)
    g:restore()
  elseif t.mode ~= "off" and t.mode ~= "heat-cool" then
    if status and not t.delayed_for then
      g:text(status_text(t), c, 162, { font = F.label, size = 27, color = tone, align = "center" })
    end
    -- Below the number, first a drained battery (stock's 0x1d2fcc case
    -- 0x11), then a lockout counting down (case 5): a compressor lockout
    -- to the start, a minimum cycle, holding the system off or on, to
    -- its end (0x1ce2fc).
    local left = t.starts_in or t.delayed_for
    if t.system_issue then
      g:text("SYSTEM ISSUE", c, 340, { font = F.label, size = 27, color = tone, align = "center" })
    elseif left then
      local words = t.lockout == "compressor" and "STARTS IN" or "DELAYED FOR"
      local text = string.format("%s %d:%02d", words, math.floor(left / 60), left % 60)
      g:text(text, c, 340, { font = F.label, size = 27, color = tone, align = "center", snap = false })
    end
  end
  -- Airwave's symbol, or a running fan timer's, takes the leaf's place,
  -- as stock's (Airwave's matched on nest-qemu); Sunblock's yellow sun
  -- comes before them all, then a drained battery's U+E037, as stock's
  -- 0x1d0624 draws them.
  g:save()
  g:alpha((not t.sunblock and t.system_issue and issue) or fade or 1)
  if t.sunblock then
    icons.draw(g, "sun", c, 404, 48, { color = "#ffcc04", live = live })
  elseif t.system_issue then
    icons.draw(g, "issue", c, 404, 48, { color = "#ffbb00", live = live })
  elseif t.airwave then
    icons.draw(g, "airwave", c, 406, 46, { color = "#ffffff", live = live })
  elseif fan then
    icons.draw(g, "fan", c, 404, 48, { color = "#ffffff", live = live })
  elseif leaf_shown(t) then
    icons.draw(g, "leaf", c, 404, 48, { p = 1, bg = colors.bg, live = true })
  end
  g:restore()
end

-- `fade` is the opacity of the words over and under the middle and of the
-- symbol under it, `figure` that of the middle (1 when nil), `issue` that
-- of the system-issue symbol (`fade` when nil). `live` draws the symbol
-- as shapes, not from its layer.
local function details(g, c, t, m, side, fan, status, fade, figure, live, issue)
  middle(g, c, t, m, figure, fade)
  words(g, c, t, m, side, fan, status, fade, live, issue)
end

-- Draws the home screen; `look.scale` (below 1) shrinks the dial inside a
-- black face and `look.light` (below 1) fades it all to black, as stock's
-- dial does when the menus open; `look.squash` narrows it, on black, as it
-- turns face-on after an edit. Back from Settings, as stock's: `look.dial`
-- fades the ticks and the room's tick and label alone, `look.status`
-- false leaves the words over the number out, and `look.ring` is how
-- strongly the Done tile's ring, grown with the dial, still shows. From
-- Farsight: `look.zoom` grows it all about the centre, then `look.shift`
-- ({ x, y }) moves it, `look.fade` and `look.figure` are the opacities of
-- the words and symbol about the middle and of the middle (the number,
-- Off's or Eco's words, the range's numbers), `look.issue` that of the
-- system-issue symbol (`look.fade` when nil), and `look.label` false
-- leaves the room's label out.
function home.draw(gfx, ui, look)
  look = look or {}
  local scale, light, squash = look.scale or 1, look.light or 1, look.squash or 1
  local zoom, shift = look.zoom or 1, look.shift or { 0, 0 }
  local dial, status = look.dial or 1, look.status ~= false
  local c = ui.face.size / 2
  local t = ui.thermostat
  local off = t.mode == "off"
  local m = state_of(t)
  local colors = m.colors
  local shown, animating = marker_at(m.target + (t.nudge or 0), ui.now, ui.changed.turned >= ui.changed.target)

  if squash < 1 then
    gfx:clear("#000000")
  else
    gfx:clear(light < 1 and anim.mix_color("#000000", colors.bg, light) or colors.bg)
  end
  layers(gfx, c)
  gfx:save()
  if scale < 1 or squash < 1 then
    -- The shrunk dial's face on black: nothing to draw on black.
    local black = colors.bg == "#000000"
    if squash >= 1 and not black then gfx:circle(c, c, D.rim + D.rim_width, "#000000") end
    gfx:translate(c, c)
    gfx:scale(scale * squash, scale)
    gfx:translate(-c, -c)
    if not black then gfx:circle(c, c, c, colors.bg) end
  end
  -- Zoomed, the ticks, words and symbols are drawn at the size they show
  -- (`sharp`), the rim stays the caller's to draw, at its size.
  local zoomed = zoom ~= 1
  if zoomed or shift[1] ~= 0 or shift[2] ~= 0 then
    gfx:translate(c + shift[1], c + shift[2])
    gfx:scale(zoom)
    gfx:translate(-c, -c)
  end
  local g = zoomed and sharp(gfx, zoom) or gfx

  local a_marker = angle(shown)
  local side = (not off and a_marker >= m.a_current) and -1 or 1
  local a0, a1 = math.min(m.a_current, a_marker), math.max(m.a_current, a_marker)
  gfx:save()
  gfx:alpha(dial)
  if off then
    if not zoomed then gfx:ring(c, c, D.rim, D.rim_width, "#000000") end
  else
    if zoomed then
      tick_lines(gfx, c, a0, a1, colors.tick, colors.lit)
    else
      for _, piece in ipairs(band(c, D.from - D.step, D.to + D.step, D.inner - 2, D.outer + 2)) do
        gfx:paint_layer("dial", colors.tick, piece)
      end
      gfx:ring(c, c, D.rim, D.rim_width, "#000000")
      paint_ticks(gfx, c, a0, a1, colors.lit)
    end
    if t.mode ~= "heat-cool" or t.eco then
      -- Eco's marker spans the ticks only, as the room's does.
      bar(gfx, c, a_marker, t.eco and D.inner or D.marker_inner, D.outer, P.target)
    end
  end
  -- The room's bar and label are a polygon and a few runs of text: drawn
  -- as they are, as a layer of them would be drawn again at every detent
  -- (the label's outline is the panel's colour, which deepens with the
  -- target).
  room(g, c, t, m, side, look.label == false)
  gfx:restore()
  if (look.ring or 0) > 0 then
    gfx:ring(c, c, look.ring_radius, look.ring_width, anim.mix_color("#000000", look.ring_color, look.ring))
  end
  local fan = ui.fan_on
  -- Drawn as it is, not from a layer: the number, the words about it
  -- (the time to the target among them) and the panel's colour behind
  -- them change at every detent, and each is a cached run of text or a
  -- symbol's own layer.
  details(g, c, t, m, side, fan, status, look.fade, look.figure, zoomed, look.issue)
  gfx:restore()
  if light < 1 then
    gfx:circle(c, c, D.rim + D.rim_width, string.format("#000000%02x", math.floor((1 - light) * 255 + 0.5)))
  end
  return animating
end

-- Stock's Farsight, lit by someone in the room: the background as at
-- home and the target alone, 1.45 times larger about the centre (the
-- temperature layer's 0x1d3b84), or the room's label alone, 7.21 times
-- larger and centred (the dial's 0x1ddfb4, moved by 0x1dcf78).
local FAR, FAR_ROOM = 1.45, 7.21

-- How far someone near has brought Farsight's near view in, 0 to 1, and
-- whether it is still coming: stock's 500 ms on its sin³ ease (state 5,
-- 0x42bd94's curve 0), from when the engine starts it.
local NEAR = 0.5
local function nearness(ui)
  local at = ui.changed.near
  if not at then return 0, false end
  local t = (ui.now - at) / NEAR
  if t >= 1 then return 1, false end
  if t <= 0 then return 0, true end
  return math.sin(t * math.pi / 2) ^ 3, true
end

-- Stock's curve 4 (0x42bd94) at `t` 0 to 1: 1.68 sin²(90t°) to two
-- thirds of the way, then a sinking wave about 1.0588, overshooting to
-- 1.26 and settling on 1.
local function overshoot(t)
  local a = t * 90
  if t < 2 / 3 then return 1.68 * math.sin(math.rad(a)) ^ 2 end
  return math.sin(math.rad(a * 6.5)) * 0.4 * math.exp(-3 * math.rad(a - 60)) + 1.0588
end

-- Farsight's clocks and weather arriving as the display lights (stock's
-- state 4: 500 ms on curve 4, 0x144744): how far, and whether still
-- coming. A display lit by someone near shows its near view at once.
local ARRIVE = 0.5
local function arrival(ui)
  local near = ui.changed.near
  if near and near < ui.changed.screen then return 1, false end
  local t = (ui.now - ui.changed.screen) / ARRIVE
  if t >= 1 then return 1, false end
  return overshoot(math.max(t, 0)), true
end

-- How far Farsight's clocks or weather have faded out after the input
-- that left them, 0 to 1, or nil: stock's state 6, 500 ms on its sin³
-- ease, the analog clock's 750 ms (0x1450a0).
local LEAVE, LEAVE_ANALOG = 0.5, 0.75
local function leaving(ui, analog)
  local at = ui.changed.leave
  if not at then return nil end
  local t = anim.clamp((ui.now - at) / (analog and LEAVE_ANALOG or LEAVE), 0, 1)
  return math.sin(t * math.pi / 2) ^ 3
end

-- Farsight's analog clock on black: the hour hand 14.5 wide to 109, the
-- minute hand 10 wide to 157, both round-ended and white, the second
-- hand #0bdbff and 3.5 wide to 200, over a white hub of 19. As someone comes
-- near (`p` 0 to 1) a #4d4d4d dial fades in, twelve ticks 6 wide and
-- round-ended growing in from 229 to 183, and the weekday and day ("SAT
-- 17") in #00a3d9, 36 tall on 144 under the centre (0x144444, 0x142b64,
-- 0x144268). The hands and hub show at `hands` of their length, the hub
-- while they do.
local function analog_clock(gfx, c, clock, p, hands)
  if p > 0 then
    local grey = faded("#4d4d4d", p)
    local inner = math.min(235 - 52 * p, 229)
    for i = 0, 11 do
      local x1, y1 = polar(c, 229, i * 30)
      local x2, y2 = polar(c, inner, i * 30)
      gfx:line(x1, y1, x2, y2, 6, grey, true)
    end
    if clock.weekday_day then
      gfx:text(clock.weekday_day, c, c + 144, { font = F.strong, size = 36, color = faded("#00a3d9", p), align = "center" })
    end
  end
  local function hand(degrees, length, width, color, round)
    local a = math.rad(degrees)
    gfx:line(c, c, c + length * math.sin(a), c - length * math.cos(a), width, color, round)
  end
  if hands <= 0 then return end
  local m = clock.minute + clock.second / 60
  hand((clock.hour % 12 + m / 60) * 30, 109 * hands, 14.5, "#ffffff", true)
  hand(m * 6, 157 * hands, 10, "#ffffff", true)
  hand(clock.second * 6, 200 * hands, 3.5, "#0bdbff", false)
  gfx:circle(c, c, 19, "#ffffff")
end

-- Farsight's digital clock on black: the hour unpadded, bold digits 117
-- tall on 297, the colon two round dots centred on them. As someone comes
-- near (`p` 0 to 1) it shrinks to 0.8 about the centre and rises 20, and
-- the weekday-date fades in under it ("Saturday, October 17"), 43 tall
-- easing to 36, its baseline 62 under the digits' easing to 52 (0x1437bc).
local function digital_clock(gfx, c, clock, p)
  local hour = clock.hour
  if clock.twelve_hour then
    hour = hour % 12
    if hour == 0 then hour = 12 end
  end
  local s = 1 - 0.2 * p
  gfx:save()
  gfx:translate(c, c - 20 * p)
  gfx:scale(s)
  gfx:translate(-c, -c)
  local style = { font = F.strong, size = 166, color = "#ffffff" }
  local h, m = tostring(hour), string.format("%02d", clock.minute)
  local wh, wm = gfx:measure(h, style), gfx:measure(m, style)
  local gap, dot = 14, 10.5
  local x = c - (wh + 2 * gap + 2 * dot + wm) / 2
  gfx:text(h, x, 297, style)
  local cx = x + wh + gap + dot
  gfx:circle(cx, 214, dot, "#ffffff")
  gfx:circle(cx, 263, dot, "#ffffff")
  gfx:text(m, cx + dot + gap, 297, style)
  gfx:restore()
  if p > 0 and clock.weekday_date then
    local base = c + (297 - c) * s - 20 * p
    gfx:text(clock.weekday_date, c, base + 52 + 10.4 * (1 - p),
      { font = F.strong, size = 36 + 7.2 * (1 - p), color = faded("#ffffff", p), align = "center" })
  end
end

-- The weather failure's cloud, 287 by 187 about (240, 239) at `scale` 1,
-- with an exclamation mark cut out of it (stock's glyph 0xe092).
local function cloud(gfx, x, y, scale)
  local white = "#ffffff"
  gfx:save()
  gfx:translate(x, y)
  gfx:scale(scale)
  gfx:translate(-240, -239)
  gfx:circle(229, 221, 75, white)
  gfx:circle(313, 252, 70, white)
  gfx:circle(146, 283, 49, white)
  gfx:circle(318, 277, 55, white)
  gfx:rect(146, 240, 172, 92, white)
  gfx:circle(239.5, 213, 12.5, "#000000")
  gfx:polygon({ 227, 213, 252, 213, 247.5, 262, 231.5, 262 }, "#000000")
  gfx:circle(239.5, 262, 8, "#000000")
  gfx:circle(239.5, 293.5, 10, "#000000")
  gfx:restore()
end

-- Farsight's weather: the unit's picture for it (a sheet of 158 by 118
-- frames, five a row, played at 24 a second) beside the outside
-- temperature, shrinking to 0.8 and rising 66 as someone comes near (`p`
-- 0 to 1, 0x143c48), the forecast's three hours fading in under it,
-- rising 200 to their place 110 apart: the temperature 48 tall in white on
-- 100 under the centre, centred without its degree sign, the time 32 tall
-- in #646464 on 140, both in the medium face (0x1423fc, under 0x1441a0's
-- akkuratfontmedium). Without the weather, the cloud, centred and alone,
-- growing into its text block's glyph line as someone comes near (0x143fe4:
-- the glyph line, 405 shrinking to 101, then why there is none, offline or
-- the fetch failed with nest.com/m2, and OK, highlighted, OK on 407). The
-- text, nest.com/m2 in #00bfff and OK fade in with nearness (0x41264c,
-- 0x4176a8): far, online (nlNetworkConnectivity::IsConnected) with the
-- fetch failing, stock shows the cloud alone.
local function weather(gfx, c, ui, p)
  local t = ui.thermostat
  if t.outdoor and t.weather_image then
    local text = string.format("%d°", math.floor(t.outdoor + 0.5))
    local style = { font = F.number, size = 162, color = "#ffffff" }
    local w = gfx:measure(text, style)
    local gap = 18
    local x = c - (158 + gap + w) / 2
    local s = 1 - 0.2 * p
    gfx:save()
    gfx:translate(c, c - 66 * p)
    gfx:scale(s)
    gfx:translate(-c, -c)
    local shown = gfx:sprite("unit:" .. t.weather_image, math.floor(ui.now * 24), 5, 158, 118, x, 181)
    if shown then gfx:text(text, x + 158 + gap, 298, style) end
    gfx:restore()
    if shown and p > 0 and #t.forecast == 3 then
      local rise = 200 * (1 - p)
      local temp_style = { font = F.number, size = 48, color = faded("#ffffff", p) }
      local time_style = { font = F.number, size = 32, color = faded("#646464", p), align = "center" }
      for i, hour in ipairs(t.forecast) do
        local hx = c + (i - 2) * 110
        local degrees = math.floor(hour.temp + 0.5)
        local w = gfx:measure(tostring(degrees), temp_style)
        gfx:text(string.format("%d°", degrees), hx - w / 2, c + 100 + rise, temp_style)
        gfx:text(hour.time, hx, c + 140 + rise, time_style)
      end
    end
    if shown then return true end
  end
  local style = { font = F.strong, size = 30, spacing = 0.9, color = faded("#ffffff", p), align = "center" }
  local lines, first = alert.block(gfx, t.online and "Can't get the weather right now."
    or "Can't display the weather because your thermostat is offline.", style, t.online)
  if t.online then
    gfx:text("nest.com/m2", c, first + (#lines - 1) * 39 + 61,
      { font = F.strong, size = 30, spacing = 0.9, color = faded("#00bfff", p), align = "center" })
  end
  if p <= 0 then
    cloud(gfx, 240, 239, 1)
    return false
  end
  cloud(gfx, 240, anim.mix(239, 77, p), 1 - 304 / 405 * p)
  for i, l in ipairs(lines) do gfx:text(l, c, first + (i - 1) * 39, style) end
  local pill = { font = F.label, size = 35.7, align = "left", color = faded("#ffffff", p) }
  local w = gfx:measure("OK", pill)
  gfx:rect(c - w / 2 - 24, 372, w + 48, 48, faded(theme.menu.pill, p), 24)
  gfx:text("OK", c - w / 2, 407, pill)
  return false
end

-- How strongly an alert over Farsight shows, 0 to 1 (0x227364): not while
-- Farsight is far (states 2 and 4), coming in with the home screen or the
-- near view (state 5) and as the clocks or weather give way from afar
-- (state 6), whole over the near view.
function home.over(ui)
  local kind = ui.thermostat.farsight
  local views = kind == "analog_clock" or kind == "digital_clock" or kind == "weather"
  local near, left = ui.changed.near, ui.changed.leave
  if left then
    if views and near and near <= left then return 1 end
    return leaving(ui, kind == "analog_clock")
  end
  return (nearness(ui))
end

function home.farsight(gfx, ui)
  local c = ui.face.size / 2
  local t = ui.thermostat
  local kind = t.farsight
  if kind == "analog_clock" or kind == "digital_clock" or kind == "weather" then
    -- Arriving, they grow in from 1 / 1.2 of their size on curve 4 as they
    -- fade in, overshooting; left, they shrink to 1 / 1.45 as they fade
    -- out, near or far as they were (0x144744 divides their size by
    -- 1 + 0.2 (1 - v), then 1 + 0.45 v), the analog hands shrinking away
    -- over the first fifth (0x144444), and the home screen fades in under
    -- them (0x1da9d4: its background, dial and temperatures at the
    -- leaving's progress).
    local p, moving = nearness(ui)
    local alpha, grow, hands
    local out = leaving(ui, kind == "analog_clock")
    if out then
      p = (ui.changed.near and ui.changed.near <= ui.changed.leave) and 1 or 0
      alpha, grow, hands, moving = 1 - out, 1 / (1 + 0.45 * out), anim.clamp(1 - out / 0.2, 0, 1), true
      home.draw(gfx, ui, { light = out })
    else
      local v, arriving = arrival(ui)
      alpha, grow, hands, moving = math.min(v, 1), 1 / (1 + 0.2 * (1 - v)), 1, moving or arriving
      gfx:clear("#000000")
    end
    local playing = false
    local function view(g)
      g:save()
      g:translate(c, c)
      g:scale(grow)
      g:translate(-c, -c)
      if kind == "analog_clock" then
        analog_clock(g, c, ui.clock, p, hands)
      elseif kind == "digital_clock" then
        digital_clock(g, c, ui.clock, p)
      else
        playing = weather(g, c, ui, p)
      end
      g:restore()
    end
    -- Faded, the view is drawn whole into a layer and painted at its
    -- opacity, so that its overlapping shapes (the cloud's) fade as one.
    if alpha < 1 then
      gfx:layer("farsight view", ui.now, view)
      gfx:paint_layer("farsight view", faded("#ffffff", alpha))
    else
      view(gfx)
    end
    return moving or playing
  end
  local m = state_of(t)
  local colors = m.colors
  local current = kind == "current_temp"
  local big = current and FAR_ROOM or FAR
  local side = (t.mode ~= "off" and angle(m.target + (t.nudge or 0)) >= m.a_current) and -1 or 1
  local lx, ly = polar(c, D.label_radius, m.a_current + side * D.label_angle)
  -- Where (x, y) of the home screen shows grown `zoom` times about the
  -- centre and moved `shift` times the room's label's place towards it.
  local function at(x, y, zoom, shift)
    return c + zoom * (x - c) - shift * (lx - c), c + zoom * (y - c) - shift * (ly - c)
  end
  -- The target and the room's label are set at their size, not grown.
  local function target(zoom, shift, a)
    local x, y = at(c, 300, zoom, shift)
    number(gfx, x, t.target, t.unit, y, a, zoom)
  end
  -- Far, the label's whole part is centred, as the target's is.
  local function label(zoom, shift, color, lone)
    local x, y = at(lx, ly, zoom, shift)
    label_at(gfx, t, t.current, { current = color, bg = colors.bg }, x, y, zoom, lone)
  end
  -- Off, Eco and heat-cool: the home screen's middle (Off's words, ECO,
  -- the range's numbers) grown `zoom` times about the centre at opacity
  -- `a`, as stock's temperature layer draws it (0x1d3b84).
  local function middle(zoom, a)
    gfx:save()
    gfx:translate(c, c)
    gfx:scale(zoom)
    gfx:translate(-c, -c)
    local g = sharp(gfx, zoom)
    if t.eco then
      eco_text(g, c, a)
    elseif t.mode == "off" then
      off_text(g, c, t, a)
    else
      range(g, c, t, colors, true, a)
    end
    gfx:restore()
  end
  -- A drained battery's U+E037 shows far too, grown as the middle is, at
  -- opacity `a` (0x1d0624's param_6: whole in states 2, 5 and 6, coming
  -- in with v in 4); the room's view has none.
  local function issue(zoom, a)
    if current or not t.system_issue then return end
    gfx:save()
    gfx:translate(c, c)
    gfx:scale(zoom)
    gfx:translate(-c, -c)
    gfx:alpha(a)
    icons.draw(sharp(gfx, zoom), "issue", c, 404, 48, { color = "#ffbb00", live = true })
    gfx:restore()
  end
  local plain = not t.eco and t.mode ~= "off" and t.mode ~= "heat-cool"
  local out = leaving(ui, false)
  if out then
    -- An input or someone near: the home screen comes in over 500 ms on
    -- sin³ (state 5), shrinking to its size as its dial, words and symbol
    -- fade in (0x1ddfb4, 0x1d3b84). The target or the middle shrinks from
    -- 1.45 about the centre; the room's label from 7.21, sliding out to
    -- its place and turning from white to its colour (0x1dcf78, 0x1db3e0),
    -- the target or middle fading in with the dial, the system-issue
    -- symbol whole but in the room's view.
    local zoom = big - (big - 1) * out
    local shift = current and big * (1 - out) or 0
    local figure = plain and 0 or (current and out or 1)
    local animating = home.draw(gfx, ui, { zoom = zoom, shift = { -shift * (lx - c), -shift * (ly - c) },
                                           dial = out, fade = out, figure = figure, label = not current,
                                           issue = not current and 1 or nil })
    if plain then target(zoom, shift, current and out or 1) end
    if current then
      gfx:save()
      gfx:alpha(label_hidden(t) and 1 - out or 1)
      label(zoom, shift, anim.mix_color("#ffffff", colors.current, out), 1 - out)
      gfx:restore()
    end
    gfx:ring(c, c, D.rim, D.rim_width, "#000000")
    return animating or out < 1
  end
  -- Arriving, it grows from 0.8 of its size on curve 4, the target or
  -- middle and the system-issue symbol fading in as it does (state 4:
  -- 0x1d3b84, 0x1ddfb4).
  local v, arriving = arrival(ui)
  local zoom = big * (0.8 + 0.2 * v)
  gfx:clear((t.mode == "off" and not current) and P.idle.bg.near or colors.bg)
  if current then
    gfx:save()
    gfx:alpha(math.min(v, 1))
    label(zoom, zoom, "#ffffff", 1)
    gfx:restore()
  elseif plain then
    target(zoom, 0, math.min(v, 1))
  else
    middle(zoom, math.min(v, 1))
  end
  issue(zoom, math.min(v, 1))
  gfx:ring(c, c, D.rim, D.rim_width, "#000000")
  return arriving
end

return home
