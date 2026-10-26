-- Stock's Wi-Fi pages (connectlayer, networktypelayer): the phase line
-- over the network's name in a blue pill, the activity arc while
-- connecting, STOP CONNECTING under it once the wheel turns to it; the
-- online page (a tick in the pill), the failed one (a cross), the hidden
-- network's security question, and the disconnecting page. Positions
-- measured on stock (nest-qemu).
local alert = require("alert")
local icons = require("icons")
local theme = require("theme")

local M = {}
local F = theme.fonts
local C = 240
local BLUE = "#00a3d9"
local TEXT = { font = F.strong, size = 31, color = "#ffffff", align = "center" }
local NAME = { font = F.strong, size = 34, color = "#ffffff" }
local CHOICE = { font = F.strong, size = 31, color = "#5a5a5a", align = "center" }
local GREY = "#8c8c8c"

local function lines(gfx, text, y)
  -- Translated whole first, then broken where it fits.
  local out = {}
  for part in (tr(text) .. "\n"):gmatch("([^\n]*)\n") do
    for _, l in ipairs(alert.words(gfx, part, TEXT, 400)) do out[#out + 1] = l end
  end
  for i, l in ipairs(out) do gfx:text(l, C, y + (i - 1) * 39, TEXT) end
  return y + (#out - 1) * 39
end

-- The name in its pill, after a mark ("tick", "cross") when given.
local function pill(gfx, name, mark)
  local w = gfx:measure(name, NAME) + (mark and 34 or 0)
  gfx:rect(C - w / 2 - 22, 204, w + 44, 48, BLUE, 24)
  local x = C - w / 2
  if mark then
    gfx:circle(x + 11, 228, 11, "#ffffff")
    if mark == "tick" then
      gfx:lines({ x + 5, 228, x + 9, 233, x + 9, 233, x + 17, 223 }, 3, BLUE)
    else
      gfx:lines({ x + 6, 223, x + 16, 233, x + 6, 233, x + 16, 223 }, 3, BLUE)
    end
    x = x + 34
  end
  gfx:text(name, x, 240, NAME)
end

function M.draw(gfx, ui)
  local w = ui.wifi
  gfx:clear("#000000")
  if w.page == "connecting" then
    alert.arc(gfx, ui, 1)
    gfx:text(w.text, C, 153, TEXT)
    pill(gfx, w.name)
    gfx:text("STOP CONNECTING", C, 280, { font = F.strong, size = 31, color = w.stop and "#ffffff" or GREY, align = "center" })
    return true
  elseif w.page == "disconnecting" then
    alert.arc(gfx, ui, 1)
    lines(gfx, w.text, 220)
    return true
  elseif w.page == "security" then
    gfx:text(w.text, C, 166, TEXT)
    for i, c in ipairs(w.choices) do
      local y = 252 + (i - 1) * 48
      if i == w.at then
        local cw = gfx:measure(c, CHOICE)
        gfx:rect(C - cw / 2 - 22, y - 34, cw + 44, 46, BLUE, 23)
        gfx:text(c, C, y, { font = F.strong, size = 31, color = "#ffffff", align = "center" })
      else
        gfx:text(c, C, y, CHOICE)
      end
    end
    return false
  end
  -- Online or failed: the text, the name marked, the help link, the hint.
  lines(gfx, w.text, 153)
  pill(gfx, w.name, w.page == "online" and "tick" or "cross")
  local y = 299
  if w.help ~= "" then
    gfx:text(w.help, C, y, { font = F.label, size = 27, color = GREY, align = "center" })
    y = y + 40
  end
  gfx:text(w.hint, C, y, TEXT)
  return false
end

return M
