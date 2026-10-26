# client-ng

> [!WARNING]
> 🚧 **Work in progress.** 🚧 This project is under active development and is not ready for use. Features are incomplete, behaviour may change without notice, and it should not be relied on to control real HVAC equipment.

The thermostat's UI, replacing Nest's nlclient. A Rust engine owns the display, wheel, button, settings, menus, HVAC control and backplate link; a Lua **skin** draws what it shows (Settings > Theme). What stock still has and the client lacks: `TODO.md`. Testing workflow: `TESTING.md`.

## Layout

| Path | What |
| --- | --- |
| `src/engine.rs` | what the wheel and button do on each screen; the view skins draw |
| `src/menu.rs` | menu tree and navigation |
| `src/skin.rs` | sandboxed Lua host, `gfx` API, menus reader |
| `src/hvac/`, `src/backplate/` | HVAC control, backplate link |
| `src/net.rs` | nestlocal link |
| `src/log.rs` | the log |
| `src/platform/gen3/` | GPU painter (femtovg on GLES2), batched quads |
| `src/platform/gen2/` | CPU painter, framebuffer |
| `src/platform/` | EGL, evdev, piezo, backlight, system bus (Linux) |
| `src/platform/desktop/` | desktop build: round window, input, control server, backplate.py |
| `skins/` | skins; `classic/` is stock's design, the others reuse its menus |
| `config/menus.lua`, `config/plugins/`, `config/locales/` | menus, menu plugins, translations |
| `build.sh` | ARM build, staged in `build/client-ng/` |
| `tools/nest.py` | run, drive, record and compare on nest-qemu |
| `tools/desktop-mcp.py` | MCP server for the desktop build |

## Build

From unfr0zen-dev's root (client-ng is a submodule and workspace member):

```sh
client-ng/build.sh                   # ARMv7, glibc 2.19, in the unfr0zen-build image
CLIENT_NATIVE=1 client-ng/build.sh   # with this host's cargo-zigbuild and zig
```

GPU libraries, libdbus and FreeType load at run time: nothing proprietary is needed to build. The binary keeps no build-host paths.

Check every skin and the menus without a display:

```sh
cargo run -- --check --skins skins --menus config/menus.lua
```

## Desktop

```sh
cargo run --release --features desktop
cargo run --release --features desktop -- --no-settings --display-idle 600
cargo run --release --features desktop -- --headless --control 127.0.0.1:8760   # no window, no focus
```

