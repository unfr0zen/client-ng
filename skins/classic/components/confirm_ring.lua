-- Stock's confirm page (Reset > Restart): the fan dial's track round the
-- face filling as the wheel turns it, the prompt over the middle and the
-- way out on its pill at the bottom.
local Component = require("components.component")
local theme = require("theme")
local icons = require("icons")
local anim = require("anim")

local M = theme.menu
local ConfirmRing = Component:extend({
  radius = M.dial_radius, width = M.dial_width, angle = M.dial_angle,
  track = M.dial_track, fill = M.pill,
  prompt_middle = 253, prompt_step = M.prompt_step, base = 420, pill_top = 384,
})
ConfirmRing.follow = anim.follower(0.08)

-- Turned `turned` (0 to 1); whether its fill still moves.
function ConfirmRing:draw(gfx, ui, prompt, label, turned, now)
  local c = ui.face.size / 2
  local a0, a1, r, w = -self.angle, self.angle, self.radius, self.width
  gfx:arc(c, c, r, a0, a1, w, self.track)
  local sx, sy = icons.polar(c, c, r, a0)
  local ex1, ey1 = icons.polar(c, c, r, a1)
  gfx:circle(sx, sy, w / 2, self.track)
  gfx:circle(ex1, ey1, w / 2, self.track)
  local shown, moving = self.follow(turned, now)
  if shown > 0.001 then
    local a = a0 + (a1 - a0) * shown
    gfx:circle(sx, sy, w / 2, self.fill)
    gfx:arc(c, c, r, a0, a, w, self.fill)
    local ex, ey = icons.polar(c, c, r, a)
    gfx:circle(ex, ey, w / 2, self.fill)
  end
  local lines = {}
  for line in (prompt or ""):gmatch("[^\n]+") do lines[#lines + 1] = line end
  for i, line in ipairs(lines) do
    gfx:text(line, c, self.prompt_middle + (i - (#lines + 1) / 2) * self.prompt_step,
             { font = theme.fonts.label, size = 32, color = "#ffffff", align = "center" })
  end
  local parts = self.registry()
  local button = (self.button or parts.button):extend({ rise = self.base - self.pill_top })
  button:draw(gfx, ui, label, c, self.base, true)
  return moving
end

return ConfirmRing
