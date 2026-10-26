-- The screens and stock's transitions between them: the home screen, the
-- menus over it. Given a home screen module (`draw(gfx, ui, look)` and
-- `background(ui)`, as home.lua's), returns a skin's `draw`; other skins
-- reuse the menus with their own home screen as require("classic:screens").
local menu = require("menu")
local alert = require("alert")
local schedule = require("schedule")
local history = require("history")
local keyboard = require("keyboard")
local postal = require("postal")
local wifi = require("wifi")
local anim = require("anim")
local theme = require("theme")

-- Stock's transitions, in seconds from the change, matched to stock side by
-- side on nest-qemu (client-ng/TESTING.md). Opening, the dial
-- shrinks and darkens while the menu's symbols stand round it, then the
-- menu's middle fades in; closing, the middle fades, the symbols turn
-- edge-on, and the dial grows back and brightens.
local OPEN = { dial = 0.22, text_from = 0.18, text = 0.12 }
local CLOSE = { text = 0.09, flip_from = 0.06, flip = 0.12, dial_from = 0.12, dial = 0.22 }
-- Closing from a submenu (a carousel's DONE), as stock's: the home screen
-- grows from the Done tile's size to the face's, eased sin² over `dial` s
-- from `dial_from`, its ticks fading in as the tile's ring fades out over
-- the first third, the words over the number back once it is done.
local CLOSE_SUB = { dial_from = 0.04, dial = 0.5 }
local SHRUNK = 0.6
-- Ending an edit opened from the top menu turns its screen edge-on, then
-- the dial face-on, each over FLIP seconds.
local FLIP = 0.25
local STARTING = { wait = 0.7, fade = 0.75 }

local function phase(t, from, length)
  return anim.clamp((t - from) / length, 0, 1)
end

return function(home)
  -- Which screen was drawn last, when that changed, and the last menu seen,
  -- so the menus can fold away after the engine has closed them.
  local last_screen, changed_at, last_menu = nil, -100, nil

  -- What lies behind the top menu's symbols (at about 185 units from the
  -- middle) while the dial, `scale` across and `light` bright, shows.
  local function under(ui, scale, light)
    if scale * ui.face.size / 2 < 190 then return "#000000" end
    return anim.mix_color("#000000", home.background(ui), light)
  end

  -- An alert, notice or dialog over Farsight, as stock's (0x2273d8):
  -- Farsight under it, the overlay drawn into a layer and painted at the
  -- opacity `home.over` gives (none while Farsight is far).
  local function over_far(gfx, ui, draw)
    local a = ui.far and home.over and home.over(ui)
    if not a or a >= 1 then return draw(gfx) end
    local farsight = home.farsight or home.draw
    farsight(gfx, ui)
    if a > 0 then
      gfx:layer("over farsight", ui.now, draw)
      gfx:paint_layer("over farsight", string.format("#ffffff%02x", math.floor(a * 255 + 0.5)))
    end
    return true
  end

  return function(gfx, ui)
    if ui.menu then last_menu = ui.menu end
    if ui.screen ~= last_screen then
      if last_screen ~= nil then changed_at = ui.now end
      if ui.screen == "menu" then menu.reset() end
      last_screen = ui.screen
    end
    local t = ui.now - changed_at

    if ui.screen == "dialog" and ui.dialog then
      return over_far(gfx, ui, function(g) return alert.dialog(g, ui) end)
    end

    if ui.screen == "systest" and ui.test then
      return alert.test(gfx, ui)
    end

    if ui.screen == "wifi" and ui.wifi then
      return wifi.draw(gfx, ui)
    end

    if ui.screen == "keyboard" and ui.keyboard then
      return keyboard.draw(gfx, ui, t)
    end

    if ui.screen == "postal" and ui.postal then
      return postal.draw(gfx, ui)
    end

    if ui.screen == "history" and ui.history then
      return history.draw(gfx, ui, home)
    end

    if ui.screen == "schedule" and ui.schedule then
      return schedule.draw(gfx, ui, home.background(ui))
    end

    if ui.screen == "farsight" then
      return (home.farsight or home.draw)(gfx, ui)
    end

    -- Waiting for the backplate: black for STARTING.wait seconds, as
    -- stock's boot layer, then the activity arc fading in.
    if ui.screen == "starting" then
      local waited = ui.now - ui.changed.screen - STARTING.wait
      gfx:clear("#000000")
      if waited > 0 then alert.arc(gfx, ui, anim.clamp(waited / STARTING.fade, 0, 1)) end
      return true
    end

    if ui.screen == "notice" and ui.notice then
      return over_far(gfx, ui, function(g) return alert.notice(g, ui) end)
    end

    if ui.screen == "error" and ui.error then
      return alert.error(gfx, ui)
    end

    if ui.screen == "alert" and ui.alert then
      -- Fades in when it comes up and each time the display wakes.
      return over_far(gfx, ui, function(g) return alert.draw(g, ui, ui.now - ui.changed.screen) end)
    end

    if ui.screen == "menu" and ui.menu and ui.menu.info then
      local info, top = ui.menu.info, ui.menu.info_top
      return menu.page(gfx, ui, function(g) return alert.info(g, ui, info, top) end)
    end

    if ui.screen == "menu" and ui.menu then
      local p = anim.ease_out(phase(t, 0, OPEN.dial))
      local animating = p < 1
      local scale, light = 1 - (1 - SHRUNK) * p, 1 - p
      if p < 1 then
        home.draw(gfx, ui, { scale = scale, light = light })
      else
        gfx:clear(theme.colors.menu)
      end
      local text = phase(t, OPEN.text_from, OPEN.text)
      animating = menu.draw(gfx, ui, ui.menu, { icons = 1, text = text, under = under(ui, scale, light), background = home.background(ui) }) or animating
      return animating or text < 1
    end

    if last_menu and last_menu.editing and t < 2 * FLIP then
      if t < FLIP then
        gfx:clear(theme.colors.menu)
        local c = ui.face.size / 2
        gfx:save()
        gfx:translate(c, c)
        gfx:scale(math.max(0.001, math.cos(math.pi / 2 * anim.ease_in_out(t / FLIP))), 1)
        gfx:translate(-c, -c)
        menu.draw(gfx, ui, last_menu, { icons = 1, text = 1, background = home.background(ui) })
        gfx:restore()
      else
        home.draw(gfx, ui, { squash = math.max(0.001, math.sin(math.pi / 2 * anim.ease_in_out((t - FLIP) / FLIP))) })
      end
      return true
    end
    local from_sub = last_menu and (last_menu.depth or 0) > 0
    if from_sub and t < CLOSE_SUB.dial_from + CLOSE_SUB.dial then
      local e = math.sin(math.pi / 2 * phase(t, CLOSE_SUB.dial_from, CLOSE_SUB.dial)) ^ 2
      local m = theme.menu
      home.draw(gfx, ui, { scale = SHRUNK + (1 - SHRUNK) * e, dial = e, status = false, ring = anim.clamp(1 - 3 * e, 0, 1),
                           ring_radius = m.done_radius / SHRUNK, ring_width = m.done_width / SHRUNK, ring_color = m.done_color })
      return true
    end
    local close = from_sub and CLOSE_SUB or CLOSE
    local d = anim.ease_out(phase(t, close.dial_from, close.dial))
    local closing = last_menu and t < close.dial_from + close.dial
    if not closing then
      return home.draw(gfx, ui)
    end
    local scale = SHRUNK + (1 - SHRUNK) * d
    if d > 0 then
      home.draw(gfx, ui, { scale = scale, light = d })
    else
      gfx:clear(theme.colors.menu)
    end
    local flip = 1 - anim.ease_out(phase(t, CLOSE.flip_from, CLOSE.flip))
    if from_sub then flip = d > 0 and 0 or 1 end
    if flip > 0 then
      menu.draw(gfx, ui, last_menu, { icons = flip, text = 1 - phase(t, 0, CLOSE.text), under = under(ui, scale, d),
                background = home.background(ui) })
    end
    return true
  end
end