Use `--release` (debug draws ~27 ms a frame). `--pixels 480` draws the face at the gen 3's resolution. `--headless` opens no window: the face is drawn at 480 pixels and seen and driven only through the control server. `--model NAME` sets the unit's model (nlmodel): the default Display-3.4 (nest-qemu's) is a Heat Link unit; `--model Display-2.7` is a gen 3 with conventional wiring, for Equipment Settings' conventional pages.

| Dependency | |
| --- | --- |
| Rust stable, a C compiler | LuaJIT builds from source |
| Python 3 | backplate.py, MCP server |
| nest-qemu checkout | its `ui/backplate.py`; at `$NEST_QEMU` or beside this repo as `nest-qemu`/`qemu`. Else stock's simulator |
| FreeType 2.6 (optional) | `tools/freetype.sh` → `.work/freetype`: text as stock's to the pixel |
| Unit files (optional) | `tools/nest.py unit-files` → `.work/unit/share` (or `$NEST_UNIT_SHARE`): Akkurat, pictures, stock strings |
| ffmpeg (optional) | MCP recordings |
| Linux runtime | X11 or Wayland, OpenGL/EGL; a compositor for the see-through window |

| Input | |
| --- | --- |
| Mouse wheel notch, 80 px of trackpad, arrow keys | one step / one entry / one tile |
| Left click on the lens, Space, Enter | the button |
| Drag the steel ring | move the window |
| Ctrl+Q | quit |

- Skins, menus and plugins reload on save.
- Settings: `target/release/settings.json`, or `--settings FILE`.
- No cloud unless `--cloud URL`.
- The backplate is backplate.py over TCP (`--simulate-backplate` for stock's simulator).
- Restart, resets, clock setting and system tools only log.

### Control server

`--control HOST:PORT` (default `127.0.0.1:8095`), same paths as nest-qemu's ui.py:

| Endpoint | |
| --- | --- |
| `GET /screen.png` | last frame, 480×480 |
| `GET /screen` | frames as server-sent events (base64 PNG) |
| `GET /state` | wheel position, backplate |
| `POST /button/press`, `/button/release` | button |
| `POST /wheel/turn` | `{"delta": detents}` (ring physics) or `{"notches": n}` (mouse wheel) |
| `POST /menu/open` | `{"path": [labels], "at": n}` |
| `POST /cloud` | `{"key": {fields}}` as nestlocal sends: `shared.SERIAL`, `device.SERIAL`, `weather` |
| `POST /backplate` | backplate.py control (room, wiring, presence, light); `{}` reads it |
| `POST /approach` | someone comes near (the thermopile or near PIR; backplate.py's presence is the PIR) |
| `POST /quit` | stop |

`tools/desktop-mcp.py` (`client-ng-desktop` in `.mcp.json`) wraps it: `launch`, `stop`, `status`, `logs`, `screenshot`, `capture_frames`, `press_button`, `turn_wheel`, `input_sequence`, `record_start`/`record_stop`, `backplate`, `cloud`. `launch` runs headless unless `headless: false`, builds into `target/desktop` and keeps logs, settings and recordings in `.work/desktop/`. The display sleeps after 40 s: a black screenshot means asleep.

## nest-qemu

```sh
client-ng/tools/nest.py qemu start                   # boot (own NAND copy in .work/client-qemu/)
client-ng/build.sh && client-ng/tools/nest.py run    # deploy, replace nlclient; `run -- ARGS` for client args
client-ng/tools/nest.py screenshot | record DIR | compare A B
client-ng/tools/nest.py wheel 3 | click | watch | logs -f
client-ng/tools/nest.py stop                         # nlclient comes back
client-ng/tools/nest.py qemu stop
```

Guest login root/nest. `qemu start --mqtt HOST:PORT` relays nestlocal's MQTT to a broker.

## Log

stderr; on the unit `/tmp/client-ng.log`, new each start. Format `HH:MM:SS.mmm LEVEL area: message`, coloured on a terminal (not with `NO_COLOR`).

| `--log` / `CLIENT_NG_LOG` | |
| --- | --- |
| `error`, `warn` | failures |
| `info` (default) | changes: screen, setting, system state, links |
| `debug` | input, decisions with reasons, requests/replies |
| `trace` | per frame, every backplate frame |

`--fps` logs frame rates and timings. Secrets are never logged; files are named relative to the working directory.

## Skins

A folder in `skins/` whose `skin.lua` returns:

```lua
return {
  name = "Classic", api = 1, design = 480,
  fonts = { regular = { "unit:AkkuratNest-Regular.ttf", "fonts/Inter-Regular.ttf" } },
  draw = function(gfx, ui) ... return animating end,
}
```

- **Fonts:** a skin file, `other:FILE` (another skin's), `unit:NAME` (`/nestlabs/share/fonts`), or a list (first found).
  Bundled fonts contain Latin-1 and characters used by the sources and translations, with unhinted outlines. To regenerate them with fontTools, run `python3 client-ng/tools/skin-fonts.py ORIGINAL_SKINS_DIRECTORY`, where the directory contains the original fonts at the same paths as `skins/`. Use the original fonts when adding characters; subsetting cannot recover removed glyphs.
- **`draw`:** runs after input, on file changes, while it returns `true`, and once at the time a frame asked for with `gfx:wake`.
- **Sandbox:** LuaJIT (Lua 5.1 + `bit`) with table/string/math, `log`, `clock()`, and `require` limited to the skin's own and `require("other:module")`. No settings writes, no file access.
- **Failures:** a skin that errors, fails to load or takes >250 ms a frame is replaced by a fallback face.
- **Reusing Classic:** other skins take Classic's menus with `require("classic:screens")(home)`, where `home` has `draw(gfx, ui, look)` (`look.scale`, `look.light`, `look.squash`) and `background(ui)`. They must register Classic's `regular`, `medium` and `bold` fonts.
- **Speed:** on nest-qemu every shape is costly. Put static parts in `layer`/`paint_layer`.

`ui`:

| Field | |
| --- | --- |
| `now` | seconds since start |
| `screen` | `"home"`, `"menu"`, … |
| `face` | `size` (design units), `px`, `scale` |
| `thermostat` | `target`, `current`, `min`, `max`, `low`, `high`, `unit`, `mode`, `hvac`, `humidity`, `can_heat`, `can_cool`, `range`, `range_editing`, `safety`, `press_to`, `lockout`, `system_issue`; `forecast` (three `{temp, time}`) while someone is near Farsight's weather |
| `menu` | when open: `title`, `path`, `depth`, `selected`, `editing`, `icon`, `style`, `prompt`, `pending`, `items` |
| `clock` | `weekday`, `hour`, `minute`, `twelve_hour`; `weekday_date`, `weekday_day` while someone is near Farsight |
| `changed` | `screen`, `target`, `menu`, `turned`; `near`, when Farsight's near view comes in, `leave`, when its clocks or weather start fading out after an input (in `now`'s seconds) |

`gfx` (design units; angles clockwise from 12 o'clock; colours `#rgb`/`#rrggbb`/`#rrggbbaa`):

| Method | |
| --- | --- |
| `circle`, `ring`, `arc`, `line`, `lines`, `rect`, `polygon` | shapes (`lines` for many segments in one stroke) |
| `text(s, x, y, style)`, `measure(s, style)`, `has_glyphs(s, style)` | `font`, `size`, `color`, `align`, `baseline`, `spacing`, `outline`, `outline_width` |
| `image(path, x, y, w, h, alpha)` | PNG/JPEG from the skin |
| `save`, `restore`, `translate`, `rotate`, `scale`, `alpha` | transform |
| `clear(c)` | whole panel, cheapest background |
| `layer(name, key, fn, w, h)` | offscreen layer, redrawn only when `key` changes |
| `paint_layer(name, c, clip)` | paint a layer, tinted, optionally clipped |
| `wake(s)` | draw again in `s` seconds without animating meanwhile (the soonest asked in a frame) |

Displays: skins draw upright in `design` units; the engine scales and rotates to the panel (gen 3: 480×640 mounted sideways). `--face`, `--origin`, `--turns` override for unknown panels.

Fonts are SIL OFL (`OFL-*.txt` in each skin). Classic uses stock's Akkurat from the unit (not shipped), Inter where it is absent.

More skins, each installed beside these: [client-ng-skins](https://github.com/unfr0zen/client-ng-skins).

## Menus

`config/menus.lua` returns the tree. Each entry is one of:
- a submenu (`items`)
- `choice` (`options`)
- `toggle`
- `range` (`min`, `max`, `step`, `unit`)
- `action`
- `back = true`
- `done = true`

Choices, toggles and ranges name a settings key and a `default`; `style` passes to the skin. Menus close after 30 s idle.

**Plugins:** each `config/plugins/*.lua`, and each the system drops into `/etc/unfr0zen/plugins` (`--plugins DIR`), applies in name order across both, a dropped-in file replacing a bundled one of its name; each plugin's entries follow the one before's, and changes reload. unfr0zen installs a page per feature it enables from `rootfs/plugins/` (`10-unfr0zen.lua`, `20-matter.lua`, `30-zigbee.lua`, `40-doom.lua`). A plugin returns:

| Key | |
| --- | --- |
| `hide` | entry labels to drop |
| `menus` | entries added under `under` (Settings by default) |
| `values` | `{ key = "shell command" }`, run at start and every 2 s while the menus are open; usable as `{key}` and in `when`/`unless` |

Entries also take:
- `run = "cmd"`
- `image = "unit:NAME"`, `"run:NAME"` (`/var/run/unfr0zen`) or `"plugin:NAME"` (beside the plugin)
- `tile = "unit:NAME"`
- `wait = seconds` with `after`

**Languages:** texts are matched by their English words against stock's en_US strings on the unit, and shown in the language picked. `config/locales/<lang>.lproj/Localizable.strings` adds or overrides texts, and `unfr0zen.language` names a new language. Chinese uses Noto Sans SC, cut down by `tools/cjk-font.py`.

## Unit services

- **Heartbeat:** owns `com.nestlabs.nlclient` on the bus and answers nlheartbeatd. If it stops answering, the UI restarts and then the watchdog resets the unit.
- **Sleep:** the display sleeps after 40 s (`--display-idle`). The client is nlsleepd entity `/com/nestlabs/nlClientApp`: it holds the unit awake while lit, gives a wake 2 s to light the display, and re-registers when nlsleepd restarts. nest-qemu can't complete a suspend.
- **Piezo:** a 2 kHz, 3 ms click per step and press via `pwm-beeper`, off with `display.click`.
- **nestlocal:** `POST /entry` (`--cloud`, default `https://127.0.0.1/entry`, CA `--ca`), then a held subscribe and puts. nestlocal is authoritative; local changes are sent 0.8 s after the wheel rests. `--no-cloud` stays local.

| nestlocal field | UI |
| --- | --- |
| `shared.target_temperature` | target (both ways) |
| `shared.target_temperature_type` | mode (`range` = heat-cool) (both ways) |
| `shared.current_temperature` | room temperature |
| `device.temperature_scale` | `display.units` (both ways) |
