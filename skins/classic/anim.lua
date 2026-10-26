-- Small animation helpers: easing, and values that glide to their targets.
local anim = {}

function anim.clamp(x, lo, hi)
  if x < lo then return lo elseif x > hi then return hi end
  return x
end

-- 0 to 1, fast then settling.
function anim.ease_out(p)
  p = anim.clamp(p, 0, 1)
  return 1 - (1 - p) ^ 3
end

function anim.ease_in_out(p)
  p = anim.clamp(p, 0, 1)
  return p < 0.5 and 4 * p * p * p or 1 - (-2 * p + 2) ^ 3 / 2
end

function anim.mix(a, b, p)
  return a + (b - a) * p
end

local function rgb(hex)
  return tonumber(hex:sub(2, 3), 16), tonumber(hex:sub(4, 5), 16), tonumber(hex:sub(6, 7), 16)
end

-- Between two #rrggbb colours.
function anim.mix_color(a, b, p)
  if a == b then return a end
  local r1, g1, b1 = rgb(a)
  local r2, g2, b2 = rgb(b)
  return string.format("#%02x%02x%02x", math.floor(anim.mix(r1, r2, p) + 0.5),
    math.floor(anim.mix(g1, g2, p) + 0.5), math.floor(anim.mix(b1, b2, p) + 0.5))
end

-- A value that follows its target over about `rate` seconds per step,
-- landing on it once within `near` (0.002 unless given).
-- `step(target, now)` returns the value and whether it is still moving.
function anim.follower(rate, near)
  near = near or 0.002
  local value, last
  return function(target, now)
    if value == nil or last == nil then
      value, last = target, now
      return value, false
    end
    local dt = anim.clamp(now - last, 0, 0.1)
    last = now
    local k = 1 - math.exp(-dt / rate)
    value = value + (target - value) * k
    if math.abs(target - value) < near then
      value = target
      return value, false
    end
    return value, true
  end
end

return anim
