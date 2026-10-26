-- The settings menus. Each entry is one of:
--   a submenu:  { label = "...", items = { ... } }
--   a choice:   { label = "...", choice = "key", options = { { "value", "Label" }, ... }, default = "value" }
--               (options = "skins" offers the skins installed by folder name, the default first)
--   a toggle:   { label = "...", toggle = "key", default = false }
--   a range:    { label = "...", range = "key", min = 0, max = 100, step = 5, unit = "%", default = 50 }
--   an action:  { label = "...", action = "name" }
--   an entry:   { label = "...", enter = "key", placeholder = "...", max = 30, run = "shell command" }
--               (the keyboard; or boxes = "AA 999", question = "...": 9 a digit, A a letter, * either,
--               ? either or nothing, a space the gap; kept as the key, and given to run as $VALUE)
--   info:       { label = "...", info = "text", link = "..." }   (a page, then Back;
--               busy = "key" runs the activity arc round it while that setting is on)
--   a form:     { label = "...", form = { { label, short, key, min, max, step, off = "below"|"above", unit, default }, ... },
--                 more = "text", link = "...", more_first = true, enable = "key" }
--               (enable: a setting the form turns on, with Set and Back, and off, with Done and Disable)
--               (a field of options instead: { label, key, options = { { "value", "Label" }, ... }, default = "value" })
--   an answer:  { label = "...", set = "key", value = "value" }
--   Back:       { back = true }   (where it goes; a menu without one, or Done, ends with it)
--   Done:       { done = true }   (back to the home screen from any depth)
-- `when = "key"` shows an entry only while that setting is on (`unless`, off),
-- and `prompts = { key = "..." }` replaces the prompt while a setting is on
-- (a key "key=value" while it has that value; where several apply, the
-- first by key). A submenu styled "answers"
-- opens on the answer given and closes once one is, or its form is done.
-- A submenu styled "steps" asks its questions in turn and then leaves its
-- menu too; styled "chain", it ends back on its menu.
-- `icon` names a symbol the skin draws for the entry, `prompt` a question
-- shown with its screen, and `style` asks the skin for a presentation it
-- knows (Classic draws a choice styled "dial" as stock's fan timer, a form
-- styled "dial" as its page for a fan of more speeds, and a
-- submenu styled "list" as stock's list of answers). A choice styled "cycle"
-- (or "units", which shows the room's temperature) takes its next option
-- on each press, in place, as stock's Brightness tile does. The top menu opens on its Back entry, so pressing
-- twice opens and closes it; a submenu opens on its first entry after Back.
-- Location's countries (stock's nlLocationLayer), each region's A to Z:
-- a country's postal code follows, Ireland's county, the UAE's emirate and
-- then a city near it.
local function picks(prompt, list, action)
  local items = {}
  for _, c in ipairs(list) do
    local label, to = c[1], c[2]
    if type(to) == "table" then
      items[#items + 1] = to
    else
      items[#items + 1] = { label = label, action = action(to, label) }
    end
  end
  items[#items + 1] = { back = true }
  return items
end
local function named(iso, list, prompt)
  local l = {}
  for _, name in ipairs(list) do l[#l + 1] = { name, name } end
  return picks(prompt, l, function(to) return "location." .. iso .. "." .. to end)
end
local IRELAND = { label = "Ireland", style = "picks", prompt = "Select your county", items = named("IE", {
  "Carlow", "Cavan", "Clare", "Cork", "Donegal", "Dublin", "Galway", "Kerry", "Kildare", "Kilkenny", "Laois",
  "Leitrim", "Limerick", "Longford", "Louth", "Mayo", "Meath", "Monaghan", "Offaly", "Roscommon", "Sligo",
  "Tipperary", "Waterford", "Westmeath", "Wexford", "Wicklow" }, "Select your county") }
local function emirate(label, cities)
  return { label = label, style = "picks", prompt = "Select a nearby city", items = named("AE", cities) }
end
local UAE = { label = "UAE", style = "picks", prompt = "Select your emirate", items = picks(nil, {
  { "Abu Dhabi", emirate("Abu Dhabi", { "Abu Dhabi", "Al Ain", "Dalma", "Al Hayer", "Liwa Oasis", "Madinat Zayed",
    "Al Mirfa", "Mussafah", "Ruwais", "Al Samha", "Sila", "Al Shamkha", "Sweihan", "Al Wagan" }) },
  { "Ajman", emirate("Ajman", { "Ajman", "Al Manama", "Masfut" }) },
  { "Dubai", emirate("Dubai", { "Dubai", "Hatta", "Jebel Ali", "Jumeirah", "Al Khawaneej", "Murqquab" }) },
  { "Fujairah", emirate("Fujairah", { "Fujairah", "Dibba", "Masafi", "Zubara" }) },
  { "Ras al-Khaimah", emirate("Ras al-Khaimah", { "Ras al-Khaimah", "Airport", "Adhen", "Al Hamra", "Al Jeer", "Masafi" }) },
  { "Sharjah", emirate("Sharjah", { "Sharjah", "Al Dhaid", "Hamiryah", "Kalba", "Khor Fakkan", "Al Madam" }) },
  { "Umm al-Quwain", emirate("Umm al-Quwain", { "Umm al-Quwain", "Falaj al-Mualla" }) },
}) }
local EUROPE = { { "Austria", "AT" }, { "Belgium", "BE" }, { "Bulgaria", "BG" }, { "Croatia", "HR" },
  { "Czech Republic", "CZ" }, { "Denmark", "DK" }, { "Estonia", "EE" }, { "Finland", "FI" }, { "France", "FR" },
  { "Germany", "DE" }, { "Greece", "GR" }, { "Hungary", "HU" }, { "Ireland", IRELAND }, { "Italy", "IT" },
  { "Latvia", "LV" }, { "Lithuania", "LT" }, { "Luxembourg", "LU" }, { "Netherlands", "NL" }, { "Norway", "NO" },
  { "Poland", "PL" }, { "Portugal", "PT" }, { "Romania", "RO" }, { "Russia", "RU" }, { "Slovakia", "SK" },
  { "Slovenia", "SI" }, { "Spain", "ES" }, { "Sweden", "SE" }, { "Switzerland", "CH" }, { "UK", "GB" } }
local function region(label, countries)
  return { label = label, style = "picks", prompt = "Select your country",
           items = picks(nil, countries, function(iso) return "location." .. iso end) }
end
-- What stock confirms on choosing a Farsight view.
local FARSIGHT = "When you want to go from\nFarsight to thermostat,\npress or turn the ring."
-- Stock's built-in rooms' ids end with these two digits (where_id).
local WHERE = "00000000-0000-0000-0000-0001000000"
-- When the air filter was last changed (stock's filterreminderlayer):
-- this month or one of the five before, by name, or before them.
local FILTER_ASKED = "When did you last\nchange your filter?"
local FILTER_MONTHS = {
  { label = "This month", set = "filter.month", value = "0" },
  { label = "{filter.m1}", set = "filter.month", value = "1" },
  { label = "{filter.m2}", set = "filter.month", value = "2" },
  { label = "{filter.m3}", set = "filter.month", value = "3" },
  { label = "{filter.m4}", set = "filter.month", value = "4" },
  { label = "{filter.m5}", set = "filter.month", value = "5" },
  { label = "Before {filter.m5}", set = "filter.month", value = "6" },
}
-- Nest Sense's More info pages, each ending on its feature's status.
local SENSE_INFO = {
  learning = "Auto-Schedule creates\na schedule based on your\ntemp. adjustments.\n\nStatus: ",
  early = "Early-on reaches the\ndesired temperature at\nthe scheduled time, rather\nthan starting at that time.\n\nStatus: ",
  dry = "Cool to Dry can be\nexpensive because the\nAC may run below the\ntemperatures you set.\n\nStatus: ",
  sunblock = "Sunblock tells your\nthermostat when it's in\ndirect sunlight and adjusts\nthe temperature correctly.\n\nStatus: ",
  airwave = "Airwave turns off the AC\na few minutes early, using\nthe fan to continue cooling.\n\nStatus: ",
}

-- The fan timer's runs, and a fan's speeds by its count (stock's 0xcb60c:
-- of two the second is high), with auto for heating's and cooling's own.
local FAN_RUNS = { { "15", "15 minutes" }, { "30", "30 minutes" }, { "45", "45 minutes" },
                   { "60", "1 hour" }, { "120", "2 hours" }, { "240", "4 hours" }, { "480", "8 hours" },
                   { "720", "12 hours" }, { "1440", "1 day" }, { "4320", "3 days" }, { "7200", "5 days" },
                   { "10080", "1 week" } }
local FAN_TIMER = { { "off", "Fan" } }
for _, run in ipairs(FAN_RUNS) do FAN_TIMER[#FAN_TIMER + 1] = run end
local FAN_SPEEDS = {
  [2] = { { "stage1", "low" }, { "stage2", "high" } },
  [3] = { { "stage1", "low" }, { "stage2", "medium" }, { "stage3", "high" } },
}
-- A fan of `n` speeds: the timer's page sets a run and its speed, and
-- DONE starts it (stock's fanlayer state 4).
local function fan_timer(n)
  return { label = "Fan", icon = "fan", style = "dial", when = "fan.entry=" .. n, action = "fan.timer.start", form = {
    { label = "Run the fan for the next", key = "fan.run", options = FAN_RUNS, default = "15" },
    { label = "at/speed.", key = "fan.timer_speed", options = FAN_SPEEDS[n] },
  } }
end
-- The fan schedule's form: its duty and hours, and with `n` speeds its
-- speed under them (stock's state 2, 0x13d548).
local function fan_schedule(label, when, n)
  local form = {
    { label = "Run the fan for/Run the fan", key = "fan.duty", min = 15, max = 60, step = 15, unit = "mins", default = 60 },
    { label = "per hour from/from", key = "fan.start", min = 0, max = 23, step = 1, unit = "hour", default = 0 },
    { label = "to", key = "fan.end", min = 0, max = 23, step = 1, unit = "hour", default = 0 },
  }
  if n then form[4] = { label = "at/speed.", key = "fan.schedule_speed", options = FAN_SPEEDS[n] } end
  return { label = label, icon = "fan", style = "schedule", enable = "fan.schedule", summarize = "fan.schedule", when = when,
           summaries = { off = "Disabled", on = "Enabled" }, form = form }
end
-- Settings' Fan with `n` speeds: its schedule, and the speed heating and
-- cooling run it at, asked by what the system does (stock's states 0 and
-- 1, 0x13e264). Its tile has no summary: nlFanScreen (0x1420f4) names the
-- schedule's state only for a fan of one speed.
local function fan_settings(n)
  local speeds = { { "auto", "auto" } }
  for _, s in ipairs(FAN_SPEEDS[n]) do speeds[#speeds + 1] = s end
  return { label = "Fan", icon = "fan", style = "list", prompt = "Fan settings", when = "equipment.fan_speeds=" .. n, items = {
    fan_schedule("Schedule", nil, n),
    { label = "System fan speed", style = "speed", prompt = "What speed should the fan run when heating or cooling?",
      prompts = { ["equipment.heats_cools=heat"] = "What speed should the fan run when heating?",
                  ["equipment.heats_cools=cool"] = "What speed should the fan run when cooling?" },
      form = { { key = "fan.heat_cool_speed", options = speeds } } },
    { back = true },
  } }
end
-- Eco's and Safety Temp's fields, each page asking for those the system
-- runs.
local ECO_HEAT = { label = "Heat to", short = "Min", key = "eco.heat", min = 4.5, max = 21, step = 0.5, off = "below", default = 8.889 }
local ECO_COOL = { label = "Cool to", short = "Max", key = "eco.cool", min = 24, max = 32, step = 0.5, off = "above", default = "off" }
local SAFETY_LOW = { label = "Heat to", short = "Min", key = "safety.low", min = 4.5, max = 7, step = 0.5, off = "below", default = 4.444 }
local SAFETY_HIGH = { label = "Cool to", short = "Max", key = "safety.high", min = 24, max = 40, step = 0.5, off = "above", default = "off" }
-- Heat Pump Balance's levels (stock's heatpumpcontrollayer), each
-- confirmed as it is chosen.
local function balance(label, value, confirm)
  return { label = label, set = "sense.balance", value = value, confirm = confirm }
end
-- Early-On's, True Radiant's and Heat Pump Balance's night limit.
local MAX_DURATION = { label = "Max duration", prompt = "During the night,\nhow long should your\nthermostat preheat?", form = {
  { label = "Up to", key = "sense.early.hours", min = 0, max = 5, step = 1, unit = "hours", default = 5 },
}, more = "You can limit how long your\nthermostat will run during\nthe night while preheating to\nyour desired temperature.", more_first = true }

-- Connecting a wired Heat Link wirelessly.
local HEAT_LINK_WIRELESS = "Do you want to connect\nwirelessly to Heat Link?"
local HEAT_LINK_PAIRING = {
  { label = "Check Again", style = "answers", prompt = "Make sure the light on\nthe Heat Link is blinking\nblue. That means it's\nready to connect.", items = {
    { label = "Continue", style = "heatlink_connecting", info = "Heat Link\nconnecting", wait = 240,
      after = "Failed to connect\nwirelessly to the Heat Link." },
    { back = true },
  } },
  { label = "Reset Pairing", action = "heatlink.reset" },
  { label = "Replace Heat Link", action = "heatlink.replace" },
  { back = 2, label = "Done" },
}

-- Set time's time zones by the country's group (stock's datetimelayer,
-- 0x1210a0 over its table 0x122014): each zone's file and name.
local ZONES = {
  US = { { "Pacific/Honolulu", "Hawaii" }, { "America/Anchorage", "Alaska" }, { "America/Los_Angeles", "Pacific" },
         { "America/Denver", "Mountain" }, { "America/Phoenix", "Arizona" }, { "America/Chicago", "Central" },
         { "America/New_York", "Eastern" }, { "America/Puerto_Rico", "Atlantic" } },
  CA = { { "America/Los_Angeles", "Pacific" }, { "America/Denver", "Mountain" }, { "America/Chicago", "Central" },
         { "America/Regina", "Saskatchewan" }, { "America/New_York", "Eastern" }, { "America/Puerto_Rico", "Atlantic" },
         { "America/St_Johns", "Newfoundland" } },
  MX = { { "America/Cancun", "Eastern" }, { "America/Mexico_City", "Central" }, { "America/Mazatlan", "Pacific" },
         { "America/Tijuana", "Northwest" } },
  RU = { { "Europe/Kaliningrad", "Kaliningrad" }, { "Europe/Moscow", "Moscow" }, { "Europe/Samara", "Samara" },
         { "Asia/Yekaterinburg", "Yekaterinburg" }, { "Asia/Omsk", "Omsk" }, { "Asia/Krasnoyarsk", "Krasnoyarsk" },
         { "Asia/Irkutsk", "Irkutsk" }, { "Asia/Yakutsk", "Yakutsk" }, { "Asia/Vladivostok", "Vladivostok" },
         { "Asia/Magadan", "Magadan" }, { "Asia/Kamchatka", "Kamchatka" } },
  ME = { { "Asia/Riyadh", "Arabia Time" }, { "Asia/Dubai", "Gulf Time" } },
  AU = { { "Australia/Perth", "Western Australia" }, { "Australia/Adelaide", "Central Australia" },
         { "Australia/Darwin", "Northern Territory" }, { "Australia/Sydney", "Eastern Australia" },
         { "Australia/Brisbane", "Queensland" } },
  NZ = { { "Pacific/Auckland", "New Zealand" } },
  CN = { { "Asia/Shanghai", "China Time" } },
  EU = { { "Europe/London", "Western Europe" }, { "Europe/Berlin", "Central Europe" }, { "Europe/Tallinn", "Eastern Europe" } },
}
-- Offline, Set time's date and time, and the zone where the country has
-- any (`group` none without).
local function set_time(group)
  local form = {
    { label = "Month", key = "clock.month", min = 1, max = 12, step = 1, unit = "month" },
    { label = "Day", key = "clock.day", min = 1, max = 31, step = 1, unit = "day" },
    { label = "Year", key = "clock.year", min = 2000, max = 2099, step = 1, unit = "year" },
    { label = "Hour", key = "clock.hour", min = 0, max = 23, step = 1, unit = "hour" },
    { label = "Minute", key = "clock.minute", min = 0, max = 59, step = 1, unit = "minute" },
  }
  if ZONES[group] then form[6] = { label = "Time zone", key = "clock.zone", options = ZONES[group] } end
  return { label = "Set time", style = "datetime", prompt = "Set the date\nand time", action = "clock.set",
           when = "clock.zones=" .. group, form = form }
end

-- Where the thermostat is: a region, then its country.
local REGIONS = {
  region("Asia", { { "China", "CN" }, { "Russia", "RU" } }),
  region("Asia Pacific", { { "Australia", "AU" }, { "New Zealand", "NZ" } }),
  region("Europe", EUROPE),
  region("Middle East", { { "Saudi Arabia", "SA" }, { "UAE", UAE } }),
  region("North America", { { "Canada", "CA" }, { "Mexico", "MX" }, { "USA", "US" } }),
}

return {
  title = "Menu",
  back = "Back",
  items = {
    { label = "Eco", icon = "leaf", style = "list", prompt = "Do you want to use\nEco Temperatures?",
      prompts = { eco = "Thermostat is set to Eco.",
                  ["thermostat.mode=off"] = "This thermostat is off.\nTurn on and start using\nEco Temperatures?" }, items = {
      { label = "Start Eco", action = "eco.start", unless = "eco" },
      { label = "Stop Eco", action = "eco.stop", when = "eco" },
      { label = "More info", info = "Save energy by using Eco\nTemperatures when no\none's home. Or turn them\non manually any time.",
        link = "nest.com/eco" },
      { back = true },
    } },
    { label = "Fan", icon = "fan", style = "dial", choice = "fan.timer", default = "off", when = "fan.entry=dial",
      options = FAN_TIMER },
    fan_timer(2),
    fan_timer(3),
    { label = "Schedule", icon = "schedule", action = "schedule" },
    { label = "History", icon = "history", summarize = "history.yesterday", action = "history", when = "history.any" },
    { label = "History", icon = "history", summary = "{history.yesterday}", unless = "history.any",
      info = "Your thermostat needs more\ntime to report energy usage.", button = "Done" },
    -- Under its name, what needs looking at (Wi-Fi, internet), with a badge.
    { label = "Settings", icon = "settings", summarize = "settings.notice", items = {
      { done = true },
      { label = "Theme", choice = "display.skin", options = "skins", default = "classic" },
      -- Its tile names no value, as stock's.
      { label = "Home/Away\nAssist", icon = "homeaway", style = "answers", summarize = "away.assist", default = "on",
        summaries = { on = "", off = "" },
        prompt = "Automatically use\nEco Temperatures\nwhen no one's home?", items = {
        { label = "Stop using", set = "away.assist", value = "off", when = "away.assist" },
        { label = "Use Eco", icon = "leaf", set = "away.assist", value = "on", unless = "away.assist" },
        { label = "More info", info = "Your thermostat can save\nenergy by automatically\nadjusting the temperature\nwhen no one's home." },
        { back = true, label = "Done" },
      } },
      -- Eco and Safety Temp ask for what the system runs (stock's
      -- awaytempslayer and safetytemplayer): both ends, one, or neither.
      { label = "Eco", icon = "leaf", prompt = "Choose your Eco\nTemperatures.", when = "equipment.system=both", form = { ECO_HEAT, ECO_COOL },
        more = "Use Eco\nTemperatures whenever\nyou want to save energy.\nLook for the Leaf when\nchoosing temperatures\nto heat and cool to.", link = "nest.com/eco" },
      { label = "Eco", icon = "leaf", prompt = "Choose your Eco\nTemperature.", when = "equipment.system=heat", form = { ECO_HEAT },
        more = "Use Eco\nTemperatures whenever\nyou want to save energy.\nLook for the Leaf when\nchoosing the temperature\nto heat to.", link = "nest.com/eco" },
      { label = "Eco", icon = "leaf", prompt = "Choose your Eco\nTemperature.", when = "equipment.system=cool", form = { ECO_COOL },
        more = "Use Eco\nTemperatures whenever\nyou want to save energy.\nLook for the Leaf when\nchoosing the temperature\nto cool to.", link = "nest.com/eco" },
      { label = "Eco", icon = "leaf", info = "Heating or Cooling\nsystem not connected.", when = "equipment.system=none" },
      { label = "Safety Temp", prompt = "Choose your Safety\nTemperatures.", when = "equipment.system=both", form = { SAFETY_LOW, SAFETY_HIGH },
        more = "Safety\nTemperatures can help\nprotect your home from\nextremely cold or hot\nweather. It can ensure that\nyour pipes won't freeze or\nyour home won't overheat\neven if your thermostat is set\nto Off, or Eco Temperatures,\nor your home is set to\nAway. Your heating Safety\nTemperature is set to\n4.5°C by default, and your\ncooling Safety Temperature\nis off by default." },
      { label = "Safety Temp", prompt = "Choose your Safety\nTemperature.", when = "equipment.system=heat", form = { SAFETY_LOW },
        more = "A Safety Temperature\ncan help protect your\nhome from extremely cold\nweather. It can ensure that\nyour pipes won't freeze\neven if your thermostat is set\nto Off, or Eco Temperatures,\nor your home is set to\nAway. Your heating Safety\nTemperature is set to\n4.5°C by default." },
      { label = "Safety Temp", prompt = "Choose your Safety\nTemperature.", when = "equipment.system=cool", form = { SAFETY_HIGH },
        more = "A Safety Temperature\ncan help protect your\nhome from extremely hot\nweather. It can ensure that\nyour home won't overheat\neven if your thermostat is set\nto Off, or Eco Temperatures,\nor your home is set to\nAway. Your cooling Safety\nTemperature is off by default." },
      { label = "Safety Temp", info = "Heating or Cooling\nsystem not connected", when = "equipment.system=none" },
      fan_schedule("Fan Schedule", "equipment.fan_speeds=1"),
      fan_settings(2),
      fan_settings(3),
      { label = "Nest Sense", style = "titled", tile = "leaf", items = {
        { label = "Auto-Schedule", icon = "autoschedule", style = "answers", summarize = "sense.learning", default = "on",
          prompt = "Heating Auto-Schedule\nactive for {sense.days}.", prompts = { ["thermostat.mode=cool"] = "Cooling Auto-Schedule\nactive for {sense.days}.",
                                                                     ["thermostat.mode=heat-cool"] = "Heat • Cool Auto-Schedule\nactive for {sense.days}.",
                                                                     ["thermostat.mode=off"] = "Auto-Schedule active\nfor 0 days.",
                                                                     ["sense.learning=off"] = "Should Auto-Schedule\nlearn from your\nadjustments?" }, items = {
          -- Each answer confirmed as stock's learninglayer does: on in
          -- the mode off, turned on; else continued.
          { label = "Continue", icon = "leaf", set = "sense.learning", value = "on", confirm = "Continuing Auto-Schedule", unless = "thermostat.mode=off" },
          { label = "Continue", icon = "leaf", set = "sense.learning", value = "on", confirm = "Turning on Auto-Schedule", when = "thermostat.mode=off" },
          { label = "Off", set = "sense.learning", value = "off", confirm = "Turning off Auto-Schedule" },
          { label = "More info", info = SENSE_INFO.learning .. "On", when = "sense.learning" },
          { label = "More info", info = SENSE_INFO.learning .. "Off", unless = "sense.learning" },
        } },
        { label = "Time-to-Temperature", icon = "hourglass", button = "Done",
          info = "Time-to-Temperature\nestimates how long it\nwill take your thermostat\nto reach your target\ntemperature." },
        -- Beside True Radiant, which preheats, only with cooling.
        { label = "Early-On", icon = "earlyon", style = "answers", summarize = "sense.early", default = "off", when = "equipment.early_on",
          prompt = "Should your thermostat\npreheat and precool\nor start heating and\ncooling as scheduled?", items = {
          { label = "Start early", set = "sense.early", value = "on" },
          { label = "On schedule", icon = "leaf", set = "sense.early", value = "off" },
          MAX_DURATION,
          { label = "More info", info = SENSE_INFO.early .. "On", when = "sense.early" },
          { label = "More info", info = SENSE_INFO.early .. "Off", unless = "sense.early" },
        } },
        -- A heat pump with auxiliary heat and no dual fuel (stock's
        -- vt+0x26c); its level as heatpump_savings names it.
        { label = "Heat Pump Balance", icon = "heatpump", style = "answers", summarize = "sense.balance", default = "max-comfort", when = "equipment.balance",
          prompt = "Set the Nest Sense Heat\nPump Balance level:", items = {
          balance("Max savings", "max-savings", "Your thermostat will\nadjust temperature\nsettings for maximum\nsavings."),
          balance("Balanced", "balanced", "Your thermostat will\nadjust temperature\nsettings for balanced\ncomfort and savings."),
          balance("Max comfort", "max-comfort", "Your thermostat will\nadjust temperature\nsettings for maximum\ncomfort."),
          balance("Off", "off", "Nest Sense is off and\nwill not adjust the\ntemperature settings."),
          MAX_DURATION,
          { label = "More info", info = "Heat Pump Balance\noptimizes temperature\nsettings according to\nyour needs.\n\nStatus: {balance.status}" },
        } },
        -- Stock's with an OpenTherm boiler, which it modulates (a Heat Link's).
        { label = "True Radiant", icon = "radiant", style = "answers", summarize = "sense.radiant", default = "on", when = "equipment.heat_control=opentherm",
          prompt = "Should your\nthermostat optimize\nyour radiant system?", items = {
          { label = "Yes", icon = "leaf", set = "sense.radiant", value = "on" },
          { label = "No", set = "sense.radiant", value = "off" },
          MAX_DURATION,
          { label = "More info", info = "True Radiant works with your OpenTherm modulating boiler. It reduces temperature swings and starts heating early to help keep you comfortable." },
        } },
        { label = "Cool to Dry", icon = "dry", style = "answers", summarize = "sense.dry", default = "off", when = "equipment.dry",
          prompt = "Should your thermostat use\nadditional cooling to reduce\nhigh indoor humidity?", items = {
          { label = "Yes", set = "sense.dry", value = "on" },
          { label = "No", icon = "leaf", set = "sense.dry", value = "off" },
          { label = "More info", info = SENSE_INFO.dry .. "On", when = "sense.dry" },
          { label = "More info", info = SENSE_INFO.dry .. "Off", unless = "sense.dry" },
        } },
        { label = "Sunblock", icon = "sun", style = "answers", summarize = "sense.sunblock", default = "on",
          prompt = "Use Sunblock when\nthe sun is shining on\nthe thermostat?", items = {
          { label = "Yes", set = "sense.sunblock", value = "on" },
          { label = "No", set = "sense.sunblock", value = "off" },
          { label = "More info", info = SENSE_INFO.sunblock .. "{sunblock.status}", when = "sense.sunblock" },
          { label = "More info", info = SENSE_INFO.sunblock .. "Off", unless = "sense.sunblock" },
        } },
        { label = "Leaf", icon = "leaf", button = "Done",
          info = "The Leaf appears\nwhen you have adjusted\nthe thermostat to an\nenergy-saving temperature." },
        { label = "Airwave", icon = "airwave", style = "answers", summarize = "sense.airwave", default = "on", when = "equipment.airwave",
          prompt = "Should your thermostat use\nAirwave to save energy while\nrunning air conditioning?", items = {
          { label = "Airwave on", icon = "leaf", set = "sense.airwave", value = "on" },
          { label = "Airwave off", set = "sense.airwave", value = "off" },
          { label = "More info", info = SENSE_INFO.airwave .. "{airwave.status}", when = "sense.airwave" },
          { label = "More info", info = SENSE_INFO.airwave .. "Off", unless = "sense.airwave" },
        } },
        { done = true },
      } },
      -- With a stage that moves air through a filter.
      { label = "Reminders", style = "answers", summarize = "filter.remind", default = "off", summaries = { off = "None", on = "Air Filter" }, when = "equipment.filter",
        intro = "{filter.intro}", prompt = "Do you want your\nthermostat to remind\nyou to change filters?", items = {
        { label = "Remind me", style = "answers", unless = "filter.remind", prompt = FILTER_ASKED, items = FILTER_MONTHS },
        { label = "Remind me", set = "filter.remind", value = "on", when = "filter.remind" },
        { label = "Don't remind me", set = "filter.remind", value = "off" },
        { label = "Reset", style = "answers", when = "filter.remind", prompt = FILTER_ASKED, items = FILTER_MONTHS },
      } },
      { label = "Display", style = "list", prompt = "What should the\ndisplay show?", summarize = "display.farsight", default = "target_temp",
        summaries = { target_temp = "Farsight:\nTarget Temp", current_temp = "Farsight:\nCurrent Temp", analog_clock = "Farsight:\nAnalog Clock",
                      digital_clock = "Farsight:\nDigital Clock", weather = "Farsight:\nWeather", approach = "Wake on approach", press = "Wake on press" },
        items = {
        { heading = "Use Farsight" },
        { label = "Target Temp", set = "display.farsight", value = "target_temp", confirm = FARSIGHT },
        { label = "Current Temp", set = "display.farsight", value = "current_temp", confirm = FARSIGHT },
        { label = "Analog Clock", set = "display.farsight", value = "analog_clock", confirm = FARSIGHT },
        { label = "Digital Clock", set = "display.farsight", value = "digital_clock", confirm = FARSIGHT },
        { label = "Weather", set = "display.farsight", value = "weather", confirm = FARSIGHT, when = "net.connected" },
        -- Offline, stock's says why (Wi-Fi, or a battery too low for it)
        -- and keeps the view's check mark.
        { label = "Weather", style = "answer", tick = "display.farsight=weather", info = "{weather.offline}", unless = "net.connected" },
        { heading = "Don't use Farsight" },
        { label = "Wake on approach", set = "display.farsight", value = "approach" },
        { label = "Wake on press", set = "display.farsight", value = "press" },
        { label = "More info", info = "Use Farsight to see\ninformation like time\nor weather when your\nthermostat notices\nyou're nearby." },
        { back = true, label = "Done" },
      } },
      { label = "Brightness", choice = "display.brightness", style = "cycle", default = "auto",
        options = { { "auto", "Auto" }, { "low", "Low" }, { "medium", "Medium" }, { "high", "High" } } },
      { label = "Click Sound", choice = "display.click", style = "cycle", default = "on", options = { { "on", "On" }, { "off", "Off" } } },
      { label = "°F / °C", choice = "display.units", style = "units", default = "C", options = { { "F", "Fahrenheit" }, { "C", "Celsius" } } },
      -- Online, the time is the network's: the tile opens on its format.
      { label = "Date & Time", style = "titled", prompt = "{clock.time}", summarize = "clock.tile", when = "net.connected", items = {
        { label = "12 hour", set = "display.clock", value = "12h" },
        { label = "24 hour", set = "display.clock", value = "24h" },
        { back = true, label = "Done" },
      } },
      { label = "Date & Time", style = "titled", summarize = "clock.tile", unless = "net.connected", items = {
        { label = "Set format", style = "titled", prompt = "{clock.time}", summarize = "display.clock", default = "24h", items = {
          { label = "12 hour", set = "display.clock", value = "12h" },
          { label = "24 hour", set = "display.clock", value = "24h" },
          { back = 2, label = "Done" },
        } },
        set_time("US"), set_time("CA"), set_time("MX"), set_time("RU"), set_time("ME"), set_time("AU"), set_time("NZ"),
        set_time("CN"), set_time("EU"), set_time("none"),
        { back = true, label = "Done" },
      } },
      { label = "Language", style = "picks", ordered = true, summarize = "display.language", default = "en_US", items = {
        -- One per language installed (stock's on the unit, and any in locales/ beside these menus).
        { label = "languages", ask = "Confirm your choice:", set = "display.language", value = "", fill = "i18n.list" },
      } },
      { label = "Location", style = "titled", summarize = "location.place", prompt = "Knowing about\nyour home helps your\nthermostat work better.", items = {
        { label = "Construction", style = "steps", items = {
          { label = "Type", style = "question", prompt = "Please describe\nthis location:", items = {
            { label = "Single-Family", set = "home.type", value = "family" },
            { label = "Multi-Family", set = "home.type", value = "multi_family" },
            { label = "Apt. / Condo", set = "home.type", value = "apartment" },
            { label = "Business", set = "home.type", value = "business" },
          } },
          { label = "Thermostats", style = "question", prompt = "How many thermostats\nare in this home?", items = {
            { label = "Only this one", set = "home.thermostats", value = "1" },
            { label = "2", set = "home.thermostats", value = "2" },
            { label = "3", set = "home.thermostats", value = "3" },
            { label = "4", set = "home.thermostats", value = "4" },
            { label = "More", set = "home.thermostats", value = "more" },
          } },
          { label = "Built", style = "question", prompt = "When was this home\nbuilt or last remodeled?", items = {
            { label = "2010s", set = "home.built", value = "2010" },
            { label = "2000s", set = "home.built", value = "2000" },
            { label = "1990s", set = "home.built", value = "1990" },
            { label = "1980s", set = "home.built", value = "1980" },
            { label = "1970s", set = "home.built", value = "1970" },
            { label = "1960s", set = "home.built", value = "1960" },
            { label = "1950s", set = "home.built", value = "1950" },
            { label = "1940s", set = "home.built", value = "1940" },
            { label = "Before 1940", set = "home.built", value = "earlier" },
            { label = "I Don't Know", set = "home.built", value = "unknown" },
          } },
        } },
        -- A place known (the weather's), stock asks first whether it is
        -- where the thermostat is.
        { label = "Location", style = "answers", prompt = "Is this thermostat located in", detail = "{location.asked}?",
          when = "location.known", items = {
          { label = "Yes", action = "location.keep" },
          { label = "No", style = "picks", prompt = "Where are you?", items = REGIONS },
        } },
        { label = "Location", style = "picks", prompt = "Where are you?", unless = "location.known", items = REGIONS },
        { done = true },
      } },
      { label = "Network", style = "network", summarize = "net.ssid", prompt = "{net.status}", items = {
        { label = "networks", set = "net.join", value = "", fill = "net.list" },
        { label = "Type network name", action = "net.type" },
        { label = "Disconnect", action = "net.disconnect", when = "net.connected" },
        { label = "Do not connect", action = "net.disconnect", unless = "net.connected" },
        { back = true, label = "Done" },
      } },
      -- Offline, stock's templocklayer asks for Wi-Fi first.
      { label = "Lock", tile = "lock", info = "To activate locking,\nconnect the thermostat\nto Wi-Fi.", button = "Done", unless = "net.connected" },
      { label = "Lock", tile = "lock", style = "answers", prompt = "You need to add this\nthermostat to the Nest\napp to activate locking.", when = "net.connected", items = {
        { label = "Add", style = "logo", icon = "google", info = "", wait = 10, home = true },
        { back = true, label = "Done" },
      } },
      { label = "Placement", style = "picks", prompt = "Where is your\nthermostat?", summarize = "placement.where",
        default = WHERE .. "0c", summaries = { [WHERE .. "04"] = "Attic", [WHERE .. "01"] = "Basement", [WHERE .. "09"] = "Bathroom", [WHERE .. "0d"] = "Bedroom", [WHERE .. "03"] = "Den", [WHERE .. "10"] = "Dining Room", [WHERE .. "06"] = "Downstairs", [WHERE .. "00"] = "Entryway", [WHERE .. "0b"] = "Family Room", [WHERE .. "07"] = "Garage", [WHERE .. "15"] = "Guest House", [WHERE .. "1a"] = "Guest Room", [WHERE .. "02"] = "Hallway", [WHERE .. "08"] = "Kids Room", [WHERE .. "0a"] = "Kitchen", [WHERE .. "0c"] = "Living Room", [WHERE .. "05"] = "Master Bedroom", [WHERE .. "0e"] = "Office", [WHERE .. "0f"] = "Upstairs" }, items = {
        { label = "Attic", set = "placement.where", value = WHERE .. "04" },
        { label = "Basement", set = "placement.where", value = WHERE .. "01" },
        { label = "Bathroom", set = "placement.where", value = WHERE .. "09" },
        { label = "Bedroom", set = "placement.where", value = WHERE .. "0d" },
        { label = "Den", set = "placement.where", value = WHERE .. "03" },
        { label = "Dining Room", set = "placement.where", value = WHERE .. "10" },
        { label = "Downstairs", set = "placement.where", value = WHERE .. "06" },
        { label = "Entryway", set = "placement.where", value = WHERE .. "00" },
        { label = "Family Room", set = "placement.where", value = WHERE .. "0b" },
        { label = "Garage", set = "placement.where", value = WHERE .. "07" },
        { label = "Guest House", set = "placement.where", value = WHERE .. "15" },
        { label = "Guest Room", set = "placement.where", value = WHERE .. "1a" },
        { label = "Hallway", set = "placement.where", value = WHERE .. "02" },
        { label = "Kids Room", set = "placement.where", value = WHERE .. "08" },
        { label = "Kitchen", set = "placement.where", value = WHERE .. "0a" },
        { label = "Living Room", set = "placement.where", value = WHERE .. "0c" },
        { label = "Master Bedroom", set = "placement.where", value = WHERE .. "05" },
        { label = "Office", set = "placement.where", value = WHERE .. "0e" },
        { label = "Upstairs", set = "placement.where", value = WHERE .. "0f" },
        { label = "custom rooms", set = "placement.where", value = "", fill = "placement.customs" },
        { label = "Add custom", action = "keyboard.custom" },
        { label = "Add label", enter = "device.label", placeholder = "Label" },
        { back = true, label = "Done" },
      } },
      { label = "Equipment", style = "system", title = "Your System", prompt = "{tech.system}", summarize = "equipment.notice", items = {
        { label = "Continue", style = "choices", title = "Equipment Settings", items = {
          { back = 2, label = "Done" },
          -- Stock's Heat Link pages for a wired Heat Link (heat_link_connection 1),
          -- on a Heat Link unit only (g2 0xbb9c0).
          { label = "Heat Link", style = "heatlink", title = "Heat Link\nconnected", prompt = "wired only", when = "equipment.heat_link", items = {
            { label = "Wireless", style = "answers", prompt = HEAT_LINK_WIRELESS, items = HEAT_LINK_PAIRING },
            -- Stock asks once more before leaving a wired-only Heat Link.
            { label = "Done", style = "answers", prompt = "You set up Heat Link with\na wired-only connection and\nit won't be able to receive\nsoftware updates. Set up\na wireless connection?", items = {
              { label = "Set up", style = "answers", prompt = HEAT_LINK_WIRELESS, items = HEAT_LINK_PAIRING },
              { back = 2, label = "Continue" },
            } },
          } },
          -- Stock's conventional entries (equipmentlayer), each with its
          -- equipment; their pages are the engine's (equipment.rs).
          { label = "Heat Type", style = "chain", when = "equipment.heat_type", items = {
            { label = "Type", style = "question", prompt = "What type of heating\ndo you have?", items = {
              { label = "FORCED AIR", set = "equipment.heat_delivery", value = "forced-air" },
              { label = "RADIATORS", set = "equipment.heat_delivery", value = "radiator" },
              { label = "IN-FLOOR RADIANT", set = "equipment.heat_delivery", value = "in-floor-radiant" },
              { label = "ELEC. BASEBOARD", set = "equipment.heat_delivery", value = "electric-strip" },
              { label = "I DON'T KNOW", set = "equipment.heat_delivery", value = "unknown" },
            } },
            { label = "Fuel", style = "question", prompt = "What is the fuel source\nfor your heating?", items = {
              { label = "GAS", set = "equipment.heat_source", value = "gas" },
              { label = "ELECTRIC", set = "equipment.heat_source", value = "electric" },
              { label = "OIL", set = "equipment.heat_source", value = "oil" },
              { label = "PROPANE (LP)", set = "equipment.heat_source", value = "lp" },
              { label = "GEOTHERMAL", set = "equipment.heat_source", value = "geothermal" },
              { label = "I DON'T KNOW", set = "equipment.heat_source", value = "unknown" },
            } },
          } },
          { label = "Heat Pump", action = "equipment.heatpump", when = "equipment.heat_pump" },
          { label = "Dual Fuel", action = "equipment.dualfuel", when = "equipment.dual_fuel_system" },
          { label = "Humidifier", action = "equipment.humidity", when = "equipment.humidifier" },
          { label = "Dehumidifier", action = "equipment.humidity", when = "equipment.dehumidifier" },
          { label = "Emerg. Heat", choice = "emergency", style = "answers", when = "equipment.emergency_heat", default = "off",
            prompt = "Turn on emergency heat?\n\nEmergency heat won't use\nyour heat pump, so it\ncan be expensive.",
            options = { { "off", "Leave off" }, { "on", "Turn on" } } },
          { label = "* Wire Type", action = "equipment.star", when = "equipment.star" },
          { label = "Pro Setup", style = "answers", prompt = "Incorrect settings will cause\nunexpected HVAC activity\nor equipment damage.", items = {
            { back = true },
            { label = "Continue", action = "equipment.setup" },
          } },
          { back = true },
        } },
        -- Stock's system test asks for Wi-Fi, then weather, first.
        { label = "Test", style = "answers", when = "test.blocked", prompts = {
            ["test.gate=wifi"] = "Your thermostat is not\nconnected to Wi-Fi. Once\nconnected, you'll be able\nto run a test of your\nsystem from settings.",
            ["test.gate=weather"] = "Your thermostat currently\ndoes not have access to\nweather information. Once\navailable, you will be able\nto run a test of your system\nfrom within settings." },
          items = { { back = true, label = "Continue" } } },
        { label = "Test", action = "equipment.test", unless = "test.blocked" },
      } },
      { label = "Nest Pro", tile = "pro", style = "answers", prompt = "Want to add your\ncontact information?\nEnter your Nest Pro ID.", items = {
        { label = "Enter Pro ID", action = "pro.enter" },
        { back = true, label = "Cancel" },
      } },
      { label = "Software", icon = "software", style = "titled", prompt = "Version {tech.software}",
        prompts = { ["tech.software.dated"] = "Version {tech.software}\n{tech.software.updated}" }, summarize = "tech.software", items = {
        { label = "Update", style = "wait", info = "Your thermostat's\nsoftware is up to date.", wait = 4, when = "net.connected" },
        { label = "Update", info = "Unable to check for a\nsoftware update.\n\nYour thermostat isn't\nconnected to Wi-Fi.", unless = "net.connected" },
        { back = true, label = "Done" },
      } },
      { label = "Technical\nInfo", style = "titled", items = {
        { label = "POWER", section = { { "Battery", "{tech.battery}" }, { "{tech.v1.label}", "{tech.v1}" }, { "{tech.vin.label}", "{tech.vin}" }, { "{tech.iin.label}", "{tech.iin}" } } },
        -- HEAT LINK only on a Heat Link unit; stock passes over it until a
        -- Heat Link is paired, then gives the entry key it was paired with.
        { heading = "HEAT LINK", when = "tech.heatlink=heading" },
        { label = "HEAT LINK", section = { { "Entry Key", "{tech.heatlink.key}" } }, when = "tech.heatlink=key" },
        -- NEST WEAVE only on a Heat Link unit.
        { label = "NEST WEAVE", section = { { "Fabric ID", "n/a" }, { "Status", "Not connected." } }, when = "equipment.heat_link" },
        { label = "NETWORK", section = { { "Wi-Fi", "{tech.wifi}" }, { "Interface", "wlan0" }, { "IPv4", "" }, { "Address", "{tech.ipv4}" },
          { "Subnet mask", "{tech.mask}" }, { "Router", "{tech.router}" }, { "IPv6", "" }, { "Address", "{tech.ipv6}" },
          { "Prefix length", "{tech.prefix6}" }, { "Router", "{tech.router6}" }, { "Name servers", "{tech.dns}" },
          { "Search domains", "{tech.search}" }, { "Service Environment", "local" }, { "Last data upload", "{tech.upload}" },
          { "Last cloud comm", "{tech.cloud}" },
          { "MAC address", "{tech.mac}" } } },
        { label = "SENSORS", section = { { "Current temp.", "{tech.current}" }, { "Target temp.", "{tech.target}" },
          { "Rel. humidity", "{tech.humidity}" }, { "Activity", "{tech.activity}" } } },
        -- EQUIPMENT on a unit outside the Heat Link gate (stock's 0x1bc930).
        { label = "EQUIPMENT", section = { { "Equipment sensed on", "{tech.sensed}" }, { "{tech.inserted.label}", "{tech.inserted}" } },
          unless = "equipment.heat_link" },
        { label = "DISPLAY", section = { { "Where", "{tech.where}" }, { "Software version", "{tech.software}" }, { "Model", "{tech.model}" },
          { "Serial number", "{tech.serial}" }, { "{tech.zone.label}", "{tech.zone}" }, { "Unlocked", "True" } } },
        { label = "BASE", section = { { "Software version", "{tech.base.software}" }, { "Model", "{tech.base.model}" },
          { "Serial number", "{tech.base.serial}" } } },
        { done = true },
      } },
      { label = "Legal Info", style = "legal", button = "Done",
        info = "Use of the thermostat\nconnected to Wi-Fi is\nsubject to the Privacy\nPolicy: g.co/nest/privacy\n\nWeather data is provided\nby The Weather Channel™\n\nPostal code data\nfrom GeoNames,\nwww.geonames.org\n\n{legal.saying}" },
      { label = "Reset", style = "picks", prompt = "What would you\nlike to reset?", items = {
        { label = "Restart", style = "confirm", prompt = "Turn the ring to restart\nyour thermostat without\nresetting information.", items = {
          { back = true, label = "Cancel" },
          { label = "Restart", action = "restart" },
        } },
        -- Auto-Schedule while it learns, else the schedule (stock's
        -- resetlayer); each reset counts down first.
        { label = "Auto-Schedule", style = "answers", prompt = "Reset to erase the schedule\nand restart Auto-Schedule.", when = "sense.learning", items = {
          { back = true, label = "Cancel" },
          { label = "Reset", action = "reset.autoschedule" },
        } },
        { label = "Schedule", style = "answers", prompt = "Reset to create a\nnew schedule.", unless = "sense.learning", items = {
          { back = true, label = "Cancel" },
          { label = "Reset", style = "answers", prompt = "Do you want to start over\nwith a basic schedule or\njust clear the current one?", items = {
            { label = "Basic", action = "reset.schedule.basic" },
            { label = "Clear", action = "reset.schedule.clear" },
            { back = 2, label = "Cancel" },
          } },
        } },
        { label = "Away", style = "answers", prompt = "Reset Away if you've\nmoved your thermostat or\nchanged your daily routine.", items = {
          { back = true, label = "Cancel" },
          { label = "Reset", action = "reset.away" },
        } },
        { label = "Network", style = "answers", prompt = "Reset to erase all\nnetwork information.", items = {
          { back = true, label = "Cancel" },
          { label = "Reset", style = "confirm", prompt = "Turn the ring to reset\nnetwork settings.", items = {
            { back = true, label = "Cancel" },
            { label = "Reset", action = "reset.network" },
          } },
        } },
        { label = "All Settings", style = "answers", prompt = "This will erase all of\nyour settings and return\nto factory defaults.", items = {
          { back = true, label = "Cancel" },
          { label = "Reset", style = "confirm", prompt = "Turn the ring to reset\nyour thermostat to\nfactory defaults.", items = {
            { back = true, label = "Cancel" },
            { label = "Reset", action = "reset.all" },
          } },
        } },
        { back = true, label = "Cancel" },
      } },
      { done = true },
    } },
    { back = true },
    { label = "Mode", icon = "mode", choice = "thermostat.mode", default = "heat",
      options = { { "heat", "Heat" }, { "cool", "Cool" }, { "heat-cool", "Heat • Cool" }, { "off", "Off" } } },
  },
}
