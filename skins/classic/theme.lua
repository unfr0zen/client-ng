-- The Classic skin's look: colours, type and the dial's proportions.
-- Restyling usually means changing only this file.
return {
  colors = {
    heating = "#ec3100",
    cooling = "#0a6fd6",
    idle = "#000000",
    menu = "#000000",
    text = "#ffffff",
    dim = "#ffffff59",
    tick = "#ffffff4d",
    tick_active = "#ffffffe6",
    target = "#ffffff",
    current = "#ffffd9",
    current_idle = "#bdbdbd",
    rim = "#000000",
    accent = "#ec3100",
    faint = "#ffffff40",
    -- The menus.
    selected = "#00a8e1",
    icon = "#6e6e6e",
    text_soft = "#f0f0f0",
  },
  menu = {
    -- The top menu: its symbols' angles (degrees clockwise from 12 o'clock)
    -- by how many entries, otherwise spread evenly; their radius and size,
    -- the selected one larger and further in (stock 6.5's, measured on
    -- nest-qemu). Stock spaces Mode, Eco and Fan 45° apart about 12
    -- o'clock, so without Fan Mode and Eco sit at 337.5 and 22.5 (0x1a6e04).
    slots = { [6] = { 22.5, 135, 165, 195, 225, 337.5 }, [7] = { 0, 45, 135, 165, 195, 225, 315 } },
    radius = 191.7,
    radius_selected = 182.7,
    icon = 53.5,
    icon_selected = 66,
    -- Seconds for a change of selection to fade across. A choice's value
    -- gives way to its hint in a cycle of `hint_cycle` seconds, repeated
    -- while it is selected: the hint slides in between the first two
    -- fractions of it and back out between the last two (stock's selector
    -- state 3, 4000 ms, 0x5e4b54; its fractions 0x2222, 0x3bbb, 0xa221 and
    -- 0xbbb9 of 0x10000, 0x1a9c00).
    select = 0.35,
    hint_cycle = 4.0,
    hint_steps = { 0x2222 / 0x10000, 0x3bbb / 0x10000, 0xa221 / 0x10000, 0xbbb9 / 0x10000 },
    title = "#eff1e7",
    sub = "#989898",
    link = "#00bfff",
    -- A choice being edited: its options' first baseline and spacing, the
    -- further gap before Back, their colour and the highlight's pill.
    choice_top = 197,
    choice_step = 48,
    choice_back_gap = 18,
    -- A prompt under the symbol: its first baseline and spacing, and the
    -- gap from its last line to the first entry's baseline.
    prompt_top = 193,
    prompt_step = 39,
    prompt_gap = 79,
    option = "#666666",
    pill = "#00a3d9",
    -- Submenus: circles of this radius and colour, a step apart; the menu's
    -- name's baseline above them, their lines' spacing and second colour.
    circle = 144,
    carousel_step = 320,
    circle_color = "#00a1f2",
    circle_text = "#bfeaff",
    carousel_title = 68,
    carousel_line = 37,
    -- Done's ring, round the target.
    done_radius = 141,
    done_width = 9.5,
    done_color = "#646464",
    -- The fan timer's dial: an arc to either side of 12 o'clock.
    dial_radius = 206.9,
    dial_width = 24,
    dial_angle = 117.7,
    dial_track = "#999999",
    -- The lists below it.
    row = 62,
    row_text = 30,
  },
  -- Seconds for the screens' and the menus' transitions.
  transition = 0.35,
  -- Font names registered in skin.lua.
  -- Stock 6.5 sets its labels in Akkurat Bold and its large target in
  -- Medium (matched on nest-qemu).
  fonts = { number = "medium", label = "bold", body = "regular", strong = "bold" },
  -- The home screen's colours by what the system is doing. Heating and
  -- cooling deepen from `near` to `far` as the target moves up to `span`
  -- degrees Celsius above (heating) or below (cooling) the room's
  -- temperature, and stay `near` past it.
  states = {
    heating = {
      bg = { near = "#f25500", far = "#ec3100" },
      tick = { near = "#f78a00", far = "#f46700" },
      span = 5.5,
      lit = "#ffffd9", current = "#ffffd9", status = "#ffffd9",
    },
    cooling = {
      bg = { near = "#0030f1", far = "#001aec" },
      tick = { near = "#0066f6", far = "#0045f4" },
      span = 6.5,
      lit = "#d9f6ff", current = "#d9f6ff", status = "#d9f6ff",
    },
    idle = {
      bg = { near = "#000000", far = "#000000" },
      tick = { near = "#4d4d4d", far = "#4d4d4d" },
      span = 1,
      lit = "#8d8d8d", current = "#bfbfbf", status = "#bfbfbf",
    },
    target = "#ffffff",
    rim = "#000000",
  },
  dial = {
    -- Ticks from `from` to `to`, degrees clockwise from 12 o'clock, every
    -- `step`. Temperatures from `min` to `max` degrees Celsius lie on a
    -- scale `at20` degrees round for 20 degrees and `per` degrees a degree;
    -- the target's marker midway between the two ticks nearest its place,
    -- the room's bar likewise a tick before (stock 6.5's, measured on
    -- nest-qemu over 60 to 75 F and 19 to 24 C).
    min = 9,
    max = 32,
    from = -146,
    to = 146,
    step = 2,
    scale = { at20 = -12.64, per = 13.1 },
    inner = 179.33,
    outer = 232.67,
    tick_width = 2.65,
    -- The target's marker reaches further in. It and the current
    -- temperature's tick widen outwards, `k` * radius + `b` units across.
    marker_inner = 159.15,
    marker = { k = 0.0547, b = -1.52 },
    room = { k = 0.074, b = -6.06 },
    -- The current temperature's label: its centre's radius and angle from
    -- the tick, on the side away from the target.
    label_radius = 205,
    label_angle = 11,
    -- The rim round the face.
    rim = 242.4,
    rim_width = 6.8,
    -- Seconds the marker takes to sweep to a target set from elsewhere.
    sweep = 2.4,
  },
}
