-- Stock's fan timer: an arc round the face filling with the value, the
-- value and a hint under the entry's symbol, BACK at the bottom (on its
-- pill while nothing is set). While the fan runs, the arc shows in white
-- what is left of the run, over STOP FAN and BACK.
local Component = require("components.component")
local theme = require("theme")
local icons = require("icons")
local anim = require("anim")

local M = theme.menu
local Dial = Component:extend({
  radius = M.dial_radius, width = M.dial_width, angle = M.dial_angle, track = M.dial_track, fill = M.pill,
  running = "#ffffff",
  icon_y = 171.5, icon_size = 72,
  value = { font = theme.fonts.label, size = 35.7, color = "#ffffff", y = 252 },
  -- A fan of more than one speed running: its sentence's lines.
  sentence = { font = theme.fonts.label, size = 32, color = "#ffffff", line = 39 },
  hint = { font = theme.fonts.label, size = 28, y = 295 }, unset = "Turn to set", set = "Press to start",
  back = "BACK", back_base = 420,
  -- Running: STOP FAN and BACK, their pills `pad` either side.
  stop = "STOP FAN", stop_base = 405, run_back_base = 452, pad = 18,
})
Dial.follow = anim.follower(0.08)

-- The arc's track, and `fraction` of it from its start in `color`.
function Dial:draw_arc(gfx, c, fraction, color, start)
  local a0, a1, r, w = -self.angle, self.angle, self.radius, self.width
  gfx:arc(c, c, r, a0, a1, w, self.track)
  local sx, sy = icons.polar(c, c, r, a0)
  local ex1, ey1 = icons.polar(c, c, r, a1)
  gfx:circle(sx, sy, w / 2, start)
  gfx:circle(ex1, ey1, w / 2, self.track)
  if fraction > 0.001 then
    local a = a0 + (a1 - a0) * fraction
    gfx:arc(c, c, r, a0, a, w, color)
    local ex, ey = icons.polar(c, c, r, a)
    gfx:circle(ex, ey, w / 2, color)
  end
end

-- Setting the timer: `fraction` of the arc, `label`, `unset` while
-- nothing is set. Whether the arc still moves.
function Dial:draw(gfx, ui, icon, fraction, label, unset, now)
  local c = ui.face.size / 2
  local shown, moving = self.follow(fraction, now)
  -- Its start fills as soon as there is a value.
  self:draw_arc(gfx, c, shown, self.fill, fraction > 0 and self.fill or self.track)
  if icon then icons.draw(gfx, icon, c, self.icon_y, self.icon_size, { color = "#ffffff" }) end
  local v = self.value
  gfx:text(label, c, v.y, { font = v.font, size = v.size, color = v.color, align = "center" })
  gfx:text(unset and self.unset or self.set, c, self.hint.y, { font = self.hint.font, size = self.hint.size, color = self.track, align = "center" })
  self.registry().button:draw(gfx, ui, self.back, c, self.back_base, unset)
  return moving
end

-- A fan of more than one speed running: its sentence, STOP FAN under it
-- and BACK at the foot, as stock's list layout places them (fanlayer
-- state 5, table 0x706568), STOP FAN or BACK highlighted.
function Dial:draw_sentence(gfx, ui, left, stop)
  local c = ui.face.size / 2
  local alert = require("alert")
  local s = self.sentence
  local style = { font = s.font, size = s.size, color = s.color, align = "center" }
  local function place(n) return (alert.list((n - 1) * s.line, 1, 1)) end
  local lines, first = alert.block(gfx, left, style, nil, nil, place)
  for i, l in ipairs(lines) do
    gfx:text(l, c, first + (i - 1) * s.line, style)
  end
  local _, stop_top, back_top = alert.list((#lines - 1) * s.line, 1, 1)
  local button = self.registry().button
  button:draw(gfx, ui, self.stop, c, stop_top + button.rise, stop)
  button:draw(gfx, ui, self.back, c, back_top + button.rise, not stop)
end

-- The fan running: `fraction` of the arc left, `left` (its time) under
-- the symbol, STOP FAN or BACK highlighted; with `sentence`, the page of
-- a fan of more than one speed instead.
function Dial:draw_running(gfx, ui, icon, fraction, left, stop, sentence)
  if sentence then return self:draw_sentence(gfx, ui, left, stop) end
  local c = ui.face.size / 2
  self:draw_arc(gfx, c, fraction, self.running, self.running)
  if icon then icons.draw(gfx, icon, c, self.icon_y, self.icon_size, { color = "#ffffff" }) end
  local v = self.value
  gfx:text(left, c, v.y, { font = v.font, size = v.size, color = v.color, align = "center" })
  local parts = self.registry()
  local button = parts.button:extend({ snap = false, rise = 35.7 * 0.729 + 9, pill = parts.pill:extend({ left = self.pad, right = self.pad }) })
  button:draw(gfx, ui, self.stop, c, self.stop_base, stop)
  button:draw(gfx, ui, self.back, c, self.run_back_base, not stop)
end

return Dial
