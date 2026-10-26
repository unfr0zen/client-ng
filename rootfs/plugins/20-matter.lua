-- Matter's page in Settings, which unfr0zen installs with Matter: the
-- pairing code nestlocal makes, and a window to pair a new controller.
-- nestlocal's files in /var/run/unfr0zen: matter.status, .code and .png
-- to read, matter.request to ask.
local RUN = "/var/run/unfr0zen/"

return {
  values = {
    ["matter.status"] = "cat " .. RUN .. "matter.status 2>/dev/null",
    ["matter.code"] = "cat " .. RUN .. "matter.code 2>/dev/null",
    ["matter.open"] = "case \"$(cat " .. RUN .. "matter.status 2>/dev/null)\" in 'Pairing open'*) echo on ;; *) echo off ;; esac",
  },
  menus = {
    { label = "Matter", style = "answers", image = "run:matter.png", prompt = "{matter.code}\n{matter.status}", items = {
      { label = "Enable pairing", info = "{matter.code}\n{matter.status}", image = "run:matter.png", wait = 900,
        run = "echo 1 > " .. RUN .. "matter.request.tmp && mv " .. RUN .. "matter.request.tmp " .. RUN .. "matter.request",
        after = "{matter.status}", unless = "matter.open" },
      { back = true },
    } },
  },
}
