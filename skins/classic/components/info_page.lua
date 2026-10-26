-- A page of text over the menus (More info, History's "needs more time",
-- an entry's intro), as stock's: the text set as a prompt is, its link
-- under it in blue, its button at the bottom on a pill; a top-menu
-- entry's symbol above, a deeper entry's as its list shows it; a plugin's
-- page with its picture on top. Too long for the face, it starts at the
-- top and the wheel scrolls it.
local Component = require("components.component")
local theme = require("theme")
local icons = require("icons")

local InfoPage = Component:extend({
  text = { font = theme.fonts.strong, size = 32, color = "#ffffff" }, line = 39, width = 420,
  link_color = theme.colors.selected, link_gap = 60,
  -- The text centred on `middle` (baselines `under` below the lines'
  -- middles), ending no lower than `last` (`last_link` over a link); under
  -- a top-menu symbol it ends on `symbol_end`; under a deeper one it
  -- centres on `deep_middle`. It sits `lift` higher, half a unit more
  -- over a link.
  middle = 215, under = 11, last = 341, last_link = 338, symbol_end = 273, deep_middle = 249.5, lift = 1, link_lift = 0.5,
  symbol = { y = 83, size = 71 }, deep_symbol = { y = 76.5, size = 70 },
  -- Scrolled, it starts on `top`, its button `button_gap` under the end.
  top = 83, button_gap = 67,
  -- The button: its baseline, its words, its pill (24 either side, its
  -- top on a whole pixel `rise` over the baseline).
  button = { base = 408, font = theme.fonts.label, size = 35.7, color = "#ffffff", rise = 35.7 * 0.72 + 9 },
  -- A plugin's page: its picture `picture` across from `picture_y`, the
  -- text from `picture_text` in the older style below, `picture_width` wide.
  picture = 150, picture_y = 30, picture_text = 222, picture_width = 400,
  picture_style = { font = theme.fonts.strong, size = 30, spacing = 0.9 },
})

-- `info` (text, link, button, icon, image, scrolled), opened from the top
-- menu when `top`. Whether it still moves.
function InfoPage:draw(gfx, ui, info, top)
  local alert = require("alert")
  local c = ui.face.size / 2
  local pill = self.registry().pill
  local b = self.button
  local button_style = { font = b.font, size = b.size, align = "left", color = b.color }
  gfx:clear(theme.colors.menu)
  if info.image then
    gfx:image(info.image, c - self.picture / 2, self.picture_y, self.picture, self.picture)
    local p = self.picture_style
    local style = { font = p.font, size = p.size, spacing = p.spacing, color = self.text.color, align = "center" }
    local lines = alert.wrap(gfx, info.text, style, self.picture_width)
    for i, l in ipairs(lines) do
      gfx:text(l, c, self.picture_text + (i - 1) * self.line, style)
    end
    if info.link then
      gfx:text(info.link, c, self.picture_text + #lines * self.line, { font = p.font, size = p.size, spacing = p.spacing,
                                                                     color = self.link_color, align = "center" })
    end
    local w = gfx:measure(info.button, button_style)
    pill:draw(gfx, c - w / 2, b.base - b.rise, w)
    gfx:text(info.button, c - w / 2, b.base, button_style)
    return false
  end
  local icon = top and info.icon
  if icon then icons.draw(gfx, icon, c, self.symbol.y, self.symbol.size, { p = 1 }) end
  if info.icon and not top then icons.draw(gfx, info.icon, c, self.deep_symbol.y, self.deep_symbol.size, { p = 1 }) end
  local style = { font = self.text.font, size = self.text.size, color = self.text.color, align = "center" }
  local lines = alert.wrap(gfx, info.text, style, self.width)
  local span = (#lines - 1) * self.line + (info.link and self.link_gap or 0)
  local last = info.link and self.last_link or self.last
  local first = icon and self.symbol_end - (#lines - 1) * self.line or math.min(self.middle - span / 2 + self.under, last - span)
  if info.icon and not top then first = self.deep_middle - span / 2 end
  local over = info.link and self.link_lift or 0
  first = first - self.lift
  local button = b.base
  if not icon and first < self.top - 1 then
    local room = span - (last - self.top)
    first = self.top - math.min((info.scrolled or 0) * self.line, room)
    button = first + span + self.button_gap
  end
  local text = self.registry().text:extend(self.text)
  for i, l in ipairs(lines) do
    text:draw_centred(gfx, ui, l, c, first - over + (i - 1) * self.line)
  end
  if info.link then
    gfx:text(info.link, c, first + span, { font = self.text.font, size = self.text.size, color = self.link_color, align = "center" })
  end
  local w = gfx:measure(info.button, button_style)
  pill:draw(gfx, c - w / 2, math.floor(button - b.rise - self.lift), w)
  gfx:text(info.button, c - w / 2, button, button_style)
  return false
end

return InfoPage
