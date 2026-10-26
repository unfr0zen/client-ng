-- Stock's postal-code entry (nlLocationLayer): the question, a row of
-- boxes, one per character, `-` until set; the box being set in blue,
-- then DONE in its pill. Positions measured on stock (nest-qemu).
local theme = require("theme")

local M = {}
local F = theme.fonts
local C = 240
local W, H, PITCH, GAP = 51, 43, 60, 12
local GREY = "#666666"

function M.draw(gfx, ui)
  local p = ui.postal
  gfx:clear("#000000")
  gfx:text(p.prompt, C, 195, { font = F.strong, size = 32, color = "#ffffff", align = "center" })
  local n = #p.chars
  local width = (n - 1) * PITCH + W + (p.gap and GAP or 0)
  local x = C - width / 2
  for i = 1, n do
    if p.gap and i == p.gap + 1 then x = x + GAP end
    local current = i == p.at
    if current then
      -- The box being set stands taller, shaded in from its edge.
      gfx:rect(x, 219, W, 51, "#00a3d9", 8)
      gfx:rect(x + 2, 221, W - 4, 47, "#002734", 6)
      gfx:rect(x + 5, 224, W - 10, 41, "#004157", 4)
    else
      gfx:rect(x, 223, W, H, GREY, 8)
      gfx:rect(x + 2, 225, W - 4, H - 4, "#000000", 6)
    end
    local color = current and "#ffffff" or (p.set[i] and GREY or "#424242")
    gfx:text(p.chars[i], x + W / 2, 255, { font = F.strong, size = 31, color = color, align = "center" })
    x = x + PITCH
  end
  local done = p.at > n
  local style = { font = F.strong, size = 36, color = done and "#ffffff" or GREY, align = "center" }
  if done then
    local w = gfx:measure("DONE", style)
    gfx:rect(C - w / 2 - 26, 372, w + 52, 48, "#00a3d9", 24)
  end
  gfx:text("DONE", C, 407, style)
  return false
end

return M
