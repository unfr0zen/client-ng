-- The home screen as a heads-up display: a bezel of ticks, a gauge of
-- segments lit from the room's temperature to the target in what the
-- system is doing (warm when heating, cool when cooling), the target's
-- pointer and the room's marker; a ring turning while the system runs;
-- readouts in monospace, and scan lines over it all.
--
-- What does not change is drawn once into layers; the gauge's segments are
-- one layer painted in pieces.
local anim = require("classic:anim")
local theme = require("theme")
local C, G, Z, R = theme.colors, theme.gauge, theme.bezel, theme.spinner

local home = {}

local function polar(c, r, degrees)
  local a = math.rad(degrees)
  return c + r * math.sin(a), c - r * math.cos(a)
end

local function angle(t, value)
  local f = anim.clamp((value - t.min) / (t.max - t.min), 0, 1)
  return G.from + f * (G.to - G.from)
end

-- The system's colour: its accent and what it is doing, as a word.
local function status(t)
  if t.range then
    local name = t.range == "low" and "HEAT" or "COOL"
    return t.range == "low" and C.orange or C.cyan, (t.range_editing and "SET " or "SELECT ") .. name
  end
  if t.mode == "off" then return C.comment, "SYSTEM OFF" end
  if t.hvac == "heating" then return C.orange, "HEATING" end
  if t.hvac == "cooling" then return C.cyan, "COOLING" end
  return C.green, "STANDBY"
end

local MODE_NAME = { heat = "HEAT", cool = "COOL", ["heat-cool"] = "AUTO", off = "OFF" }

local function fixed(value, unit)
  if unit == "F" then return string.format("%d", math.floor(value + 0.5)) end
  return string.format("%.1f", value)
end

-- Convex pieces covering the gauge between angles a0 and a1.
local function band(c, a0, a1)
  local r0, r1 = G.inner - 4, G.outer + 4
  local pieces = {}
  local n = math.max(1, math.ceil((a1 - a0) / 60))
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

-- Paints the segments whose middles lie between angles a0 and a1.
local function paint_segments(gfx, c, a0, a1, color)
  local half = G.step / 2
  local first = math.ceil((a0 - G.from - half) / G.step - 1e-6)
  local last = math.floor((a1 - G.from - half) / G.step + 1e-6)
  if last < first then return end
  for _, piece in ipairs(band(c, G.from + first * G.step, G.from + (last + 1) * G.step)) do
    gfx:paint_layer("segments", color, piece)
  end
end

