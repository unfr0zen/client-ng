-- The menus' components: each is a table of parameters with methods that
-- draw with them. A variant extends one with the parameters it changes and
-- inherits the rest, and its methods, from it:
--
--   local Pill = require("classic:components.pill")
--   local Square = Pill:extend({ radius = 4, color = "#ff8800" })
--
-- A parameter that is a table is replaced whole, not merged.
local Component = {}
Component.__index = Component

-- A variant of this component with `params` over its own.
function Component:extend(params)
  local variant = {}
  for k, v in pairs(params or {}) do variant[k] = v end
  variant.__index = variant
  return setmetatable(variant, self)
end

-- The components as the menus use them now (a skin may have replaced
-- some): a component draws its parts with these unless given its own.
function Component.registry()
  return require("components")
end

return Component
