-- Classic's menu components, by name. The menus draw with whatever is here
-- when they draw, so a skin borrowing them restyles a component everywhere
-- by replacing it with a variant:
--
--   local components = require("classic:components")
--   components.pill = components.pill:extend({ color = "#ff8800" })
return {
  ring = require("components.ring"),
  text = require("components.text"),
  pill = require("components.pill"),
  button = require("components.button"),
  field_box = require("components.field_box"),
  form_row = require("components.form_row"),
  confirm_ring = require("components.confirm_ring"),
  dial = require("components.dial"),
  info_page = require("components.info_page"),
  keyboard = require("components.keyboard"),
  tile = require("components.tile"),
  list_row = require("components.list_row"),
  list = require("components.list"),
  prompt = require("components.prompt"),
  -- A heading among a list's rows (Display's "Use Farsight").
  heading = require("components.text"):extend({ size = 32 }),
  carousel = require("components.carousel"),
}
