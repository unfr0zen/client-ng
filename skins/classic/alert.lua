-- An alert over everything: its text in the middle and a blue arc running
-- round the rim, fading in, as stock's (matched on nest-qemu).
local anim = require("anim")
local icons = require("icons")
local theme = require("theme")

local A = {
  color = "#0092c2",
  radius = 234.5,
  width = 9,
  -- Degrees a second, clockwise, and the tail's length behind the head
  -- with its brightness along the way (degrees, alpha).
  speed = 57,
  tail = { { 0, 1 }, { 30, 0.9 }, { 60, 0.81 }, { 90, 0.62 }, { 135, 0 } },
  fade = 0.8,
  text = { size = 30, spacing = 0.9, line = 39 },
}

local M = {}

local function brightness(d)
  local t = A.tail
  for i = 2, #t do
    if d <= t[i][1] then
      local a, b = t[i - 1], t[i]
      return a[2] + (b[2] - a[2]) * (d - a[1]) / (b[1] - a[1])
    end
  end
  return 0
end

-- The text's own lines, as stock's strings break them, each further cut
-- into lines no wider than `width`; an empty one stays, as a gap.
local function wrap(gfx, text, style, width)
  -- Translated as a whole first: its lines then break where they fit.
  text = tr(text)
  local lines = {}
  for paragraph in (text .. "\n"):gmatch("([^\n]*)\n") do
    if paragraph == "" then
      lines[#lines + 1] = ""
    else
      for _, l in ipairs(M.words(gfx, paragraph, style, width)) do lines[#lines + 1] = l end
    end
  end
  return lines
end

M.wrap = wrap

local CLOSING = {}
for ch in ("，。、？！：；）」』”’》…"):gmatch("[\194-\244][\128-\191]*") do CLOSING[ch] = true end

-- Words into lines no wider than `width`; a word wider than that (Chinese
-- has no spaces) breaks between its characters.
function M.words(gfx, text, style, width)
  text = tr(text)
  local words = {}
  for word in text:gmatch("%S+") do
    if gfx:measure(word, style) <= width then
      words[#words + 1] = word
    else
      local piece = ""
      for ch in word:gmatch("[%z\1-\127\194-\244][\128-\191]*") do
        if piece ~= "" and gfx:measure(piece .. ch, style) > width then
          -- Closing punctuation never starts a line: the character before
          -- it comes down too.
          if CLOSING[ch] then
            local head, last = piece:match("^(.-)([\194-\244][\128-\191]*)$")
            if not head then head, last = piece:match("^(.-)(.)$") end
            if head and head ~= "" then piece, ch = head, last .. ch end
          end
          words[#words + 1] = piece
          piece = ch
        else
          piece = piece .. ch
        end
      end
      words[#words + 1] = piece
    end
  end
  local lines, line = {}, nil
  for _, word in ipairs(words) do
    local try = line and (line .. " " .. word) or word
    if line and gfx:measure(try, style) > width then
      lines[#lines + 1] = line
      line = word
    else
      line = try
    end
  end
  lines[#lines + 1] = line
  return lines
end


-- A page of text, a link in blue under it and Back, as stock's MORE INFO
-- (matched on nest-qemu): the text ends at the same line however long.
-- Stock's lists fade to black over the face's top and bottom 48 rows:
-- black bands, each more opaque towards the edge.
function M.fade_edges(gfx)
  for y = 0, 46, 2 do
    local a = 1 - math.min(1, math.max(0, (y + 1 - 4) / 44))
    if a > 0 then
      gfx:save()
      gfx:alpha(a)
      gfx:rect(0, y, 480, 2, "#000000")
      gfx:rect(0, 478 - y, 480, 2, "#000000")
      gfx:restore()
    end
  end
end

-- Stock's Legal Info: Google's mark, "LEGAL INFO", the text in grey,
-- scrolling, DONE after it (matched on nest-qemu).
local function legal(gfx, ui, info)
  local c = ui.face.size / 2
  gfx:clear(theme.colors.menu)
  local style = { font = theme.fonts.strong, size = A.text.size, spacing = A.text.spacing, color = "#c4c4c4", align = "center" }
  local lines = wrap(gfx, info.text, style, 420)
  local span = (#lines - 1) * A.text.line
  -- The mark on 85.5, the title's baseline 174, the text from 223, DONE
  -- 69 under the last line; the page scrolls up to put DONE on 432.
  local total = 223 + span + 69
  local room = math.max(0, total - 432)
  -- A line a turn; the last turn lands on the end however little is left.
  local scrolled = (info.scrolled or 0) * A.text.line
  if scrolled + A.text.line > room then scrolled = room end
  icons.draw(gfx, "google", c, 85.5 - scrolled, 47, { p = 1 })
  gfx:text("LEGAL INFO", c, 174 - scrolled, { font = theme.fonts.label, size = 32, color = "#ffffff", align = "center" })
  for i, l in ipairs(lines) do
    local base = 223 + (i - 1) * A.text.line - scrolled
    if base > 0 and base < 500 then gfx:text(l, c, base, style) end
  end
  local s = { font = theme.fonts.label, size = 35.7, align = "left", color = "#ffffff" }
  local w = gfx:measure(info.button, s)
  local button = total - scrolled
  gfx:rect(c - w / 2 - 24, button - 36, w + 48, 48, theme.menu.pill, 24)
  gfx:text(info.button, c - w / 2, button, s)
  M.fade_edges(gfx)
  return false
end

-- A page shown for a moment, no button: its entry's symbol, the text, and
-- the activity arc running round (stock's "checking for updates").
local function wait(gfx, ui, info)
  local c = ui.face.size / 2
  gfx:clear(theme.colors.menu)
  -- A logo page is the symbol alone in the middle (stock's Lock > ADD).
  if info.style == "logo" then
    icons.draw(gfx, info.icon, c, c, 87, { p = 1 })
    return false
  end
  -- Stock's Heat Link pairing: the state in grey on 121 and 160 over the
  -- blank Heat Link from 226, the arc running round.
  if info.style == "heatlink_connecting" then
    local style = { font = theme.fonts.strong, size = A.text.size, spacing = A.text.spacing, color = "#c4c4c4", align = "center" }
    local i = 0
    for line in info.text:gmatch("[^\n]+") do
      gfx:text(line, c, 121 + i * A.text.line, style)
      i = i + 1
    end
    gfx:image("unit:a2_heatlink_blank.png", c - 72, 226, 144, 144)
    M.arc(gfx, ui, 1)
    return true
  end
  if info.icon then icons.draw(gfx, info.icon, c, 76.5, 70, { p = 1 }) end
  local style = { font = theme.fonts.strong, size = A.text.size, spacing = A.text.spacing, color = "#ffffff", align = "center" }
  local lines = wrap(gfx, info.text, style, 420)
  -- A plugin's picture over its text (Matter's pairing code).
  if info.image then
    gfx:image(info.image, c - 80, 50, 160, 160)
    for i, l in ipairs(lines) do
      gfx:text(l, c, 260 + (i - 1) * A.text.line, style)
    end
    M.arc(gfx, ui, 1)
    return true
  end
  for i, l in ipairs(lines) do
    gfx:text(l, c, 252 - (#lines - 1) * A.text.line + (i - 1) * A.text.line, style)
  end
  M.arc(gfx, ui, 1)
  return true
end

-- A page of text over the menus: the InfoPage component.
local function page(gfx, ui, info, top)
  return require("components").info_page:draw(gfx, ui, info, top)
end

function M.info(gfx, ui, info, top)
  if info.style == "legal" then return legal(gfx, ui, info) end
  if info.wait then return wait(gfx, ui, info) end
  local animating = page(gfx, ui, info, top)
  -- A plugin's page while its work runs: the activity arc round it.
  if info.busy then
    M.arc(gfx, ui, 1)
    return true
  end
  return animating
end

-- An HVAC alert (one answer, or a link) as stock's nlAlert lays it out (0x225790, its
-- lines placed by 0x418254): the glyph on 77 (72 high, on its baseline
-- 113), a heading's lines 39 apart under that baseline, the text as a block under them with its link, the answer's
-- pill on 407. A block too tall for the face runs on, the answer 32 under
-- its end, and the page turns down a line a detent (`d.scrolled`) until
-- the answer is on 407.
local function single(gfx, ui, d)
  local c = ui.face.size / 2
  local style = { font = theme.fonts.strong, size = A.text.size, spacing = A.text.spacing, color = "#ffffff", align = "center" }
  local heads = d.title and M.words(gfx, d.title, style, 408) or {}
  local lines, first = M.block(gfx, d.text, style, d.link, 159 + #heads * A.text.line)
  local last = first + (#lines - 1) * A.text.line + (d.link and 61 or 0)
  local button = math.max(407, last + 32 + 35)
  -- More answers follow 57 apart; the selected one is kept on 407 at
  -- most (unsure: stock's frames with two unseen).
  local final = button + (#d.options - 1) * 57
  local scrolled = math.min((d.scrolled or 0) * A.text.line, final - 407)
  scrolled = math.max(scrolled, button + ((d.selected or 1) - 1) * 57 - 407)
  local y = function(base) return base - scrolled end
  icons.draw(gfx, d.icon, c, y(77), 70, { p = 1 })
  for i, l in ipairs(heads) do
    gfx:text(l, c, y(113 + i * A.text.line), style)
  end
  for i, l in ipairs(lines) do
    gfx:text(l, c, y(first + (i - 1) * A.text.line), style)
  end
  if d.link then
    gfx:text(d.link, c, y(last), { font = theme.fonts.strong, size = A.text.size, spacing = A.text.spacing, color = theme.menu.link, align = "center" })
  end
  for i, option in ipairs(d.options) do
    local on = i == (d.selected or 1)
    local pill = { font = theme.fonts.label, size = 35.7, align = "left", color = on and "#ffffff" or theme.menu.option }
    local o = tr(option)
    local w = gfx:measure(o, pill)
    local at = button + (i - 1) * 57
    if on then gfx:rect(c - w / 2 - 24, y(at - 35), w + 48, 48, theme.menu.pill, 24) end
    gfx:text(o, c - w / 2, y(at), pill)
  end
  return false
end

-- A question and its answers, as stock's "stop using Eco" (matched on
-- nest-qemu): the text above, the answers in a column, the selected one
-- on a blue pill; the answer in force ticked, and a column longer than
-- the face scrolled to keep the selected one on 408 at most.
function M.dialog(gfx, ui)
  local c = ui.face.size / 2
  local d = ui.dialog
  gfx:clear(theme.colors.menu)
  if d.icon and (#d.options == 1 or d.link) then return single(gfx, ui, d) end
  local style = { font = theme.fonts.strong, size = A.text.size, spacing = A.text.spacing, color = "#ffffff", align = "center" }
  local lines = wrap(gfx, d.text, style, 420)
  local ys = #d.options > 2 and {} or { 294, 408 }
  local dy = 0
  if d.icon then
    -- Stock's Wi-Fi alerts: the symbol on top, the text about the
    -- middle, the answers close under it.
    icons.draw(gfx, d.icon, c, 80, 70, { color = "#ffffff" })
    for i, l in ipairs(lines) do
      gfx:text(l, c, 230 - (#lines - 1) * A.text.line / 2 + (i - 1) * A.text.line, style)
    end
    ys = { 360, 408 }
  else
    dy = math.max(0, (ys[d.selected] or (294 + (d.selected - 1) * 57)) - 408)
    for i, l in ipairs(lines) do
      gfx:text(l, c, 131 + (i - 1) * A.text.line - dy, style)
    end
  end
  for i, o in ipairs(d.options) do
    local s = { font = theme.fonts.label, size = 35.7, align = "left" }
    o = tr(o)
    if i == d.checked then o = "\u{2713} " .. o end
    local w = gfx:measure(o, s)
    local y = (ys[i] or (294 + (i - 1) * 57)) - dy
    if i == d.selected then gfx:rect(c - w / 2 - 24, y - 35.7 * 0.72 - 9, w + 48, 48, theme.menu.pill, 24) end
    s.color = i == d.selected and "#ffffff" or theme.menu.option
    gfx:text(o, c - w / 2, y, s)
  end
  return false
end

-- A System Test page (stock's systemtestlayer pages, laid out by its
-- layers' text page, 0x417c78): the heading's lines, the text wrapped
-- under them, a link 61 under its last line, then the choices 57 apart,
-- the last on 408 when they fit, those tested ticked. The text sits
-- midway in the room over the choices; a page taller than the face
-- scrolls to keep the selected choice on 408 at most (unsure: stock's
-- frames of these pages unseen).
function M.test(gfx, ui)
  local c = ui.face.size / 2
  local d = ui.test
  gfx:clear(theme.colors.menu)
  local style = { font = theme.fonts.strong, size = A.text.size, spacing = A.text.spacing, color = "#ffffff", align = "center" }
  local lines = {}
  for _, h in ipairs(d.heading) do
    for _, l in ipairs(M.words(gfx, h, style, 408)) do lines[#lines + 1] = l end
  end
  if #d.heading > 0 and d.text ~= "" then lines[#lines + 1] = "" end
  if d.text ~= "" then
    for _, l in ipairs(wrap(gfx, d.text, style, 408)) do lines[#lines + 1] = l end
  end
  local n = #d.options
  local span = math.max(#lines - 1, 0) * A.text.line + (d.link and 61 or 0)
  local first = math.max(408 - (n - 1) * 57, 98 + span + 69)
  local top = math.max(98, math.floor((60 + first - 69 - span) / 2))
  local dy = math.max(0, first + ((d.selected or 1) - 1) * 57 - 408)
  for i, l in ipairs(lines) do
    gfx:text(l, c, top + (i - 1) * A.text.line - dy, style)
  end
  if d.link then
    gfx:text(d.link, c, top + span - dy, { font = theme.fonts.strong, size = A.text.size, spacing = A.text.spacing, color = theme.menu.link, align = "center" })
  end
  for i, o in ipairs(d.options) do
    local s = { font = theme.fonts.label, size = 35.7, align = "left" }
    if d.ticked[i] then o = "\u{2713} " .. o end
    local w = gfx:measure(o, s)
    local y = first + (i - 1) * 57 - dy
    if i == d.selected then gfx:rect(c - w / 2 - 24, y - 35.7 * 0.72 - 9, w + 48, 48, theme.menu.pill, 24) end
    s.color = i == d.selected and "#ffffff" or theme.menu.option
    gfx:text(o, c - w / 2, y, s)
  end
  return false
end

-- A text block's lines as stock's lists lay them out on the round face
-- (0x4197b0), under a glyph line on 77 and over a button on 407, as in
-- alerts and notices (0x225790) and Farsight's weather failure (0x143fe4):
-- its lines 39 apart, a link (`link`) 61 under the last, the first set by
-- 0x418254 from three bounds: under the glyph line (159, or `top` under
-- more header lines), centred about 249.5, 32 over the button's top
-- (372); a block too tall for them starts on the first. A line is at
-- most 408 wide, or the face's width at its top less 24 where that is less
-- (0x418ac8); the words break where the squares of the lines' slack, the
-- first's and last's at 0.8, add up least (0x418b8c), again as the lines'
-- places move. `place`, when given, sets the first baseline for a count
-- of lines instead. Returns the lines and the first's baseline.
function M.block(gfx, text, style, link, top, place)
  top = top or 159
  local words = {}
  for w in tr(text):gmatch("%S+") do words[#words + 1] = w end
  place = place or function(n)
    local span = (n - 1) * 39 + (link and 61 or 0)
    local last = 340 - span
    if last <= top then return top end
    return ((top + last) / 2 + math.max(math.min(249.5 - span / 2, last), top)) / 2
  end
  local function limit(y)
    -- Past the button's bound the page is read turned up: full width
    -- (unsure: stock's widths there).
    if y > 340 then return 408 end
    local row = y - 19
    if row < 0 or row >= 240 then row = 480 - y end
    if row < 0 or row >= 240 then return 408 end
    return math.min(408, math.max(math.floor(2 * math.sqrt(240 ^ 2 - (row - 240) ^ 2)) - 24, 0))
  end
  local function breaks(limits)
    -- best[k]: the first k words' least cost, their lines and the last break.
    local best = { [0] = { cost = 0, lines = 0 } }
    for k = 1, #words do
      for j = k - 1, 0, -1 do
        local from = best[j]
        local l = limits[from.lines + 1] or 408
        local w = gfx:measure(table.concat(words, " ", j + 1, k), style)
        if w > l and k > j + 1 then break end
        local cost = (l - w) ^ 2
        if from.lines == 0 or k == #words then cost = cost * 0.8 end
        cost = cost + from.cost
        if not best[k] or cost < best[k].cost then best[k] = { cost = cost, lines = from.lines + 1, from = j } end
      end
    end
    local lines, k = {}, #words
    while k > 0 do
      table.insert(lines, 1, table.concat(words, " ", best[k].from + 1, k))
      k = best[k].from
    end
    return lines
  end
  local lines = breaks({})
  for _ = 1, 4 do
    local first, limits = place(#lines), {}
    for i = 1, #lines + 1 do limits[i] = limit(first + (i - 1) * 39) end
    local n = #lines
    lines = breaks(limits)
    if #lines == n then break end
  end
  return lines, place(#lines)
end

-- Stock's list layout (0x418254) for rows under no glyph line, as the
-- fan's pages lay theirs out: rows `span` from the first baseline to the
-- last (39 a line, 61 to a row of another text), `under` buttons straight
-- under them and `foot` buttons at the bottom, 48 apart. The rows start
-- 79 down (60 and the first line's 19) and end 32 over the buttons under
-- them, which sit midway between the rows and the foot's top (48 a button
-- up from 420), or the foot comes 32 under them where they do not fit;
-- without buttons under them the rows end 32 over the foot. The first
-- baseline then lies between the top and as low as they go, centred
-- about 240 where that is higher. Returns the first baseline, the top of
-- the buttons under the rows and of the foot's.
function M.list(span, under, foot)
  local top = 79
  local rows_end = top + span + 32
  local foot_top = 420
  local under_top
  if under > 0 then
    if rows_end < foot_top - (foot * 48 + under * 48 + 32) then
      foot_top = foot_top - foot * 48
      local mid = math.floor((rows_end + foot_top - under * 48) / 2)
      if mid + under * 24 + 32 < foot_top then under_top = mid else under_top = rows_end end
    else
      under_top = rows_end
      foot_top = rows_end + under * 48 + 32
    end
  else
    foot_top = math.max(foot_top - foot * 48, rows_end)
    under_top = foot_top
  end
  local last = under_top - 32 - span
  local high = math.max(math.min(240 - math.floor(span / 2), last), top)
  return math.floor((math.floor((top + last) / 2) + high) / 2), under_top, foot_top
end

-- A notice with CONTINUE: the symbol on 77, the text as a block of
-- stock's lists, the button on 407 (matched with stock's "The wiring to
-- your equipment has changed.").
function M.notice(gfx, ui)
  local c = ui.face.size / 2
  gfx:clear(theme.colors.menu)
  icons.draw(gfx, "tools", c, 77, 70, { p = 1 })
  local style = { font = theme.fonts.strong, size = A.text.size, spacing = A.text.spacing, color = "#ffffff", align = "center" }
  local lines, first = M.block(gfx, ui.notice, style)
  for i, l in ipairs(lines) do
    gfx:text(l, c, first + (i - 1) * A.text.line, style)
  end
  local pill = { font = theme.fonts.label, size = 35.7, align = "left", color = "#ffffff" }
  local w = gfx:measure("CONTINUE", pill)
  gfx:rect(c - w / 2 - 24, 372, w + 48, 48, theme.menu.pill, 24)
  gfx:text("CONTINUE", c - w / 2, 407, pill)
  return false
end

-- Stock's error page: the code in an orange badge on 72.5, the title on
-- 142, the text in grey, the link in blue 61 under its last line and the
-- choices 48 apart under it, the one highlighted once the wheel turns in
-- a pill (matched with stock's H72 and H71). The text wraps at 420 and
-- "Learn more at" joins its last line unwrapped. A page taller than the
-- face is laid out tighter (text wrapped at 360 from 192, choices 56.5
-- under the link rather than 208 and 88) and scrolls to keep the
-- highlight on 396.
function M.error(gfx, ui)
  local c = ui.face.size / 2
  local e = ui.error
  gfx:clear(theme.colors.menu)
  local style = { font = theme.fonts.strong, size = A.text.size, spacing = A.text.spacing, color = "#c4c4c4", align = "center" }
  local lines
  local function layout(width, text_y, gap)
    -- Stock's string is the text with "Learn more at" at its end: another
    -- language's wraps whole.
    local whole = e.text .. " Learn more at"
    if tr(whole) ~= whole then
      lines = wrap(gfx, whole, style, width)
    else
      lines = wrap(gfx, e.text, style, width)
      lines[#lines] = lines[#lines] .. " Learn more at"
      -- Too wide joined: the whole wraps.
      if gfx:measure(lines[#lines], style) > width then lines = wrap(gfx, whole, style, width) end
    end
    local link = text_y + (#lines - 1) * A.text.line + 61
    return text_y, link, link + gap
  end
  local text_y, link_y, first = layout(420, 208, 88)
  if first + (#e.choices - 1) * 48 + 24 > ui.face.size then
    text_y, link_y, first = layout(360, 192, 56.5)
  end
  local dy = 0
  if e.selected then
    local last = first + (#e.choices - 1) * 48
    dy = math.max(0, math.min(first + (e.selected - 1) * 48, last) - 396)
  end
  gfx:circle(c, 72.5 - dy, 37.5, "#e74400")
  gfx:text(e.code, c, 81 - dy, { font = theme.fonts.label, size = 27.4, color = "#ffffff", align = "center" })
  gfx:text(e.title, c, 142 - dy, { font = theme.fonts.label, size = 29, color = "#ffffff", align = "center" })
  for i, l in ipairs(lines) do
    gfx:text(l, c, text_y + (i - 1) * A.text.line - dy, style)
  end
  gfx:text(e.link, c, link_y - dy, { font = theme.fonts.strong, size = A.text.size, spacing = A.text.spacing, color = "#00b7f4", align = "center" })
  for i, choice in ipairs(e.choices) do
    local y = first + (i - 1) * 48 - dy
    local pill = { font = theme.fonts.label, size = 35.7, align = "left" }
    local w = gfx:measure(choice, pill)
    local on = e.selected == i
    if on then gfx:rect(c - w / 2 - 24, y - 24, w + 48, 48, theme.menu.pill, 24) end
    pill.color = on and "#ffffff" or "#4b4b4b"
    gfx:text(choice, c - w / 2, y + 11, pill)
  end
  return false
end

-- `t`: seconds since the alert came up.
-- The activity arc round the rim, `fade` (0 to 1) bright.
function M.arc(gfx, ui, fade)
  local c = ui.face.size / 2
  local alpha = function(a) return string.format("%02x", math.floor(a * fade * 255 + 0.5)) end
  local head = (ui.now * A.speed) % 360
  for d = 0, A.tail[#A.tail][1] - 3, 3 do
    gfx:arc(c, c, A.radius, head - d - 3, head - d, A.width, A.color .. alpha(brightness(d + 1.5)))
  end
  local rad = math.rad(head)
  gfx:circle(c + A.radius * math.sin(rad), c - A.radius * math.cos(rad), A.width / 2, A.color .. alpha(1))
end

function M.draw(gfx, ui, t)
  local c = ui.face.size / 2
  local fade = anim.clamp(t / A.fade, 0, 1)
  local alpha = function(a) return string.format("%02x", math.floor(a * fade * 255 + 0.5)) end
  gfx:clear(theme.colors.menu)
  M.arc(gfx, ui, fade)
  local style = { font = theme.fonts.strong, size = A.text.size, spacing = A.text.spacing, color = "#ffffff" .. alpha(1), align = "center" }
  local lines = wrap(gfx, ui.alert, style, 420)
  local base = c - (#lines - 1) * A.text.line / 2 + A.text.size * 0.53
  for i, l in ipairs(lines) do
    gfx:text(l, c, base + (i - 1) * A.text.line, style)
  end
  return true
end

return M
