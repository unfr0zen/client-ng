-- Stock's list screen: a symbol on top (`top.icon`, coloured for
-- `top.mode`), a prompt under it or else a title, then the entries, the
-- set one ticked and the highlighted one on a pill (ListRow), the lines
-- over them (Prompt).
--
-- Where things go depends on the page: each kind of page is a layout in
-- `List.layouts`, chosen in `List.order` by the page's flags (`top.boiler`,
-- `top.picks`, ...). A layout sets the prompt's lines and returns where
-- the rows start; a variant may change a layout's numbers (each layout's
-- own table of them, `List.picks` and so on) or replace a layout.
local Component = require("components.component")
local theme = require("theme")
local icons = require("icons")
local alert = require("alert")

local M = theme.menu
local List = Component:extend({
  -- Rows: where they start by default, their step, the further gap before
  -- Back; the last entry (Done, Back) ends on `bottom`, a named page's on
  -- `titled_bottom`, at least `last_gap` under the row before it.
  first = M.choice_top, row = M.choice_step, back_gap = M.choice_back_gap,
  bottom = 408, titled_bottom = 407, last_gap = 80,
  -- The prompt: its first baseline, its step, the gap to the rows; wider
  -- than `wrap`, a line wraps at `wrap_width`.
  prompt_top = M.prompt_top, prompt_step = M.prompt_step, prompt_gap = M.prompt_gap,
  prompt_style = { font = theme.fonts.label, size = 32 },
  wrap = 420, wrap_width = 400,
  -- The symbol on top, and a title without one.
  icon_y = 76.25, icon_size = 74.2,
  title_y = 90, title = { font = theme.fonts.label, size = 35.7, color = M.title },
  -- An open section (Technical Info's): a label `first` under its heading,
  -- its values `step` apart, the next label `apart` on, the next heading
  -- `next` under the last value, in these colours; its heading rises to
  -- `rise` when opened.
  section = { first = 67, step = 39, apart = 58, next = 77, label = "#ffffff", value = "#c4c4c4", rise = 95 },
  -- Scrolling: a list longer than `fits` starts at its top, its prompt's
  -- first line on `top_line`, and keeps the entry after the highlighted
  -- one above `keep` (`keep_long` once scrolled, `keep_picks` for picks);
  -- its last entry ends on `last_end` with the wheel at its end.
  fits = 450, top_line = 82, keep = 420, keep_long = 407, keep_picks = 404, last_end = 407,
  -- A question page too long for the face moves with the ring itself,
  -- `step` units a step, until its last entry sits on `finish`; the ring's
  -- place counts on `slack` steps past that end (stock's Early-On, True
  -- Radiant).
  ring = { step = 13.8, finish = 396, slack = 1 },
  -- Picks too long for the face keep the highlighted row no lower than
  -- `keep`, without a prompt `bare`, never past `last` from the end.
  long_picks = { keep = 214, bare = 407, last = 412 },
  -- Rows are drawn between these baselines.
  above = 0, below = 510,
})

-- Each page's place on the ring, from where it opened.
local ring_pos = {}

function List:ring_offset(ui, top, overflow)
  local travel = ui.menu and ui.menu.travel or 0
  local page = tostring(ui.menu and ui.menu.page or 0) .. "|" .. (top.title or "") .. "|" .. (top.prompt or "")
  local p = ring_pos[page]
  if not p then
    -- A new page: the others are gone.
    if ring_pos.last ~= (ui.menu and ui.menu.page) then ring_pos = { last = ui.menu and ui.menu.page } end
    p = { travel = travel, at = 0 }
    ring_pos[page] = p
  end
  local most = math.ceil(overflow / self.ring.step) + self.ring.slack
  p.at = math.max(0, math.min(most, p.at + travel - p.travel))
  p.travel = travel
  return math.min(p.at * self.ring.step, overflow)
end

-- The lines of `text`, one per line.
local function split(text)
  local out = {}
  for line in (text or ""):gmatch("[^\n]+") do out[#out + 1] = line end
  return out
end

-- The rows that are not the last entry.
local function count_rows(lines)
  local n = 0
  for _, l in ipairs(lines) do
    if not (type(l) == "table" and l.last) then n = n + 1 end
  end
  return n
end

-- The layouts. Each fills `L.prompt` with { text, baseline, colour, size }
-- lines and sets `L.first` (the first row's baseline), and may set
-- `L.icon_y` and `L.image_top`.
List.layouts = {}
local layouts = List.layouts

-- Pro Setup's heating types: the question `over` over the six types,
-- which start on `first` and rise `rise` for each row the selection is
-- down (matched on nest-qemu).
List.boiler = { top = 74, first = 180, rise = 6.5, start = 16.5 }
function layouts.boiler(self, L, gfx, ui, top, lines, highlighted)
  local k = self.boiler
  local rise = (highlighted - 1) * k.rise - k.start
  for n, line in ipairs(split(top.prompt)) do
    L.prompt[#L.prompt + 1] = { line, k.top - rise + (n - 1) * self.prompt_step }
  end
  L.first = k.first - rise
end

-- A plugin's page: its picture, the prompt under it, then the entries,
-- the whole centred on the face: the picture to the first baseline
-- `under`, the last line to the first entry `gap`, the last entry's
-- descent `descent`.
List.picture = { size = 140, under = 46, gap = 66, descent = 12 }
function layouts.picture(self, L, gfx, ui, top, lines)
  local k, c = self.picture, ui.face.size / 2
  local texts = {}
  for part in ((top.prompt or "") .. "\n"):gmatch("([^\n]*)\n") do
    for _, line in ipairs(part == "" and { "" } or alert.words(gfx, part, self.prompt_style, self.wrap_width)) do texts[#texts + 1] = line end
  end
  local h = top.image_h or k.size
  local height = h + k.under + (#texts - 1) * self.prompt_step + k.gap + (#lines - 1) * self.row + k.descent
  L.image_top = math.floor(c - height / 2)
  local base = L.image_top + h + k.under
  for _, line in ipairs(texts) do
    L.prompt[#L.prompt + 1] = { line, base, top.image == "unit:G_unfr0zen_192px.png" and line == "@will-tm" and "#00bfff" or nil }
    base = base + self.prompt_step
  end
  L.first = base - self.prompt_step + k.gap
end

-- Picks: the rows' baselines end at the bottom when they fit (`last_question`
-- for a question's, `last` for plain picks, stock's regions), the prompt a
-- gap over them: a question's `question` over five rows, `per_row` more
-- for each row fewer (stock's Construction), plain picks' `gap`. Too many,
-- the prompt's first line is on `top` and the rows `gap` under it; without
-- a prompt the rows start on `bare` (stock's Language).
List.picks = { question = 75, per_row = 12, rows = 5, gap = 68, last_question = 407, last = 374, top = 82, bare = 77 }
function layouts.picks(self, L, gfx, ui, top, lines)
  local k = self.picks
  local n, rows = #split(top.prompt), count_rows(lines)
  local gap = top.question and k.question + k.per_row * math.max(0, k.rows - rows) or k.gap
  local last = top.question and k.last_question or k.last
  if k.top + (n - 1) * self.prompt_step + gap > last - (rows - 1) * self.row then gap = k.gap end
  local first_row = math.max(k.top + (n - 1) * self.prompt_step + gap, last - (rows - 1) * self.row)
  if n == 0 then first_row = k.bare end
  local base = first_row - gap - (n - 1) * self.prompt_step
  for _, line in ipairs(split(top.prompt)) do
    L.prompt[#L.prompt + 1] = { line, base }
    base = base + self.prompt_step
  end
  L.first = base - self.prompt_step + gap
end

-- Stock's Network: the networks, then the choices `apart` under the last
-- of one, `nearer` nearer for each further network but no nearer than
-- `closest`, the whole centred on `middle` and starting no higher than
-- `highest` (three networks from 150, two choices from 350, on nest-qemu).
List.network = { apart = 132, nearer = 14, closest = 82, middle = 274, highest = 95 }
function layouts.network(self, L, gfx, ui, top, lines)
  local k = self.network
  local nets = 0
  for _, l in ipairs(lines) do
    if type(l) == "table" and l.marks then nets = nets + 1 end
  end
  local more = math.max(0, nets - 1)
  local apart = math.max(k.apart - more * k.nearer, k.closest)
  local span = more * self.row + (lines[nets + 1] and apart + (#lines - nets - 1) * self.row or 0)
  L.first = math.max(k.highest, math.floor(k.middle - span / 2 + 0.5))
  if lines[nets + 1] then
    lines[nets + 1].gap = apart - self.row
  end
end

-- Stock's Equipment: the name on `title`, a grey list of the system's
-- parts centred on `middle`, the choices from `first`.
List.system = { title = 82, middle = 230.5, color = "#c4c4c4", first = 359 }
function layouts.system(self, L, gfx, ui, top)
  local k = self.system
  L.prompt[1] = { top.title, k.title }
  local parts = split(top.prompt)
  for i, line in ipairs(parts) do
    L.prompt[i + 1] = { line, k.middle - (#parts - 1) * self.prompt_step / 2 + (i - 1) * self.prompt_step, k.color }
  end
  L.first = k.first
end

-- Stock's "Confirm your choice:": the question on `question`, the answer
-- on `first` and Cancel under it (stock's Language).
List.ask = { question = 160, first = 254 }
function layouts.ask(self, L, gfx, ui, top)
  L.prompt[1] = { top.prompt, self.ask.question }
  L.first = self.ask.first
end

-- A prompt over answers. Stock keeps the answers where a two-line prompt
-- puts them, a single line `single` above where a pair's second would;
-- three lines start `three` higher and push the answers `pushed` down
-- (`pushed_icon` under a symbol; stock's Reminders, a unit more without
-- one). Under a symbol, three lines or more end on `icon_end` and the
-- symbol rises with them, `icon_over` over the first (stock's Nest
-- Sense). Answers alone (`top.late`) under three lines or more end on
-- `late_end` (five on `late_five`, six on `late_six`), the answers on
-- `late_first` (`late_six_first` under six; stock's Nest Pro, Reset,
-- system test, wired-only Heat Link).
List.answers = { single = 9, three = 24, pushed = 16, pushed_icon = -10, icon_end = 242, icon_over = 87.5,
                 late_end = 252, late_five = 278, late_six = 323, late_first = 359, late_six_first = 407 }
function layouts.answers(self, L, gfx, ui, top)
  local k = self.answers
  local lines = split(top.prompt)
  -- Counting blank lines too.
  local n = select(2, top.prompt:gsub("\n", "")) + 1
  local top_line = self.prompt_top + math.max(0, 2 - n) * self.prompt_step - (n >= 3 and k.three or 0)
  local lift = n == 1 and k.single or 0
  if top.icon and n >= 3 then
    top_line = k.icon_end - (n - 1) * self.prompt_step
    L.icon_y = top_line - k.icon_over
  elseif top.late and n >= 6 then
    top_line = k.late_six - (n - 1) * self.prompt_step
  elseif top.late and n >= 5 then
    top_line = k.late_five - (n - 1) * self.prompt_step
  elseif top.late and n >= 3 then
    top_line = k.late_end - (n - 1) * self.prompt_step
  end
  local base = top_line
  for i, line in ipairs(lines) do
    base = top_line + (i - 1) * self.prompt_step
    L.prompt[#L.prompt + 1] = { line, base - lift }
  end
  L.first = base + self.prompt_gap + (n >= 3 and (top.icon and k.pushed_icon or k.pushed) or 0)
  if n >= 3 and not top.icon and not top.late then L.first = L.first + 1 end
  if top.late and n >= 2 then L.first = n >= 6 and k.late_six_first or k.late_first end
end

-- A symbol, the name under it on `name`, the lines of text centred on
-- `text` in `color`, the entries from `first` (stock's Software).
List.titled_symbol = { name = 150, text = 248, color = "#c4c4c4", first = 360 }
function layouts.titled_symbol(self, L, gfx, ui, top)
  local k = self.titled_symbol
  L.prompt[1] = { (top.title:gsub("\n", " ")), k.name }
  local parts = split(top.prompt)
  for i, line in ipairs(parts) do
    L.prompt[i + 1] = { line, k.text - (#parts - 1) * self.prompt_step / 2 + (i - 1) * self.prompt_step, k.color }
  end
  L.first = k.first
end

-- Stock's Heat Link page: its state from `title`, a smaller line on
-- `line` (`size`), the Heat Link's picture from `picture_y`, `picture`
-- across, the choices from `first`.
List.heatlink = { title = 82, line = 152, size = 21, picture = 144, picture_y = 171, first = 387,
                  image = "unit:a2_heatlink_green.png" }
function layouts.heatlink(self, L, gfx, ui, top)
  local k = self.heatlink
  local n = 0
  for _, line in ipairs(split(top.title)) do
    n = n + 1
    L.prompt[n] = { line, k.title + (n - 1) * self.prompt_step }
  end
  if top.prompt then L.prompt[n + 1] = { top.prompt, k.line, "#ffffff", k.size } end
  L.first = k.first
end

-- Stock's Equipment Settings: the name on `name`, choices from `first`.
List.choices = { name = 124, first = 206 }
function layouts.choices(self, L, gfx, ui, top)
  L.prompt[1] = { top.title, self.choices.name }
  L.first = self.choices.first
end

-- A name, or a live line such as the time, over the entries, which a
-- short list centres; Done at the bottom (stock's Nest Sense and Date &
-- Time). One line sits on `one` (`name` as a title); more start on
-- `lines`, the rows `under` the last; a short list's rows centre on
-- `middle`, no higher than `highest`, `per_row` higher a row.
List.titled = { one = 142, name = 82, lines = 112, under = 79, middle = 254, highest = 150, per_row = 24 }
function layouts.titled(self, L, gfx, ui, top, lines)
  local k = self.titled
  local texts = split(top.prompt or (top.title:gsub("\n", " ")))
  local n = #texts
  for i, line in ipairs(texts) do
    L.prompt[i] = { line, (top.prompt and (i == 1 and k.one or k.lines + (i - 1) * self.prompt_step) or k.name) }
  end
  if n > 1 then L.prompt[1][2] = k.lines end
  L.first = math.max(k.highest, k.middle - (count_rows(lines) - 1) * k.per_row)
  if n > 1 then L.first = L.prompt[n][2] + k.under end
end

-- No prompt: a title without a symbol, scrolling with the rows, or
-- nothing over them.
function layouts.plain(self, L, gfx, ui, top)
  if not top.icon and top.title then L.title = top.title end
end

-- Which layout a page takes: the first whose test it passes.
List.order = {
  { "boiler", function(top) return top.boiler end },
  { "picture", function(top) return top.image end },
  { "picks", function(top) return top.picks end },
  { "network", function(top) return top.network end },
  { "system", function(top) return top.system end },
  { "ask", function(top) return top.ask end },
  { "answers", function(top) return top.prompt and not top.titled and not top.heatlink end },
  { "titled_symbol", function(top) return top.titled and top.icon end },
  { "heatlink", function(top) return top.heatlink end },
  { "choices", function(top) return top.choices end },
  { "titled", function(top) return top.titled end },
  { "plain", function() return true end },
}

-- The list: `lines` (a line may be a table { text, heading, gap, icon,
-- marks, last, detail }), `ticked` an index or a set, `highlighted` the
-- selected one, `gap` putting Back further down; `scroll(want, now)`
-- eases the scroll. Whether it still moves.
function List:draw(gfx, ui, top, lines, ticked, highlighted, gap, now, scroll)
  local c = ui.face.size / 2
  local parts = self.registry()
  -- A prompt's lines wider than the face (a translation, which comes
  -- without line breaks) wrap.
  if top.prompt then
    local out = {}
    for part in (top.prompt .. "\n"):gmatch("([^\n]*)\n") do
      if part == "" or gfx:measure(part, self.prompt_style) <= self.wrap then
        out[#out + 1] = part
      else
        for _, l in ipairs(alert.words(gfx, part, self.prompt_style, self.wrap_width)) do out[#out + 1] = l end
      end
    end
    top.prompt = table.concat(out, "\n")
  end
  -- Lines under the prompt (a place to confirm) are laid out with it,
  -- set lighter.
  local bold = math.huge
  if top.detail and top.prompt then
    bold = select(2, top.prompt:gsub("\n", "")) + 1
    top.prompt = top.prompt .. "\n" .. top.detail
  end
  local L = { prompt = {}, first = self.first, icon_y = self.icon_y }
  for _, entry in ipairs(self.order) do
    if entry[2](top) then
      self.layouts[entry[1]](self, L, gfx, ui, top, lines, highlighted)
      break
    end
  end
  local prompt, first = L.prompt, L.first
  -- Each line's baseline; an open section's lines { text, baseline, value }.
  local ys, y = {}, first
  local details = {}
  local s = self.section
  for i, l in ipairs(lines) do
    if i > 1 then y = y + self.row + (type(l) == "table" and l.gap or 0) end
    if gap and i == #lines then y = y + self.back_gap end
    local bottom = top.titled and self.titled_bottom or self.bottom
    if type(l) == "table" and l.last then y = math.max(bottom, i > 1 and y - self.row - (l.gap or 0) + self.last_gap or bottom) end
    ys[i] = y
    if type(l) == "table" and l.detail then
      local dy = y + s.first
      for _, d in ipairs(l.detail) do
        details[#details + 1] = { d.label, dy, false }
        for part in d.value:gmatch("[^\n]+") do
          dy = dy + s.step
          details[#details + 1] = { part, dy, true }
        end
        dy = dy + s.apart
      end
      y = dy - s.apart + s.next - self.row
    end
  end
  -- A list longer than the face starts at its top, as stock's Display
  -- does; then it all scrolls. One that fits, without a symbol, has its
  -- prompt a unit higher (stock's Reminders), but for answers alone
  -- under fewer than three lines.
  local lift = 0
  if not top.icon and not top.titled and not top.picks and not top.image and #prompt > 0 and y > self.fits then
    lift = prompt[1][2] - self.top_line
    for i = 1, #ys do ys[i] = ys[i] - lift end
  elseif top.prompt and not top.icon and not top.titled and not top.picks and not top.image and not top.system
      and not (top.late and #prompt < 3) then
    for _, p in ipairs(prompt) do p[2] = p[2] - 1 end
  end
  -- A long list keeps the entry after the highlighted one in view; one
  -- under a name scrolls with the selection; turned, a long list, a named
  -- one or Network's moves with the wheel's place (stock's Display, Nest
  -- Sense), its last entry ending at the end.
  local want = math.max(0, (ys[highlighted] or first) - (lift > 0 and self.keep_long or top.picks and self.keep_picks or self.keep))
  if (top.titled or top.network) and #ys > 1 then
    want = (highlighted - 1) / (#ys - 1) * math.max(0, ys[#ys] - self.last_end)
  end
  if top.list_at and (lift > 0 or top.titled or top.network) and #ys > 1 then
    want = top.list_at * math.max(0, ys[#ys] - self.last_end)
  end
  if (top.titled and top.icon) or top.system or top.choices then want = 0 end
  if top.prompt and top.icon and not (top.titled or top.picks or top.ask or top.late) and lift == 0 and ys[#ys] > self.keep then
    want = self:ring_offset(ui, top, ys[#ys] - self.ring.finish)
  end
  local lp = self.long_picks
  if top.picks and ys[#ys] > self.keep and not (type(lines[highlighted]) == "table" and lines[highlighted].last) then
    want = math.max(0, (ys[highlighted] or first) - (#prompt == 0 and lp.bare or lp.keep))
    want = math.min(want, math.max(0, ys[#ys] - lp.last))
  end
  local open = type(lines[highlighted]) == "table" and lines[highlighted].detail
  if open then want = math.max(0, ys[highlighted] - s.rise) end
  local offset, moving = scroll(want, now)
  if top.heatlink then
    local k = self.heatlink
    gfx:image(k.image, c - k.picture / 2, k.picture_y - offset, k.picture, k.picture)
  end
  if top.image then
    local w, h = top.image_w or self.picture.size, top.image_h or self.picture.size
    local image_y = L.image_top + (top.image == "unit:G_unfr0zen_192px.png" and 20 or 0)
    gfx:image(top.image, c - w / 2, image_y - offset, w, h)
  end
  if top.icon and not top.network then
    icons.draw(gfx, top.icon, c, L.icon_y - offset, self.icon_size, { p = 1, mode = top.mode or ui.thermostat.mode })
  end
  if L.title then
    gfx:text(L.title, c, self.title_y - offset, { font = self.title.font, size = self.title.size, color = self.title.color, align = "center" })
  end
  parts.prompt:draw(gfx, ui, c, prompt, bold, lift + offset)
  for i, l in ipairs(lines) do
    local text = type(l) == "table" and l.text or l
    local base = ys[i] - offset
    if base > self.above and base < self.below then
      if type(l) == "table" and l.heading then
        parts.heading:draw_centred(gfx, ui, text, c, base)
      else
        parts.list_row:draw(gfx, ui, c, base, text, { tick = (i == ticked) or (type(ticked) == "table" and ticked[i]),
                                                      icon = type(l) == "table" and l.icon, marks = type(l) == "table" and l.marks,
                                                      on = i == highlighted })
      end
    end
  end
  for _, d in ipairs(details) do
    local base = d[2] - offset
    if base > self.above and base < self.below then
      gfx:text(d[1], c, base, { font = self.prompt_style.font, size = self.prompt_style.size, color = d[3] and s.value or s.label, align = "center" })
    end
  end
  alert.fade_edges(gfx)
  return moving
end

return List
