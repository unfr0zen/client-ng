-- Doom's page in Settings, which unfr0zen installs with Doom: nestlocal
-- launches the game when doom.request appears in /var/run/unfr0zen. Its
-- logo, doom-logo.png, is made from the WAD and sits beside this file.
local RUN = "/var/run/unfr0zen/"

return {
  menus = {
    { label = "Doom", style = "answers", image = "plugin:doom-logo.png", image_size = { 246, 120 },
      prompt = "Doom runs full screen\nuntil you hold the button\nfor three seconds.", items = {
        { label = "Play", run = "echo 1 > " .. RUN .. "doom.request.tmp && mv " .. RUN .. "doom.request.tmp " .. RUN .. "doom.request" },
        { back = true },
      } },
  },
}
