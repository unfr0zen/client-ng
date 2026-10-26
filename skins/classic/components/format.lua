-- How the menus write temperatures and the time.
local M = {}

-- The room as the ring and the tiles show it: "68°", "21°", "21.5°".
function M.temperature(t)
  if t.unit == "F" then return string.format("%d°", math.floor(t.current + 0.5)) end
  local halves = math.floor(t.current * 2 + 0.5)
  if halves % 2 == 0 then return string.format("%d°", math.floor(halves / 2)) end
  return string.format("%.1f°", halves / 2)
end

-- A temperature in Celsius as the unit in use shows it: "5°C", "4.5°C",
-- "41°F"; `fixed` keeps the tenths ("5.0°C"), as stock's form boxes do.
function M.degrees(ui, c, fixed)
  if ui.thermostat.unit == "F" then return string.format("%d°F", math.floor(c * 1.8 + 32 + 0.5)) end
  -- Shown to the half degree, as stock's (8.89 reads 9).
  c = math.floor(c * 2 + 0.5) / 2
  if fixed or c % 1 ~= 0 then return string.format("%.1f°C", c) end
  return string.format("%d°C", c)
end

local DAYS = { "SUNDAY", "MONDAY", "TUESDAY", "WEDNESDAY", "THURSDAY", "FRIDAY", "SATURDAY" }
local clock_cache = {}

-- The day and time as Back shows them: "TUESDAY 03:00", "TUESDAY 3:00".
function M.clock(clock)
  local day = string.upper(tr(DAYS[clock.weekday + 1]))
  local key = day .. (clock.hour * 1000 + clock.minute * 10 + (clock.twelve_hour and 1 or 0))
  if clock_cache.key == key then return clock_cache.text end
  local hour = clock.hour
  if clock.twelve_hour then
    hour = hour % 12
    if hour == 0 then hour = 12 end
  end
  clock_cache.key = key
  -- Stock pads a 24-hour clock's hour: "TUESDAY 03:00".
  clock_cache.text = string.format(clock.twelve_hour and "%s %d:%02d" or "%s %02d:%02d", day, hour, clock.minute)
  return clock_cache.text
end

return M
