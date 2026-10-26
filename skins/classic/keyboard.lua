-- Stock's keyboard: the Keyboard component, `t` seconds after it opened.
local M = {}

function M.draw(gfx, ui, t)
  return require("components").keyboard:draw(gfx, ui, t)
end

return M