-- The layers drawn once: the bezel, the segments in white, the spinner's
-- dashes and the scan lines.
local function layers(gfx, c, size)
  gfx:layer("bezel", "1", function(g)
    g:circle(c, c, c, C.face)
    local minor, major = {}, {}
    for i = 0, Z.count - 1 do
      local a = 360 * i / Z.count
      local list = (i % Z.major == 0) and major or minor
      local x1, y1 = polar(c, (i % Z.major == 0) and Z.inner or Z.minor, a)
      local x2, y2 = polar(c, Z.outer, a)
      list[#list + 1] = x1; list[#list + 1] = y1; list[#list + 1] = x2; list[#list + 1] = y2
    end
    g:lines(minor, 1.5, C.rule)
    g:lines(major, 2.5, C.comment)
    g:ring(c, c, Z.outer + 4, 1.5, C.rule)
    g:ring(c, c, G.inner - 12, 1, C.gutter)
    -- Bracket corners round the readout.
    local segs = {}
    for _, corner in ipairs({ { -1, -1 }, { 1, -1 }, { -1, 1 }, { 1, 1 } }) do
      local x, y = c + corner[1] * 104, c + 10 + corner[2] * 84
      segs[#segs + 1] = x; segs[#segs + 1] = y; segs[#segs + 1] = x - corner[1] * 18; segs[#segs + 1] = y
      segs[#segs + 1] = x; segs[#segs + 1] = y; segs[#segs + 1] = x; segs[#segs + 1] = y - corner[2] * 18
    end
    g:lines(segs, 2, C.rule)
  end)
  gfx:layer("segments", "1", function(g)
    local n = math.floor((G.to - G.from) / G.step + 0.5)
    for i = 0, n - 1 do
      local a0 = G.from + i * G.step + G.step * (1 - G.fill) / 2
      local a1 = a0 + G.step * G.fill
      local x1, y1 = polar(c, G.inner, a0)
      local x2, y2 = polar(c, G.outer, a0)
      local x3, y3 = polar(c, G.outer, a1)
      local x4, y4 = polar(c, G.inner, a1)
      g:polygon({ x1, y1, x2, y2, x3, y3, x4, y4 }, "#ffffff")
    end
  end)
  gfx:layer("spinner", "1", function(g)
    for i = 0, R.dashes - 1 do
      local a = 360 * i / R.dashes
      g:arc(c, c, R.radius, a, a + R.length, R.width, "#ffffff")
      g:arc(c, c, R.radius - 9, a + R.length * 0.6, a + R.length, 1.5, "#ffffff")
    end
  end)
  gfx:layer("scan", tostring(theme.scan), function(g)
    local segs = {}
    for y = 0, size, theme.scan do
      segs[#segs + 1] = 0; segs[#segs + 1] = y; segs[#segs + 1] = size; segs[#segs + 1] = y
    end
    g:lines(segs, 1, "#ffffff")
  end)
end

-- A label and its value, in two colours, as one run centred on x.
local function pairs_line(g, x, y, items, size)
  local label = { font = "mono", size = size, color = C.comment }
  local value = { font = "mono_medium", size = size }
  local total = 0
  for i, it in ipairs(items) do
    total = total + g:measure(it[1] .. " ", label) + g:measure(it[2], value) + (i > 1 and size * 1.5 or 0)
  end
  local at = x - total / 2
  for i, it in ipairs(items) do
    if i > 1 then at = at + size * 1.5 end
    at = at + g:text(it[1] .. " ", at, y, label)
    value.color = it[3]
    at = at + g:text(it[2], at, y, value)
  end
end

local function two(n) return string.format("%02d", n) end

-- The readouts, which change only with the thermostat and the clock.
local function readout(g, c, t, clock)
  local accent, word = status(t)
  local hour = clock.hour
  if clock.twelve_hour then hour = (hour + 11) % 12 + 1 end
  g:text(two(hour) .. ":" .. two(clock.minute), c, 124, { font = "mono", size = 20, color = C.comment, align = "center" })
  g:text("▸ " .. tr(word), c, 162, { font = "mono_medium", size = 21, spacing = 2, color = accent, align = "center" })
  if t.mode == "off" then
    g:text("OFF", c, 262, { font = "mono_light", size = 96, color = C.comment, align = "center" })
  elseif t.mode == "heat-cool" then
    local style = { font = "mono_light", size = 56, color = C.bright }
    local low, high = fixed(t.low, t.unit), fixed(t.high, t.unit)
    local function tone(color, side) return (t.range and t.range ~= side) and C.rule or color end
    g:text(low, c - 14, 254, { font = style.font, size = style.size, color = tone(C.orange, "low"), align = "right" })
    g:text("/", c, 254, { font = "mono_light", size = 50, color = C.comment, align = "center" })
    g:text(high, c + 14, 254, { font = style.font, size = style.size, color = tone(C.cyan, "high") })
  else
    local big = { font = "mono_light", size = 104, color = C.bright }
    local text = fixed(t.target, t.unit)
    local w = g:measure(text, big)
    local x = c - w / 2 - 12
    g:text(text, x, 262, big)
    g:text("°" .. t.unit, x + w + 10, 210, { font = "mono", size = 26, color = C.blue })
  end
  local items = { { "AMB", fixed(t.current, t.unit) .. "°", C.green } }
  if t.humidity then items[2] = { "HUM", string.format("%d%%", math.floor(t.humidity + 0.5)), C.purple } end
  pairs_line(g, c, 318, items, 19)
  g:text("[ ", c - 50, 420, { font = "mono", size = 22, color = C.comment, align = "right" })
  g:text(MODE_NAME[t.mode] or string.upper(t.mode), c, 420, { font = "mono_medium", size = 22, spacing = 2, color = C.purple, align = "center" })
  g:text(" ]", c + 50, 420, { font = "mono", size = 22, color = C.comment })
end

-- The target's pointer: a notch outside the gauge and a hairline across it.
local function pointer(gfx, c, a, color)
  local tip_x, tip_y = polar(c, G.outer + 4, a)
  local l_x, l_y = polar(c, G.outer + 16, a - 2.6)
  local r_x, r_y = polar(c, G.outer + 16, a + 2.6)
  gfx:polygon({ tip_x, tip_y, r_x, r_y, l_x, l_y }, color)
  local x1, y1 = polar(c, G.inner - 8, a)
  local x2, y2 = polar(c, G.outer, a)
  gfx:line(x1, y1, x2, y2, 2, color)
end

-- The room's marker: a diamond inside the gauge.
local function marker(gfx, c, a, color)
  local x, y = polar(c, G.inner - 9, a)
  local ax, ay = math.sin(math.rad(a)), -math.cos(math.rad(a))
  local s = 5
  gfx:polygon({ x + ax * s, y + ay * s, x - ay * s, y + ax * s, x - ax * s, y - ay * s, x + ay * s, y - ax * s }, color)
end

function home.background()
  return C.face
end

local function face(gfx, ui, c)
  local t = ui.thermostat
  local accent = status(t)
  layers(gfx, c, ui.face.size)
  gfx:paint_layer("bezel", "#ffffff")
  gfx:paint_layer("segments", C.gutter)
  local a_current = angle(t, t.current)
  local running = t.hvac == "heating" or t.hvac == "cooling"
  if t.mode == "heat-cool" then
    local lo, hi = angle(t, t.low), angle(t, t.high)
    paint_segments(gfx, c, lo, hi, C.rule)
    if running then
      local a = t.hvac == "heating" and lo or hi
      paint_segments(gfx, c, math.min(a, a_current), math.max(a, a_current), accent)
    end
    pointer(gfx, c, lo, (t.range == "high") and C.rule or C.orange)
    pointer(gfx, c, hi, (t.range == "low") and C.rule or C.cyan)
  elseif t.mode ~= "off" then
    local a_target = angle(t, t.target)
    paint_segments(gfx, c, G.from, math.min(a_target, a_current), C.rule)
    paint_segments(gfx, c, math.min(a_target, a_current), math.max(a_target, a_current), accent)
    pointer(gfx, c, a_target, C.yellow)
  end
  marker(gfx, c, a_current, C.green)

  if running then
    gfx:save()
    gfx:translate(c, c)
    gfx:rotate((ui.now * R.speed) % 360)
    gfx:translate(-c, -c)
    gfx:paint_layer("spinner", accent)
    gfx:restore()
  end

  local clock = ui.clock
  local key = table.concat({ t.mode, t.hvac, t.target, t.current, t.unit, t.low, t.high, tostring(t.humidity),
                             t.range or "", tostring(t.range_editing),
                             clock.hour, clock.minute, tostring(clock.twelve_hour) }, "|")
  gfx:layer("readout", key, function(g) readout(g, c, t, clock) end)
  gfx:paint_layer("readout", "#ffffff")
  gfx:paint_layer("scan", "#0000002e")
  return running
end

-- Draws the home screen; `look` shrinks (`scale`), darkens (`light`) or
-- narrows (`squash`) it, for the menus' transitions.
function home.draw(gfx, ui, look)
  look = look or {}
  local scale, light, squash = look.scale or 1, look.light or 1, look.squash or 1
  local c = ui.face.size / 2
  if scale < 1 or squash < 1 then
    gfx:clear("#000000")
  else
    gfx:clear(light < 1 and anim.mix_color("#000000", C.bg, light) or C.bg)
  end
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

return home
