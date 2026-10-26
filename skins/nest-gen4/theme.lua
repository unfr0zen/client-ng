-- The 4th gen's colours and the home screen's proportions.
return {
  colors = {
    bg = "#000000",
    text = "#ffffff",
    secondary = "#9aa0a6",
    dim = "#5f6368",
    track = "#2c2d30",
    track_off = "#242526",
    room = "#e8eaed",
    -- The scale's ticks off the lit stretch.
    tick = "#9da0c8",
    -- What the system is doing: its target's tick, symbol and words, and
    -- the light behind the number.
    heat = "#f4aabd",
    heat_glow = "#e0707f",
    cool = "#8ab4f8",
    cool_glow = "#545ee6",
    leaf = "#4fc36b",
  },
  -- The ring open at the bottom, from `from` to `to` degrees clockwise
  -- from 12 o'clock, `width` units thick: the thermostat's lowest
  -- temperature at `from`, its highest at `to`.
  ring = { radius = 216, width = 2.5, from = -138, to = 138, room_dot = 4 },
  -- The scale the wheel shows: a tick every `step` degrees, `length` units
  -- in from the ring, `width` thick at `dim` opacity, `lit_width` thick
  -- where lit; the room's and the targets' bars `bar` units longer and
  -- `bar_width` thick; the room's number and the symbols stand `clear`
  -- units further in while it shows, the symbols at `under` opacity while
  -- under the room's number, level with it within `level` units across.
  ticks = { step = 1.5, length = 16, width = 1.1, dim = 0.55, lit_width = 2, bar = 6, bar_width = 3.5, clear = 12, under = 0.3, level = 40 },
  -- The light behind the number: its radius and the rings it is drawn in.
  glow = { radius = 170, steps = 96 },
  -- The number's size and baseline (the range's in heat-cool), the
  -- half's size against it and a digit's cell against the widest digit,
  -- the words' under it, the leaf's centre.
  number = { size = 168, base = 300, range_size = 108, range_base = 283, half = 0.34, cell = 0.86 },
  label = { size = 25, y = 362 },
  leaf = { y = 418, size = 30 },
  -- Seconds for the motion: the number's digits to roll after a pause
  -- (`roll`; at the wheel's pace while it turns, no quicker than
  -- `roll_min`), the target's and range's marks to glide (an exponential
  -- follow's time constant), the room's, the light to fade and crossfade,
  -- the scale to stay after the wheel stops (`hold`) and then fade out
  -- (`fade`), and the ring to draw in coming home, from `intro_from` after
  -- the screen changed (as the menus fold away).
  motion = { roll = 0.3, roll_min = 0.06, dot = 0.08, room = 0.25, glow = 0.6, hold = 1.5, fade = 0.4, intro = 0.6, intro_from = 0.15 },
  -- Farsight from afar: the number this many times larger.
  far = 1.4,
}
