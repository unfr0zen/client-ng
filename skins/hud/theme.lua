-- One Dark's palette and the display's proportions.
return {
  colors = {
    -- One Dark, pushed apart: its greys darker, its accents its vivid
    -- variants and its foreground near white.
    bg = "#0b0d10",
    face = "#13161b",
    gutter = "#1c2027",
    rule = "#353b45",
    comment = "#7f848e",
    fg = "#d7dae0",
    bright = "#ffffff",
    red = "#ff6b7a",
    orange = "#ffa95e",
    yellow = "#ffd479",
    green = "#a8e66e",
    cyan = "#4fe0f0",
    blue = "#5cb8ff",
    purple = "#e070f0",
  },
  -- The gauge: segments `step` degrees apart from `from` to `to` (clockwise
  -- from 12 o'clock), `fill` of each step lit, between radii `inner` and
  -- `outer`.
  gauge = { from = -135, to = 135, step = 6, fill = 0.7, inner = 170, outer = 202 },
  -- The bezel's ticks: `count` round the face, every `major`th longer.
  bezel = { count = 120, major = 10, inner = 214, minor = 222, outer = 230 },
  -- The ring that turns while the system runs: its radius, dashes and
  -- degrees a second.
  spinner = { radius = 154, dashes = 3, length = 50, width = 3, speed = 90 },
  -- Spacing of the display's scan lines.
  scan = 4,
}
