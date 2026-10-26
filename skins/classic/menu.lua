-- The menus. The top menu is stock's ring: its entries' symbols stand round
-- the face, the selected one larger and in its colours, and the middle
-- names it (or, on Back, shows the day, time, room temperature and
-- humidity). Deeper menus are lists that scroll with the wheel. Moving
-- between levels slides; edits roll in.
local theme = require("theme")
local icons = require("icons")
local anim = require("anim")
local C = theme.colors
local M = theme.menu

local menu = {}

-- The components the menus draw with (a skin may replace them).
local parts = require("components")

-- Eased selection and scroll positions, per level.
local selection = anim.follower(0.06)
local scroll = anim.follower(0.07)
-- The level drawn last frame, and the slide from it to the current one.
local last_view, last_level
local slide = { from = nil, start = -1, dir = 1 }
-- When the edited value last changed, for its roll.
local last_value, value_changed = nil, -1

local function level_of(view)
  return table.concat(view.path, ">")
end

local format = require("components.format")
local format_temperature, degrees = format.temperature, format.degrees

-- A submenu: a list round the selected row, the value being edited large.
local function list(gfx, ui, view, now)
  local s = ui.face.size
  local cx, cy = s / 2, s / 2
  local offset, moving = scroll(view.selected, now)
  gfx:text(string.upper(view.title), cx, cy - 176, { font = theme.fonts.label, size = 20, spacing = 2, color = C.dim, align = "center", baseline = "middle" })

  if view.editing then
    local item = view.items[view.selected]
    local roll = anim.ease_out((now - value_changed) / 0.18)
    gfx:text(item.label, cx, cy - 64, { font = theme.fonts.body, size = 28, color = C.dim, align = "center", baseline = "middle" })
    gfx:save()
    gfx:alpha(roll)
    gfx:text(item.value or "", cx, cy + 6 + (1 - roll) * 24, { font = theme.fonts.number, size = 72, color = C.text, align = "center", baseline = "middle" })
    gfx:restore()
    gfx:line(cx - 30, cy + 64, cx + 30, cy + 64, 3, C.selected, true)
    return moving or roll < 1
  end

  for i, item in ipairs(view.items) do
    local d = i - offset
    if math.abs(d) < 2.6 then
      local y = cy + d * M.row
      local near = anim.clamp(1 - math.abs(d), 0, 1)
      gfx:save()
      gfx:alpha(anim.clamp(1 - math.abs(d) * 0.36, 0, 1))
      local size = M.row_text + 8 * near
      local color = near > 0.5 and C.text or C.icon
      if item.kind == "back" then
        icons.draw(gfx, "back", cx, y - 4, 30 + 8 * near, { p = near > 0.5 and 1 or 0 })
      else
        gfx:text(item.label, cx, y - (item.value and 10 or 0) * near, { font = theme.fonts.label, size = size, color = color, align = "center", baseline = "middle" })
        if item.value and near > 0.05 then
          gfx:save()
          gfx:alpha(near)
          gfx:text(item.value, cx, y + 22, { font = theme.fonts.body, size = 22, color = C.selected, align = "center", baseline = "middle" })
          gfx:restore()
        elseif item.kind == "menu" and near > 0.5 then
          gfx:text("›", cx + gfx:measure(item.label, { font = theme.fonts.label, size = size }) / 2 + 16, y, { font = theme.fonts.body, size = size, color = C.dim, baseline = "middle" })
        end
      end
      gfx:restore()
    end
  end
  return moving
end

local PROMPT = { font = theme.fonts.label, size = 32, color = "#ffffff", align = "center" }

-- Stock's list screen (the List component), eased by the menus' scroll.
local function list_screen(gfx, ui, top, lines, ticked, highlighted, gap, now)
  return parts.list:draw(gfx, ui, top, lines, ticked, highlighted, gap, now, scroll)
end

local function back_label(view)
  for _, row in ipairs(view.items) do
    if row.kind == "back" then return row.label end
  end
  return "Back"
end

-- Editing a choice: its options and Back on stock's list screen, the
-- symbol taking the highlighted option's colour.
local function choice(gfx, ui, view, item, now)
  local n = #item.options
  local pending = view.pending or (item.saved or 1)
  local lines = {}
  for i, o in ipairs(item.options) do lines[i] = string.upper(o.label) end
  -- Answers alone, as stock's Reminders, have no Back.
  if item.style ~= "answers" then lines[n + 1] = string.upper(back_label(view)) end
  local top = { icon = item.icon, prompt = item.prompt, title = item.label,
                mode = pending <= n and item.options[pending].value or nil }
  return list_screen(gfx, ui, top, lines, item.saved, pending, item.style ~= "answers", now)
end

