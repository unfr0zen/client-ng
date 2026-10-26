-- Zigbee's page in Settings, which unfr0zen installs with Zigbee: the
-- network nestlocal is on, and joining another. nestlocal's files in
-- /var/run/unfr0zen: zigbee.status and .info to read, zigbee.request to ask.
local RUN = "/var/run/unfr0zen/"
local PAIRING = { label = "Enable pairing", info = "{zigbee.status}", wait = 30, after = "{zigbee.status}",
                  run = "echo 1 > " .. RUN .. "zigbee.request.tmp && mv " .. RUN .. "zigbee.request.tmp " .. RUN .. "zigbee.request" }

return {
  values = {
    ["zigbee.status"] = "cat " .. RUN .. "zigbee.status 2>/dev/null",
    ["zigbee.info"] = "cat " .. RUN .. "zigbee.info 2>/dev/null",
    ["zigbee.paired"] = "case \"$(cat " .. RUN .. "zigbee.status 2>/dev/null)\" in 'Paired'*) echo on ;; *) echo off ;; esac",
  },
  menus = {
    { label = "Zigbee", style = "answers", prompt = "{zigbee.info}\n\n{zigbee.status}", items = {
      { label = "Enable pairing", style = "answers", when = "zigbee.paired",
        prompt = "This leaves the Zigbee\nnetwork and forgets it.", items = {
          { back = true, label = "Cancel" },
          PAIRING,
        } },
      { label = PAIRING.label, info = PAIRING.info, wait = PAIRING.wait, run = PAIRING.run, after = PAIRING.after,
        unless = "zigbee.paired" },
      { back = true },
    } },
  },
}
