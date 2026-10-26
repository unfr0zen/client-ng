-- The top menu, as stock's ring: its entries' symbols round the face, the
-- selected one larger, further in and in its colours (each fading over
-- `select` seconds); the middle names it, or on Back shows the weather,
-- the day and time, the room and its humidity.
local Component = require("components.component")
local theme = require("theme")
local icons = require("icons")
local anim = require("anim")
local format = require("components.format")

local M = theme.menu
local Ring = Component:extend({
  -- The symbols' angles (degrees clockwise from 12 o'clock), a list per
  -- count of entries, otherwise spread evenly; their radius and size,
  -- selected or not.
  slots = M.slots, radius = M.radius, radius_selected = M.radius_selected,
  icon = M.icon, icon_selected = M.icon_selected, select = M.select,
  -- The middle: an entry's name and its value's lines, on these baselines
  -- for one, two or three lines, half a pixel right of the middle; the
  -- last one's fading out in `fade_out` s, this one's in over `fade_in`;
  -- a choice's value giving way to its hint and back in each
  -- `hint_cycle` s from when the ring settles (`hint_steps` the fractions
  -- of it the slides run between), the text going out sliding down a
  -- line's height (more, over more lines) and the text coming in from as
  -- far above, seen only between `hint_above` less than that height over
  -- the line's baseline and `hint_below` less than its lines' heights
  -- under (stock's black masks, 0x1a9c00;
  -- its text height is the face's size, 0x425e54). Settings' notices in `links` have
  -- their link under them, in `link`'s colour, and those in `badges` a
  -- badge on its symbol; a notice wraps at `notice_width`, the fan's
  -- line at `sub_width`.
  title = { font = theme.fonts.label, size = 35.7, color = M.title },
  sub = { font = theme.fonts.label, size = 28, color = M.sub }, sub_width = 380, notice_width = 300,
  link = { font = theme.fonts.label, size = 28, color = M.link },
  links = { ["Wi-Fi connection error"] = "nest.com/w5", ["System issue"] = "nest.com/m13" },
  badges = { "Wi-Fi connection error", "System issue", "Internet disconnected" },
  baselines = { { 251 }, { 231, 271 }, { 214, 254, 288 } },
  fade_out = 0.15, fade_in = 0.15,
  hint_cycle = M.hint_cycle, hint_steps = M.hint_steps, hint_above = 7, hint_below = 22,
  -- Back's day and time: two texts, each on its nearest pixel, the pair
  -- centred half a pixel right of the middle on `y` (`weather_dy` lower
  -- under the weather).
  clock = { font = theme.fonts.label, size = 36, color = M.title, spacing = 1.46, y = 231, weather_dy = 20 },
  -- The weather over them: the first frame of the unit's picture at
  -- `scale` and the outside temperature `gap` into the frame's margin,
  -- the pair `left` left of the middle (stock 6.5's: it no longer plays
  -- the picture's frames).
  weather = { scale = 0.454, gap = -0.83, left = 2.9, top = 148.9, frame_w = 158, frame_h = 118, frames = 5, text_y = 199,
              font = theme.fonts.label, size = 46, color = M.sub },
  -- The room: the house, `house_gap` on the temperature, `drop` on from
  -- its end the drop's middle and `drop_gap` on the humidity, centred
  -- `shift` left of the middle (`weather_dy` lower under the weather).
  room = { font = theme.fonts.label, size = 32, color = M.sub, house = 24, house_gap = 7, drop = 21.94, drop_gap = 14,
           shift = 6.14, y = 271, text_y = 282, weather_dy = 18 },
})

-- How selected each entry looks (0 to 1), and the middle's lines now and
-- before.
local state = {}
function Ring.reset()
  state.chosen, state.centre = {}, { key = nil, since = -100, last = nil }
end
Ring.reset()

local temperature_cache = {}
local function temperature(t)
  local c = temperature_cache
  if c.current ~= t.current or c.unit ~= t.unit then
    c.current, c.unit, c.text = t.current, t.unit, format.temperature(t)
  end
  return c.text
end

-- An entry whose symbol carries a badge: Settings with a Wi-Fi notice
-- under it.
local function badged(self, item)
  if item.icon ~= "settings" or item.value == nil then return false end
  for _, notice in ipairs(self.badges) do
    if item.value == tr(notice) then return true end
  end
  return false
end

-- A text in the language cut into lines no wider than `width`, kept
-- until it changes.
local wrapped = {}
local function wrap(self, gfx, text, width)
  text = tr(text)
  if wrapped.text ~= text or wrapped.width ~= width then
    wrapped.text, wrapped.width, wrapped.lines = text, width, require("alert").wrap(gfx, text, self.sub, width)
  end
  return wrapped.lines
end

-- What the middle says for an entry.
function Ring:lines(ui, item, gfx)
  if item.kind == "back" then return { "back" } end
  local lines = { string.upper(item.label) }
  -- A dial says nothing more, but when its fan runs until, or the fan
  -- schedule; a choice's value gives way to how to change it, as stock's
  -- does.
  if item.style == "dial" then
    local fan = ui.fan and ui.fan.ends or ui.fan_schedule
    if fan then
      for _, l in ipairs(wrap(self, gfx, fan, self.sub_width)) do lines[#lines + 1] = l end
    end
    return lines
  end
  local value = item.value
  local hint = item.kind == "choice" and value and "Press to change" or nil
  if item.icon == "mode" then
    -- Stock's Mode (0x1a9c00): with heating and cooling both a choice,
    -- else a press turning the system off and on (0x1a6e04). Off says so;
    -- Eco says only the press; emergency heat stands for Heat.
    local t = ui.thermostat
    local both = t.can_heat and t.can_cool
    if t.mode == "off" then
      hint = both and "Press to change" or "Press to turn on"
    elseif t.eco then
      value, hint = both and "Press to change" or "Press to turn off", nil
    else
      if t.emergency then value = "Emergency heat" end
      hint = both and "Press to change" or "Press to turn off"
    end
  end
  if item.icon == "settings" and value and value ~= "" then
    for _, l in ipairs(wrap(self, gfx, value, self.notice_width)) do lines[#lines + 1] = l end
  elseif value then
    for part in value:gmatch("[^\n]+") do lines[#lines + 1] = part end
  end
  if badged(self, item) then
    for notice, link in pairs(self.links) do
      if item.value == tr(notice) then
        lines[#lines + 1] = link
        lines.link = #lines
      end
    end
  end
  lines.hint = hint
  return lines
end

-- The weather over Back's lines; whether drawn.
function Ring:draw_weather(gfx, ui, c)
  local t = ui.thermostat
  if not (t.outdoor and t.weather_image) then return false end
  local w = self.weather
  local style = { font = w.font, size = w.size, color = w.color }
  local text = format.temperature({ current = t.outdoor, unit = t.unit })
  local tw = gfx:measure(text, style)
  local x = c - w.left - (w.frame_w * w.scale + w.gap + tw) / 2
  gfx:save()
  gfx:translate(x, w.top)
  gfx:scale(w.scale)
  local drawn = gfx:sprite("unit:" .. t.weather_image, 0, w.frames, w.frame_w, w.frame_h, 0, 0)
  gfx:restore()
  if not drawn then return false end
  gfx:text(text, x + w.frame_w * w.scale + w.gap, w.text_y, style)
  return true
end

-- How far a choice's hint has come `age` s into its cycle (0 to 1),
-- whether it is going back to the value then, and the seconds until the
-- next slide starts (0 while one runs).
function Ring:hint_roll(age)
  local s, cycle = self.hint_steps, self.hint_cycle
  local p = (age % cycle) / cycle
  if p < s[1] then return 0, false, (s[1] - p) * cycle end
  if p < s[2] then return (p - s[1]) / (s[2] - s[1]), false, 0 end
  if p < s[3] then return 1, false, (s[3] - p) * cycle end
  if p < s[4] then return (p - s[3]) / (s[4] - s[3]), true, 0 end
  return 0, false, (1 + s[1] - p) * cycle
end

-- The middle's `lines` at `alpha`, a choice's hint rolled in by `roll`
-- (its value rolled back in when `back`).
function Ring:draw_centre(gfx, ui, lines, alpha, c, roll, back)
  roll = roll or 0
  if alpha <= 0 then return end
  gfx:save()
  gfx:alpha(alpha)
  if lines[1] == "back" then
    local weather = self:draw_weather(gfx, ui, c)
    local k, r = self.clock, self.room
    local clock = { font = k.font, size = k.size, color = k.color, spacing = k.spacing }
    local day, time = format.clock(ui.clock):match("^(.*) (%S+)$")
    local wd, wt = gfx:measure(day, clock), gfx:measure(time, clock)
    local gap = gfx:measure(" ", clock) + 2 * k.spacing
    local x = c + 0.5 / (ui.face.scale or 1) - (wd + gap + wt) / 2
    local y = k.y + (weather and k.weather_dy or 0)
    gfx:text(day, x, y, clock)
    gfx:text(time, x + wd + gap, y, clock)
    local t = ui.thermostat
    local temp = temperature(t)
    local hum = t.humidity and string.format("%d%%", math.floor(t.humidity + 0.5))
    local style = { font = r.font, size = r.size, color = r.color }
    local dy = weather and r.weather_dy or 0
    local tw = gfx:measure(temp, style)
    local span = r.house + r.house_gap + tw + (hum and r.drop + r.drop_gap + gfx:measure(hum, style) or 0)
    x = c - r.shift - span / 2
    icons.draw(gfx, "home", x + r.house / 2, r.y + dy, r.house, { p = 1 })
    x = x + r.house + r.house_gap
    gfx:text(temp, x, r.text_y + dy, style)
    if hum then
      x = x + tw + r.drop
      icons.draw(gfx, "drop", x, r.y + dy, r.house, { p = 1 })
      gfx:text(hum, x + r.drop_gap, r.text_y + dy, style)
    end
  else
    local base = self.baselines[math.min(#lines, 3)]
    local title = { font = self.title.font, size = self.title.size, color = self.title.color, align = "center" }
    local sub = { font = self.sub.font, size = self.sub.size, color = self.sub.color, align = "center" }
    local link = { font = self.link.font, size = self.link.size, color = self.link.color, align = "center" }
    -- Half a pixel right of the middle, as the clock.
    c = c + 0.5 / (ui.face.scale or 1)
    local rolled = lines.hint and roll > 0
    for i = 1, math.min(#lines, 3) do
      if i == 2 and rolled then
        -- The text going out slides down and fades as the other comes
        -- in from above, inside the lines' window: the value's lines or
        -- the hint's, whichever are more (n), slide n lines and the 6
        -- between them, the window reaching n lines less 22 under the
        -- first baseline (stock's masks, `local_f8` > 1).
        local values = {}
        for k = 2, math.min(#lines, 3) do values[#values + 1] = lines[k] end
        local hints = {}
        for l in tr(lines.hint):gmatch("[^\n]+") do hints[#hints + 1] = l end
        local out, into = values, hints
        if back then out, into = into, out end
        local h = sub.size
        local n = math.max(#out, #into)
        local step, slide = h + 6, n * h + 6 * (n - 1)
        local above, below = h - self.hint_above, n * h - self.hint_below
        gfx:save()
        gfx:translate(0, base[i] - above)
        gfx:layer("ring.hint", string.format("%s\n%s\n%.3f", table.concat(out, "\n"), table.concat(into, "\n"), roll), function(g)
          g:save()
          g:alpha(1 - roll)
          for k, l in ipairs(out) do g:text(l, c, above + (k - 1) * step + slide * roll, sub) end
          g:restore()
          g:save()
          g:alpha(roll)
          for k, l in ipairs(into) do g:text(l, c, above + (k - 1) * step - slide * (1 - roll), sub) end
          g:restore()
        end, ui.face.size, above + below)
        gfx:paint_layer("ring.hint", "#ffffff")
        gfx:restore()
      elseif i == 3 and rolled then
        -- Slid with the line over it.
      else
        gfx:text(lines[i], c, base[i], i == 1 and title or i == lines.link and link or sub)
      end
    end
  end
  gfx:restore()
end

-- Moves each entry's look towards 1 for the selection, 0 for the rest.
function Ring:settle(n, selected, now)
  local chosen = state.chosen
  for i = 1, n do
    local e = chosen[i] or { v = (i == selected) and 1 or 0, at = now }
    local goal = (i == selected) and 1 or 0
    local dt = anim.clamp(now - e.at, 0, 0.1)
    e.at = now
    if e.v < goal then e.v = math.min(goal, e.v + dt / self.select)
    elseif e.v > goal then e.v = math.max(goal, e.v - dt / self.select) end
    chosen[i] = e
  end
end

-- The ring for `view`: `flip` (0 to 1) turns the symbols edge-on as the
-- menu opens or closes, `text` fades the middle, `under` is the colour
-- behind the symbols. Whether it still moves.
function Ring:draw(gfx, ui, view, flip, text, now, under)
  local c = ui.face.size / 2
  local n = #view.items
  local chosen = state.chosen
  self:settle(n, view.selected, now)
  local moving = false
  -- Mode's symbol is white in Eco (stock's 0x1a5f64).
  local t = ui.thermostat
  local mode = t.eco and t.mode ~= "off" and "eco" or t.mode
  local live = (under or "#000000") ~= "#000000"
  local function draw_icon(g, i, item, p)
    local a = self.slots[n] and self.slots[n][i] or (i - 1) * 360 / n
    local x, y = icons.polar(c, c, anim.mix(self.radius, self.radius_selected, p), a)
    g:save()
    g:translate(x, y)
    g:scale(flip, 1)
    icons.draw(g, item.icon or (item.kind == "back" and "back") or "dot", 0, 0, anim.mix(self.icon, self.icon_selected, p),
               { p = p, mode = mode, bg = under, live = live, native = self.icon, native_selected = self.icon_selected,
                 badge = badged(self, item), scale = ui.face.scale, at = { x, y } })
    g:restore()
  end
  -- The symbols at rest, in grey, are drawn once into a layer; only those
  -- selected or fading draw each frame. Turning edge-on, or over the dial,
  -- they all draw.
  local resting = flip >= 1 and not live
  local key = mode
  for i = 1, n do
    if chosen[i].v > 0 then key = key .. "," .. i end
    moving = moving or (chosen[i].v > 0 and chosen[i].v < 1)
  end
  if resting then
    for i, item in ipairs(view.items) do
      if badged(self, item) then key = key .. ",b" .. i end
    end
    gfx:layer("ring", key .. ":" .. n, function(g)
      for i, item in ipairs(view.items) do
        if chosen[i].v == 0 and not badged(self, item) then draw_icon(g, i, item, 0) end
      end
    end)
    gfx:paint_layer("ring", "#ffffff")
  end
  for i, item in ipairs(view.items) do
    -- A badge is in colour, so its symbol is never in the white layer.
    if not resting or chosen[i].v > 0 or badged(self, item) then
      draw_icon(gfx, i, item, anim.ease_in_out(chosen[i].v))
    end
  end
  -- The middle: the last selection's text fades out, then this one's in.
  local item = view.items[view.selected]
  local lines = item and self:lines(ui, item, gfx) or {}
  local centre = state.centre
  local now_key = table.concat(lines, "\n")
  if now_key ~= centre.key then
    centre.last = centre.key and centre.lines or nil
    centre.key, centre.lines, centre.since, centre.settled = now_key, lines, now, nil
  end
  -- A hint's cycle starts once the ring settles on its entry, as stock's
  -- once the wheel's scroll ends (0x1a5e0c).
  if not moving and not centre.settled then centre.settled = now end
  local age = now - centre.since
  if centre.last and age < self.fade_out then
    self:draw_centre(gfx, ui, centre.last, text * (1 - age / self.fade_out), c)
  end
  local roll, back, wait = 0, false, nil
  if lines.hint and centre.settled then roll, back, wait = self:hint_roll(now - centre.settled) end
  self:draw_centre(gfx, ui, lines, text * anim.clamp((age - self.fade_out) / self.fade_in, 0, 1), c, roll, back)
  -- A hint comes and goes for as long as its entry is selected: frames
  -- while it slides, a wake for the next slide between.
  local still = moving or age < self.fade_out + self.fade_in or wait == 0
  if not still and wait then gfx:wake(wait) end
  return still
end

return Ring