-- A submenu styled "list": its entries on stock's list screen, under the
-- symbol and prompt of the entry it opened from.
local function entries(gfx, ui, view, now)
  local lines, ticked = {}, {}
  local last
  for i, row in ipairs(view.items) do
    if view.style == "network" then
      if row.kind == "set" then
        local flags = row.style or ""
        lines[i] = { text = row.label, marks = { connected = flags:find("c") ~= nil, saved = flags:find("s") ~= nil,
                                                 secured = flags:find("p") ~= nil, level = tonumber((row.icon or ""):match("%d")) } }
      else
        -- The choices stand apart under the networks.
        lines[i] = { text = string.upper(row.label), gap = last == "set" and 84 or 0 }
      end
    elseif view.style == "picks" or view.style == "question" or view.style == "boiler" then
      -- An exit sits 80 under the last pick, or on 408 when they fit;
      -- actions after the picks (Placement's ADD CUSTOM, ADD LABEL) stand
      -- a row apart in capitals, the exit straight under them (matched on
      -- nest-qemu).
      local exit = row.kind == "done" or row.kind == "back"
      if row.kind == "action" then
        lines[i] = { text = string.upper(row.label), gap = last == "set" and M.choice_step or 0 }
      else
        lines[i] = { text = exit and string.upper(row.label) or row.label, last = exit and last ~= "action" }
      end
      -- Questions tick the answer given; Placement's rooms are not ticked.
      if row.kind == "set" and view.style == "question" then ticked[i] = row.ticked end
    elseif view.style == "titled" or view.style == "system" or view.style == "choices" or view.style == "heatlink" then
      -- Entries in their own case (in capitals under a symbol, or as
      -- choices), Done in capitals at the bottom.
      local exit = row.kind == "done" or row.kind == "back"
      local capitals = exit or view.icon or view.style ~= "titled"
      lines[i] = { text = capitals and string.upper(row.label) or row.label, last = exit and view.style == "titled" and not view.icon, detail = row.detail }
      if row.kind == "set" then ticked[i] = row.ticked end
    elseif view.style == "answers" or view.style == "ask" then
      -- Answers in capitals, a symbol before some, a form apart below them,
      -- the one given ticked after an intro (stock's Reminders); a
      -- confirmation's Cancel carries the back arrow.
      local icon = (row.kind == "set" or (view.style == "ask" and row.kind == "back")) and row.icon or nil
      lines[i] = { text = view.title == "UNFR0ZEN" and row.label or string.upper(row.label), icon = icon, gap = row.kind == "form" and 32 or 0 }
      if row.kind == "set" and view.style == "answers" and view.intro then ticked[i] = row.ticked end
    elseif row.kind == "heading" then
      -- A heading under answers stands a little apart.
      lines[i] = { text = row.label, heading = true, gap = last and 12 or 0 }
    elseif row.kind == "set" or row.style == "answer" then
      -- Answers keep their case, as stock's.
      lines[i] = { text = row.label, gap = last == "heading" and 2 or 0 }
      ticked[i] = row.ticked
    else
      -- Done stands apart from the entries over it.
      lines[i] = { text = string.upper(row.label), gap = (last == "set" or row.kind == "done") and 32 or 0 }
    end
    last = row.kind
  end
  return list_screen(gfx, ui, { icon = view.icon, prompt = view.prompt, title = view.title, titled = view.style == "titled" or view.style == "choices",
                                system = view.style == "system", choices = view.style == "choices", network = view.style == "network",
                                picks = view.style == "picks" or view.style == "question", question = view.style == "question",
                                late = view.style == "answers" and not view.icon and not view.intro, list_at = view.list_at, detail = view.detail, boiler = view.style == "boiler", image = view.image, image_w = view.image_w, image_h = view.image_h, ask = view.style == "ask", heatlink = view.style == "heatlink" },
                     lines, ticked, view.selected, false, now)
end

-- A submenu: its entries as circles side by side, the selected one in the
-- middle and its neighbours at the edges, the menu's name above.
-- The slide ends within 1.5 px of its place (of 320 a tile), not creeping on.
local slide_to = anim.follower(0.08, 0.005)
local away

-- The home screen's colour now (black idle, its heating or cooling
-- colour while the system runs), for the carousel's way home.
local home_color = "#000000"

-- One entry of a submenu, as a tile centred on (x, c).
local function carousel_item(gfx, ui, item, x, c)
  local tile = parts.tile
  if item.kind == "done" then
    -- Done: the target, for the way home (on the home screen's colour
    -- while the system runs, matched with stock heating).
    local t = ui.thermostat
    local halves = math.floor(t.target * (t.unit == "F" and 1 or 2) + 0.5)
    local whole = t.unit == "F" and tostring(halves) or tostring(math.floor(halves / 2))
    tile:draw_done(gfx, x, c, whole, t.unit ~= "F" and halves % 2 == 1, t.leaf_on, home_color)
  elseif item.kind == "back" then
    tile:draw_back(gfx, x, c)
  else
    -- The name, in as many lines as it has, then what the tile says.
    local lines = {}
    for part in string.upper(item.label):gmatch("[^\n]+") do lines[#lines + 1] = part end
    local named = #lines
    local issue = false
    if item.summary and #item.summary > 0 then
      -- A form's tile sums its fields up: "Min: 5°C", "Max: Off".
      for _, s in ipairs(item.summary) do
        lines[#lines + 1] = s.short .. ": " .. (s.value and degrees(ui, s.value) or tr("Off"))
      end
    elseif item.style == "units" then
      -- Stock's °F / °C tile shows the room in the scale in use.
      lines[#lines + 1] = format_temperature(ui.thermostat) .. ui.thermostat.unit
    elseif item.style == "network" and item.value == "Not connected" then
      lines[#lines + 1] = item.value
    elseif item.style == "network" and item.value == "Wi-Fi error" then
      lines[#lines + 1] = item.value
      lines[#lines + 1] = "nest.com/w5"
    elseif item.value == tr("@equipmentscreen.systemissue|System Issue") then
      -- Stock's Equipment tile while the short-cycle flag stands: its
      -- link under it, under U+E037.
      lines[#lines + 1] = item.value
      lines[#lines + 1] = "nest.com/m13"
      issue = true
    elseif item.style == "network" and item.value then
      -- The network's name after a Wi-Fi symbol.
      lines[#lines + 1] = { text = item.value, wifi = true }
    elseif item.value and item.kind ~= "action" then
      for part in item.value:gmatch("[^\n]+") do lines[#lines + 1] = part end
    end
    -- A form's symbol is for its fields, but for the schedule's tile; a
    -- plugin's tile has its picture.
    local symbol = issue and "issue" or item.tile or item.icon
    local picture = symbol and (symbol:match("^unit:") or symbol:match("^run:")) and symbol or nil
    if picture or (item.kind == "form" and item.style ~= "schedule") then symbol = nil end
    tile:draw(gfx, ui, x, c, lines, named, symbol, picture)
  end
end

-- Everything a tile's drawing depends on, as a layer key.
local function tile_key(ui, item)
  local t = ui.thermostat
  local parts = { item.kind or "", item.label or "", tostring(item.value), item.style or "", item.tile or item.icon or "",
                  home_color, t.unit, tostring(t.target), tostring(t.leaf_on), format_temperature(t) }
  for _, s in ipairs(item.summary or {}) do parts[#parts + 1] = s.short .. "=" .. tostring(s.value) end
  return table.concat(parts, "|")
end

-- Bands across a square layer `2 h` wide, each as wide as the circle of
-- radius `r` in its middle needs: painted band by band, the square's clear
-- corners are not drawn at all (each pixel drawn costs the GPU).
local bands = {}
local function circle_bands(h, r)
  local key = h .. ":" .. r
  if bands[key] then return bands[key] end
  local list, n = {}, 8
  for k = 0, n - 1 do
    local top, bottom = 2 * h * k / n, 2 * h * (k + 1) / n
    local dy = (top <= h and bottom >= h) and 0 or math.min(math.abs(top - h), math.abs(bottom - h))
    local half = math.sqrt(math.max(r * r - dy * dy, 0)) + 2
    list[#list + 1] = { h - half, top, h + half, top, h + half, bottom, h - half, bottom }
  end
  bands[key] = list
  return list
end

-- A tile, drawn once into a layer the square about it and painted from
-- there: textured bands for filled tiles and one draw for outlined tiles.
local function carousel_tile(gfx, ui, i, item, x, c)
  -- Whole units, so the layer lands on whole pixels.
  local h = math.ceil(parts.tile.radius) + 2
  gfx:layer("tile." .. i, tile_key(ui, item), function(g)
    g:translate(0, h - c)
    carousel_item(g, ui, item, h, c)
  end, 2 * h, 2 * h)
  gfx:save()
  gfx:translate(x - h, c - h)
  if item.kind == "done" or item.kind == "back" then
    gfx:paint_layer("tile." .. i, "#ffffff")
  else
    for _, band in ipairs(circle_bands(h, h)) do
      gfx:paint_layer("tile." .. i, "#ffffff", band)
    end
  end
  gfx:restore()
end

-- A submenu: its entries as circles side by side, as stock's, the
-- selected one in the middle and its neighbours' edges showing; a turn of
-- the wheel slides them all along. The menu's name is above, and the face's
-- black rim over them all.
-- For the transitions, `only` picks what is drawn (`title`, `middle`,
-- `sides`, `rim`; all by default; `no_done` leaves the Done tile out),
-- moves every tile by `shift`, the middle tile by `dx` and the side tiles
-- out by `left` and `right`, and fades the title by `title_alpha`.
local function carousel(gfx, ui, view, now, only)
  only = only or {}
  local all = next(only) == nil
  local deck = parts.carousel
  local c = ui.face.size / 2
  -- Where the wheel's physics has it (`view.scroll`, from the first
  -- tile at 0), else gliding to the selected tile.
  local shown, moving
  if view.scroll then
    shown, moving = view.scroll + 1, false
  else
    shown, moving = slide_to(view.selected, now)
  end
  -- The title names the menu, or Done while Done is selected.
  local selected = view.items[view.selected]
  local title = (selected and selected.kind == "done") and selected.label or view.title
  if all or only.title then
    deck:draw_title(gfx, ui, string.upper(title), only.title_alpha)
  end
  for i, item in ipairs(view.items) do
    -- On whole pixels: the circles' edges would shimmer, and the text
    -- (set on whole pixels) jump against them, through the slide's slow end.
    local offset = i - shown
    local x = c + math.floor(offset * deck.step + (only.shift or 0) + 0.5)
    local middle = math.abs(offset) < 0.5
    if middle then x = x + (only.dx or 0) end
    if offset < 0 and not middle then x = x - (only.left or 0) end
    if offset > 0 and not middle then x = x + (only.right or 0) end
    if math.abs(x - c) < c + parts.tile.radius and (all or (middle and only.middle) or (not middle and only.sides))
       and not (only.no_done and item.kind == "done") then
      carousel_tile(gfx, ui, i, item, x, c)
    end
  end
  if all or only.rim then
    deck:draw_rim(gfx, ui)
  end
  return moving
end

-- The fan running: what is left of its run in white, the minutes left,
-- and Stop fan over Back.
local function running(gfx, ui, view, item)
  local n = #item.options
  local fraction = (math.min(item.saved or 1, n) - 1) / (n - 1) * ui.fan.left
  parts.dial:draw_running(gfx, ui, item.icon, fraction, ui.fan.left_text, view.pending ~= nil and view.pending <= n, ui.fan.speed)
  return false
end

-- A choice styled "dial", or a range, being edited: stock's fan timer (the
-- Dial component), Back highlighted while nothing is set.
local function dial(gfx, ui, view, item, now)
  if ui.fan and item.options then return running(gfx, ui, view, item) end
  local fraction, label, unset
  if item.options then
    local n = #item.options
    local pending = view.pending or item.saved or 1
    unset = pending == 1 or pending > n
    fraction = (math.min(pending, n) - 1) / (n - 1)
    label = item.options[math.min(pending, n)].label
  else
    fraction, label, unset = item.fraction or 0, item.value or "", false
  end
  return parts.dial:draw(gfx, ui, item.icon, fraction, label, unset, now)
end

-- Stock's form (Eco, Safety Temp): the prompt, a row per field with its
-- label and a boxed value (outlined blue when selected, filled while
-- changed), then DONE and MORE INFO. A symbol in the entry (Eco's leaf)
-- sits in each box and whitens the labels.
local BUTTONS = { done = "DONE", more = "MORE INFO", set = "SET", back = "BACK", disable = "DISABLE",
                  ["quiet on"] = "QUIET TIME ON", ["quiet off"] = "QUIET TIME OFF" }

-- A form's buttons under its fields, from 360 (or `y`), 48 apart.
local function form_buttons(gfx, ui, c, f, y)
  for i, name in ipairs(f.buttons) do
    parts.button:draw(gfx, ui, BUTTONS[name] or string.upper(name), c, (y or 360) + (i - 1) * 48, f.at == #f.fields + i)
  end
end

-- Field `i`'s box centred on (x, y), `w` wide.
local function field_box(gfx, f, i, x, y, w, text)
  parts.field_box:draw(gfx, x, y, w, text, f.at == i, f.changing)
end

-- A label "for minutes/for always": its first part, or its second while
-- the duty is always.
local function phrase(label, always)
  local short, long = label:match("^(.-)/(.*)$")
  if not short then return label end
  return always and long or short
end

-- The fan's pages lay their rows out as stock's lists (alert.list): a
-- text 1 under its row's place, a box 11 over it (matched on nest-qemu
-- with the fan schedule), 61 from row to row.
local FAN_ROW = 61

-- A speed's row of a fan of more speeds: "at [low] speed." in three rows
-- from `row`, its box `w` wide.
local function speed_rows(gfx, f, i, c, row)
  local field = f.fields[i]
  local before, after = (field.label or ""):match("^(.-)/(.*)$")
  gfx:text(before or "", c, row + 1, PROMPT)
  field_box(gfx, f, i, c, row + FAN_ROW - 11, 158, field.text or "")
  gfx:text(after or "", c, row + 2 * FAN_ROW + 1, PROMPT)
end

-- Stock's fan schedule: "Run the fan for [15 mins] per hour from [08] to
-- [17]" (or "Run the fan [always] from"), its fields a duty in minutes
-- (60: always) and two hours (matched on nest-qemu); with a fan of more
-- speeds "at [low] speed." under them, the page taller than the face
-- (stock's 7 rows, 0x13e264) and moved up a step each field or button
-- the focus moves down, from none on the first to all its overflow on the
-- last (stock's scrollable list, 0x23aba8: the offset in proportion to
-- the focus).
local function schedule(gfx, ui, f)
  local c = ui.face.size / 2
  local duty, from, to = f.fields[1], f.fields[2], f.fields[3]
  local speed = f.fields[4] and 4
  local first, _, foot = require("alert").list(((speed and 7) or 4) * FAN_ROW - FAN_ROW, 0, 2)
  local stops = #f.fields + #f.buttons
  local dy = math.max(0, foot - 324) * anim.clamp((f.at - 1) / math.max(1, stops - 1), 0, 1)
  local row = function(i) return first + (i - 1) * FAN_ROW - dy end
  local always = not duty.value or duty.value >= 60
  gfx:text(phrase(duty.label, always), c, row(1) + 1, PROMPT)
  field_box(gfx, f, 1, c, row(2) - 11, 158, always and "always" or string.format("%d mins", math.floor(duty.value + 0.5)))
  gfx:text(phrase(from.label, always), c, row(3) + 1, PROMPT)
  local hour = function(field) return string.format("%02d", math.floor((field.value or 0) + 0.5)) end
  field_box(gfx, f, 2, 172, row(4) - 10, 82, hour(from))
  gfx:text(to.label, c, row(4) + 1, PROMPT)
  field_box(gfx, f, 3, 307, row(4) - 10, 82, hour(to))
  if speed then speed_rows(gfx, f, speed, c, row(5)) end
  for i, name in ipairs(f.buttons) do
    parts.button:draw(gfx, ui, BUTTONS[name] or string.upper(name), c, foot - dy + 36 + (i - 1) * 48, f.at == #f.fields + i)
  end
  return false
end

-- Stock's fan timer for a fan of more speeds (fanlayer state 4, table
-- 0x706548): "Run the fan for the next [15 minutes] at [low] speed.",
-- DONE at the foot starting it.
local function fan_timer(gfx, ui, f)
  local c = ui.face.size / 2
  local first, _, foot = require("alert").list(4 * FAN_ROW, 0, 1)
  gfx:text(f.fields[1].label, c, first + 1, PROMPT)
  field_box(gfx, f, 1, c, first + FAN_ROW - 11, 220, f.fields[1].text or "")
  speed_rows(gfx, f, 2, c, first + 2 * FAN_ROW)
  parts.button:draw(gfx, ui, BUTTONS[f.buttons[1]] or "DONE", c, foot + 36, f.at == #f.fields + 1)
  return false
end

-- Stock's System fan speed (fanlayer state 1, table 0x7064e8): the
-- question as a list's text, two rows on its chooser, DONE at the foot.
local function fan_speed(gfx, ui, f)
  local c = ui.face.size / 2
  local alert = require("alert")
  local below = 2 * FAN_ROW
  local function place(n) return (alert.list((n - 1) * 39 + below, 0, 1)) end
  local lines, first = alert.block(gfx, f.prompt or "", PROMPT, nil, nil, place)
  for i, l in ipairs(lines) do gfx:text(l, c, first + (i - 1) * 39, PROMPT) end
  local _, _, foot = alert.list((#lines - 1) * 39 + below, 0, 1)
  field_box(gfx, f, 1, c, first + (#lines - 1) * 39 + below - 11, 158, f.fields[1].text or "")
  parts.button:draw(gfx, ui, BUTTONS[f.buttons[1]] or "DONE", c, foot + 36, f.at == #f.fields + 1)
  return false
end

local MONTHS = { "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec" }

-- Stock's Set time: its prompt, then month, day and year in boxes, hour
-- and minute under them, and Done at the bottom (matched on nest-qemu).
local function datetime(gfx, ui, f)
  local c = ui.face.size / 2
  local n = 0
  for line in (f.prompt or ""):gmatch("[^\n]+") do
    gfx:text(line, c, 82 + n * 39, PROMPT)
    n = n + 1
  end
  local value = function(i) return math.floor((f.fields[i].value or 0) + 0.5) end
  field_box(gfx, f, 1, 134, 188, 108, MONTHS[(value(1) - 1) % 12 + 1])
  field_box(gfx, f, 2, 241.5, 188, 90, tostring(value(2)))
  field_box(gfx, f, 3, 348.5, 188, 105, tostring(value(3)))
  field_box(gfx, f, 4, 190.5, 250, 90, string.format("%02d", value(4)))
  field_box(gfx, f, 5, 289.5, 250, 90, string.format("%02d", value(5)))
  -- The country's time zone under them, where it has any (its place
  -- unchecked against stock).
  if f.fields[6] then field_box(gfx, f, 6, c, 312, 300, f.fields[6].text or "") end
  parts.button:extend({ snap = false }):draw(gfx, ui, BUTTONS[f.buttons[1]] or "DONE", c, 408, f.at == #f.fields + 1)
  return false
end

-- Stock's Pro Setup forms ("Heating", "Hot water"): the name on 83, then
-- a row each 43 apart, centred on the middle row, its label right of 154
-- and its option in a 270 wide box from 176; CONTINUE at the bottom
-- (matched on nest-qemu).
local function equipment_form(gfx, ui, f)
  local c = ui.face.size / 2
  gfx:text(f.prompt or "", c, 83, { font = theme.fonts.strong, size = 32, color = "#ffffff", align = "center" })
  local n = #f.fields
  for i, field in ipairs(f.fields) do
    local y = 244 + (i - (n + 1) / 2) * 43
    local selected = f.at == i
    gfx:text(field.label, 154, y + 5, { font = theme.fonts.strong, size = 26, color = "#ffffff", align = "right" })
    gfx:rect(176, y - 18, 270, 36, selected and M.pill or "#666666", 8)
    gfx:rect(178, y - 16, 266, 32, (selected and f.changing) and "#003d55" or theme.colors.menu, 6)
    gfx:text(field.text or "", 311, y + 5, { font = theme.fonts.strong, size = 21, color = selected and "#ffffff" or "#666666", align = "center" })
  end
  parts.button:extend({ snap = false }):draw(gfx, ui, BUTTONS[f.buttons[1]] or "CONTINUE", c, 408, f.at == n + 1)
  return false
end

-- Stock's humidity quiet time: "Do not run the humidifier from [22:00]
-- to [06:00] every day.", then QUIET TIME ON and QUIET TIME OFF (its
-- layout unchecked against stock).
local function quiet_time(gfx, ui, f)
  local c = ui.face.size / 2
  local from, to = f.fields[1], f.fields[2]
  local hour = function(field) return string.format("%02d:00", math.floor((field.value or 0) + 0.5)) end
  local n = 0
  for line in from.label:gmatch("[^\n]+") do
    gfx:text(line, c, 102 + n * 39, PROMPT)
    n = n + 1
  end
  field_box(gfx, f, 1, 152, 210, 110, hour(from))
  gfx:text(tr(to.label), c, 221, PROMPT)
  field_box(gfx, f, 2, 328, 210, 110, hour(to))
  gfx:text(tr("every day."), c, 290, PROMPT)
  form_buttons(gfx, ui, c, f)
  return false
end

local function form(gfx, ui, f)
  if f.style == "equipment" then return equipment_form(gfx, ui, f) end
  if f.style == "quiettime" then return quiet_time(gfx, ui, f) end
  if f.style == "schedule" then return schedule(gfx, ui, f) end
  if f.style == "dial" then return fan_timer(gfx, ui, f) end
  if f.style == "speed" then return fan_speed(gfx, ui, f) end
  if f.style == "datetime" then return datetime(gfx, ui, f) end
  local c = ui.face.size / 2
  -- A single field sits lower, its prompt with it (matched with stock's
  -- Eco and Early-On forms).
  local lower = (2 - #f.fields) * 55
  local n = 0
  local top = (f.icon and 106 or 107) + lower * 15 / 55
  -- A line wider than the face (another language's) breaks where it fits.
  for text in (f.prompt or ""):gmatch("[^\n]+") do
    for _, line in ipairs(require("alert").words(gfx, text, PROMPT, 380)) do
      gfx:text(line, c, top + n * 39, PROMPT)
      n = n + 1
    end
  end
  -- A long prompt (Equipment Settings' sliders) pushes the fields down.
  local first = math.max(211 + lower, top + (n - 1) * 39 + 56)
  local label_color = (f.icon or f.fields[1].unit) and "#ffffff" or "#bdbdbd"
  for i, field in ipairs(f.fields) do
    local y = first + (i - 1) * 61
    -- A count's label has no colon (stock's Early-On).
    local row = field.unit and parts.form_row.count or (f.icon and parts.form_row.symbol or parts.form_row)
    local name = field.unit and field.label or field.label .. ":"
    local text = field.value and degrees(ui, field.value, true) or "off"
    if field.text then
      -- A named end ("always", "Never use alt.").
      text = field.text
    elseif field.unit and not field.value then
      text = "off"
    elseif field.unit == "hours" then
      text = field.value == 1 and "1 hour" or string.format("%d hours", math.floor(field.value + 0.5))
    elseif field.unit and field.unit:sub(1, 1) == "%" then
      text = string.format("%d%s", math.floor(field.value + 0.5), field.unit)
    elseif field.unit then
      text = string.format("%d %s", math.floor(field.value + 0.5), field.unit)
    end
    if field.label == "" then
      -- A field the prompt names: its box alone, as wide as its text needs.
      local w = math.max(150, gfx:measure(text, { font = theme.fonts.label, size = 32 }) + 40)
      field_box(gfx, f, i, c, y, w, text)
    else
      row:draw(gfx, ui, c, y, name, text, label_color, f.at == i, f.changing, f.icon)
    end
  end
  -- Below the last field however far the prompt pushed it.
  form_buttons(gfx, ui, c, f, math.max(360, first + (#f.fields - 1) * 61 + 58))
  return false
end

-- Stock's Restart (Reset > Restart): the confirm ring, CANCEL under it.
local function confirm(gfx, ui, view, now)
  return parts.confirm_ring:draw(gfx, ui, view.prompt, string.upper(view.items[1] and view.items[1].label or "Cancel"), view.turned, now)
end

-- Whether something other than a carousel was drawn last (another level,
-- a page over the menus): a carousel shown again starts in place rather
-- than sliding from where it was left.
away = true

-- Whether a level is drawn as a carousel.
local function is_carousel(view)
  if not view or view.form or view.style == "confirm" or view.depth == 0 then return false end
  local item = view.items[view.selected]
  if view.editing and item and (item.style == "dial" or item.kind == "range" or (item.kind == "choice" and item.options)) then
    return false
  end
  return not (view.style == "list" or view.style == "answers" or view.style == "ask" or view.style == "titled"
    or view.style == "picks" or view.style == "question" or view.style == "system" or view.style == "choices"
    or view.style == "network" or view.style == "heatlink" or view.style == "boiler")
end

local function level(gfx, ui, view, phase, now)
  local was_away = away
  away = true
  if view.form then return form(gfx, ui, view.form) end
  if view.style == "confirm" then return confirm(gfx, ui, view, now) end
  local item = view.items[view.selected]
  if view.editing and item and (item.style == "dial" or item.kind == "range") then
    return dial(gfx, ui, view, item, now)
  end
  if view.editing and item and item.kind == "choice" and item.options then
    return choice(gfx, ui, view, item, now)
  end
  if view.depth == 0 then return parts.ring:draw(gfx, ui, view, phase.icons, phase.text, now, phase.under) end
  if view.style == "list" or view.style == "answers" or view.style == "ask" or view.style == "titled" or view.style == "picks" or view.style == "question"
     or view.style == "system" or view.style == "choices" or view.style == "network" or view.style == "heatlink"
     or view.style == "boiler" then
    return entries(gfx, ui, view, now)
  end
  if was_away then slide_to = anim.follower(0.08, 0.005) end
  away = false
  return carousel(gfx, ui, view, now)
end

-- Into Settings from the top menu, as stock's settings view enters: one
-- sin² over `length` s eases it all, started `-open` s before the first
-- frame (stock starts its clock at the press, then draws its images). The top menu, frozen,
-- turns away as a card, its left edge nearing, shrinking to 0.6 as it goes
-- and following the carousel left, onto the Done tile, which its back
-- shows. Behind it the carousel scrolls a tile, from Done to the first
-- tile, its title fading in over the second half.
local ENTER = { open = -0.04, length = 0.8 }
-- Opening a tile, as stock's texture flip: the tile, frozen, turns on its
-- upright axis as a card seen in perspective (its right edge nearing), and
-- the page, frozen and opaque, turns in on its back: 180 degrees eased
-- sin² over `length` s, after `open` s (`close` s closing, played back).
-- `depth` is the card's distance in half-faces, matched to stock frame by
-- frame. The title stays under the card, the side tiles over it: opening,
-- they leave 64 outwards, easing out, the left from 64% of the turn for
-- 13%, the right from 79% for 23%; closing, they come back easing in, the
-- right from the start for 27%, the left from 26% for 14%.
local FLIP = { open = 0.03, close = 0, length = 0.4, depth = 1.6 }

local function flip_length(closing)
  return (closing and FLIP.close or FLIP.open) + FLIP.length
end

local function span(e, from, length)
  return anim.clamp((e - from) / length, 0, 1)
end

-- Where a layer lies as a card turned `angle` about its upright axis, its right
-- edge coming nearer (its left with `o.left`); `back` shows it from behind
-- (mirrored back to read). The layer is the middle `half` of the face each
-- way from its centre (in half-faces), or all of it. The card stands
-- `o.depth` half-faces away (`FLIP.depth`, where it fills the face) and is
-- moved across by `o.shift` units as seen at 0.6 of the face's size.
local function corners(ui, angle, back, half, o)
  o = o or {}
  local c = ui.face.size / 2
  local k = FLIP.depth
  local depth, near = o.depth or k, o.left and -1 or 1
  local across = (o.shift or 0) / (0.6 * c)
  local function at(u, v)
    local d = depth - near * u * math.sin(angle)
    return c + c * k * (u * math.cos(angle) + across) / d, c + c * k * v / d
  end
  half = half or 1
  local l, r = -half, half
  if back then l, r = half, -half end
  local x0, y0 = at(l, -half)
  local x1, y1 = at(r, -half)
  local x2, y2 = at(r, half)
  local x3, y3 = at(l, half)
  return { x0, y0, x1, y1, x2, y2, x3, y3 }
end

-- Layer `name` as that card.
local function card(gfx, ui, name, angle, back, half, o)
  gfx:warp_layer(name, corners(ui, angle, back, half, o))
end

-- Whether the card shows its front: its top edge runs left to right on
-- the face. Moved across, the card is seen from the side it faces the eye
-- from, which its angle alone does not tell.
local function front_shows(ui, angle, o)
  local q = corners(ui, angle, false, 1, o)
  return (q[3] - q[1]) * (q[8] - q[2]) - (q[4] - q[2]) * (q[7] - q[1]) > 0
end

-- `t` s into Settings from the top menu `top` (or, `leaving`, back out).
local function enter(gfx, ui, carousel_view, top, t, now, key, leaving)
  local c = ui.face.size / 2
  local step = M.carousel_step
  local p = math.sin(math.pi * span(t, ENTER.open, ENTER.length) / 2) ^ 2
  if leaving then p = 1 - p end
  carousel(gfx, ui, carousel_view, now, { title = true, middle = true, sides = true, no_done = true,
                                          shift = (1 - p) * step, title_alpha = anim.clamp(2 * p - 1, 0, 1) })
  gfx:layer("enter.front", key, function(g)
    level(g, ui, top, { icons = 1, text = 1 }, now)
  end)
  local done
  for _, item in ipairs(carousel_view.items) do
    if item.kind == "done" then done = item end
  end
  gfx:layer("enter.back", key, function(g)
    if not done then return end
    g:translate(c, c)
    g:scale(1 / 0.6, 1 / 0.6)
    g:translate(-c, -c)
    carousel_item(g, ui, done, c, c)
  end)
  local angle = math.pi * p
  local o = { left = true, depth = FLIP.depth * (1 + 2 * p / 3), shift = -p * step }
  if front_shows(ui, angle, o) then
    card(gfx, ui, "enter.front", angle, false, 1, o)
  else
    card(gfx, ui, "enter.back", angle, true, 1, o)
  end
  gfx:ring(c, c, theme.dial.rim, theme.dial.rim_width, "#000000")
end

-- `t` s into a tile opening onto `draw_page` (or, `closing`, shutting).
local function flip(gfx, ui, carousel_view, draw_page, t, now, key, closing)
  local u = ui.face.size / 480
  local x = span(t, closing and FLIP.close or FLIP.open, FLIP.length)
  local p = math.sin(math.pi * x / 2) ^ 2
  if closing then p = 1 - p end
  -- Both faces are drawn before the turn starts, as stock does: the tile
  -- in the square about it, the page whole.
  local c = ui.face.size / 2
  local h = M.circle + 2 * u
  gfx:layer("flip.tile", key, function(g)
    g:translate(h - c, h - c)
    carousel(g, ui, carousel_view, now, { middle = true })
  end, 2 * h, 2 * h)
  gfx:layer("flip.page", key, function(g)
    g:clear("#000000")
    draw_page(g)
  end)
  carousel(gfx, ui, carousel_view, now, { title = true })
  if p < 0.5 then
    card(gfx, ui, "flip.tile", math.pi * p, false, h / c)
  else
    card(gfx, ui, "flip.page", math.pi * p, true)
  end
  local function leave(from, length) return 64 * u * (1 - (1 - span(x, from, length)) ^ 2) end
  local function come(from, length) return 64 * u * (1 - span(x, from, length) ^ 2) end
  local left, right
  if closing then
    left, right = come(0.26, 0.14), come(0, 0.27)
  else
    left, right = leave(0.64, 0.13), leave(0.79, 0.23)
  end
  carousel(gfx, ui, carousel_view, now, { sides = true, rim = true, left = left, right = right })
end

-- A page drawn over the menus (an info page), the carousel it opened from
-- and when, so that it flips open and shut as a level does.
local page = { draw = nil, from = nil, start = -1, closing = false }

-- Draws a page over the menus with `draw_page(gfx)`: flipping in from the
-- carousel it opened from, as stock's tiles do.
function menu.page(gfx, ui, draw_page)
  local now = ui.now
  if last_view and is_carousel(last_view) then
    page.from, page.start = last_view, now
  end
  away = true
  last_view, last_level, slide.from = nil, nil, nil
  page.draw, page.closing = draw_page, false
  local e = now - page.start
  if page.from and e < flip_length(false) then
    flip(gfx, ui, page.from, draw_page, e, now, "page" .. page.start, false)
    return true
  end
  return draw_page(gfx)
end

-- Draws the menus. `phase.icons` and `phase.text` run 0 to 1 as the menus
-- open (and back as they close): the top menu's symbols turn face-on and
-- its middle fades in. `phase.under` is the colour behind the symbols,
-- for their cut-outs, while the dial is still showing.
function menu.draw(gfx, ui, view, phase)
  home_color = phase.background or "#000000"
  local s = ui.face.size
  local now = ui.now
  local animating = false


  -- Back from a page to the carousel it opened from: the page flips shut.
  if page.draw and page.from and is_carousel(view) and level_of(page.from) == level_of(view) then
    if not page.closing then page.closing, page.start = true, now end
    local e = now - page.start
    if e < flip_length(true) then
      flip(gfx, ui, view, page.draw, e, now, "page" .. page.start, true)
      last_view, last_level = view, level_of(view)
      return true
    end
  end
  page.draw, page.from = nil, nil

  local key = level_of(view)
  -- A tile opening in place (a form, an edit) changes what is drawn, not
  -- the level.
  local reshaped = last_view and is_carousel(view) ~= is_carousel(last_view)
  if last_level and (key ~= last_level or reshaped) then
    slide.from, slide.start = last_view, now
    slide.dir = (view.depth >= (last_view and last_view.depth or 0)) and 1 or -1
    selection = anim.follower(0.06)
    scroll = anim.follower(0.07)
  end
  local item = view.items[view.selected]
  local value = view.editing and item and item.value or nil
  if value ~= last_value then
    value_changed, last_value = now, value
  end
  last_view, last_level = view, key

  if slide.from then
    local e = now - slide.start
    local kind, carousel_view, other, closing = nil, nil, nil, false
    if slide.from.depth == 0 and is_carousel(view) then
      kind, carousel_view, other, e = "enter", view, slide.from, e
    elseif view.depth == 0 and is_carousel(slide.from) then
      kind, carousel_view, other, closing = "enter", slide.from, view, true
    elseif is_carousel(slide.from) and not is_carousel(view) and view.depth >= slide.from.depth then
      kind, carousel_view, other = "flip", slide.from, view
    elseif is_carousel(view) and not is_carousel(slide.from) and slide.from.depth >= view.depth then
      kind, carousel_view, other, closing = "flip", view, slide.from, true
    end
    if kind == "enter" and e < ENTER.open + ENTER.length then
      enter(gfx, ui, carousel_view, other, e, now, "slide" .. slide.start, closing)
      return true
    elseif kind == "flip" and e < flip_length(closing) then
      flip(gfx, ui, carousel_view, function(g) level(g, ui, other, { icons = 1, text = 1 }, now) end, e, now,
           "slide" .. slide.start, closing)
      return true
    elseif not kind then
      -- Elsewhere the levels push each other along, opaque, in 0.3 s,
      -- each in a layer the size of the face.
      local p = anim.ease_in_out(e / 0.3)
      if p < 1 then
        local function push(name, v, x)
          gfx:layer(name, "slide" .. slide.start, function(g) level(g, ui, v, { icons = 1, text = 1 }, now) end)
          gfx:save()
          gfx:translate(x, 0)
          gfx:paint_layer(name)
          gfx:restore()
        end
        push("slide.from", slide.from, -slide.dir * s * p)
        push("slide.to", view, slide.dir * s * (1 - p))
        return true
      end
    end
  end
  slide.from = nil
  return level(gfx, ui, view, phase, now) or animating
end

-- Forgets the eased positions, so the menus open fresh.
function menu.reset()
  last_view, last_level, slide.from = nil, nil, nil
  away = true
  parts.ring.reset()
  selection = anim.follower(0.06)
  scroll = anim.follower(0.07)
end

return menu
