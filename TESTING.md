# Testing client-ng

Commands run from the unfr0zen-dev root.

## Host

```sh
cargo test -p client-ng --locked
cargo run -p client-ng -- --check --skins client-ng/skins --menus client-ng/config/menus.lua
```

- `--check` runs every skin's Lua on each screen with no display, so it catches Lua errors.
- `src/platform/` is Linux-only: run its tests in the build image. CI (`.github/workflows/ci.yml`, on pushes to main and on PRs) runs clippy, the tests and `--check` on Linux, with warnings as errors.

```sh
docker run --rm -v "$PWD:$PWD" -w "$PWD" -e CARGO_HOME="$PWD/.work/cargo" \
  -e CARGO_TARGET_DIR="$PWD/.work/linux-test" unfr0zen-build cargo test --locked -p client-ng
```

## Desktop

`cargo run --release --features desktop` (see README) is the quick loop for skins, menus and the engine.

It draws on the CPU and has no piezo, bus or suspend. Check frame rates, GPU drawing and the unit's services on nest-qemu.

## nest-qemu

[nest-qemu](https://github.com/unfr0zen/qemu) emulates the gen 3 and boots the stock firmware.
- **Where:** beside this repo (`nest-qemu` or `qemu`) or at `$NEST_QEMU`, with its `setup.sh` run once.
- **State:** `nest.py` boots a copy of its NAND state in `.work/client-qemu/` (`--fresh` reseeds it).
- **Web UI:** `http://127.0.0.1:8090` (panel, wheel, button, piezo, console). Piezo tones: `curl -sN http://127.0.0.1:8090/events`.

```sh
client-ng/tools/nest.py qemu start [--offline] [--mqtt HOST:PORT]   # ~2 min
client-ng/tools/nest.py qemu status | qemu stop
client-ng/build.sh && client-ng/tools/nest.py run [-- ARGS]         # deploy, replace nlclient
client-ng/tools/nest.py watch                    # push skin/menu edits on save
client-ng/tools/nest.py logs -f
client-ng/tools/nest.py stop                     # monit restarts stock nlclient
client-ng/tools/nest.py ssh 'CMD'                # root/nest; root fs read-only
client-ng/tools/nest.py wheel 3 [--pace 0.02]    # detents, clockwise positive
client-ng/tools/nest.py click | hold | press | release
client-ng/tools/nest.py screenshot -o x.png [--raw]
client-ng/tools/nest.py record DIR --seconds 3   # every frame; start it, then trigger the transition
client-ng/tools/nest.py compare a.png b.png -o diff.png
```

## nestlocal / Home Assistant over MQTT

```sh
printf 'listener 18831 127.0.0.1\nallow_anonymous true\n' > /tmp/mosq.conf && mosquitto -c /tmp/mosq.conf &
client-ng/tools/nest.py qemu start --mqtt 127.0.0.1:18831
client-ng/tools/nest.py ssh 'monit unmonitor nestlocal; kill $(pidof nestlocal); sleep 1;
  setsid /nestlabs/sbin/nestlocal -matter -mqtt tcp://10.0.2.100:1883 >/tmp/nl.log 2>&1 </dev/null &'

T=nestlocal/09AA01AC25160NMZ/set                 # serial = guest hostname
mosquitto_pub -p 18831 -t $T/mode -m heat        # heat, cool, heat_cool, off
mosquitto_pub -p 18831 -t $T/target -m 21.5
mosquitto_pub -p 18831 -t $T/target_low -m 19
mosquitto_pub -p 18831 -t $T/temperature_scale -m F
mosquitto_sub -p 18831 -t nestlocal/09AA01AC25160NMZ/state -C 1
```

- The broker must bind to 127.0.0.1.
- Both stock and client follow these changes. Store: `/media/data/nestlocal.json`.
- Weather: `set/outdoor_temperature`, `set/outdoor_humidity`, `set/weather`. Stock only reads them at start and every 3900 s.

## Matching stock

1. `nest.py stop`, wait ~20 s for stock.
2. Set the state over MQTT, drive stock to the screen, then `screenshot` / `record` it.
3. `nest.py run`, same state and inputs, then `screenshot`.
4. `compare`, measure the differences, and fix `skins/classic/theme.lua` or the drawing.

- **Buttons:** both UIs act on release.
- **Wheel physics:** the ring and carousel use stock's physics (`src/scroll.rs`), so the distance depends on pace. Elsewhere the client steps every 2 detents. On stock's Schedule, land on times by screenshot, not by counting.
- **Room temperature:** stock reads the backplate and the client reads nestlocal, so they can differ by a few tenths.
- **Lost inputs:** quick inputs can be lost by either UI. Check the highlight before each press.
- **Timing:** time transitions from recorded frames, and only against recordings on the same emulator. The guest clock runs slow, more so under GPU load.
- **Measurements:** stored in `theme.lua` and `icons.lua`. Text sizes come from Akkurat's cap height (0.729 em).

## Performance (nest-qemu)

`run -- --fps` logs input latency and, each second, the frames drawn with the ms a frame spent drawing (the view's tables, the painter, and of that the layers drawn again), flushing and swapping, the shapes and femtovg flushes a frame, and the worst frame's stages: `pre` is everything before the draw, with the slowest of its stages (inputs, tick, hvac, backplate, cloud, bus) named.

- **Draw calls:** ~0.2 ms each, and every covered pixel is shaded. Put static parts in a `gfx:layer`, `paint_layer` it, and clip to where it shows.
- **Shapes:** circles, rings and rounded rects come from cached textures. Keep their sizes steady; a new size is drawn by femtovg.
- **Layers:** a key change redraws the layer. Give variants their own layer names.
- **Paths and text:** stroked paths are costly. Text is cached as images after its first frame.
- **Images:** decoded on a thread; a frame drawn before an image is ready leaves it out and is drawn again when it comes.
- **The wheel:** a layer whose key holds the target is drawn again at every detent. Key layers on what they draw from, and draw what the wheel changes (the number) as it is.

## Unit services

- **Heartbeat:** every 5 s. After 8 misses (~1 min) the watchdog resets the unit, so a crashed client shows as a reboot.
- **Sleep:** after 40 s (`--display-idle`); the first input only wakes. nest-qemu can't suspend (the ADC interrupts on every conversion), so check the bus side only: `dbus-monitor --system "interface=com.nestlabs.scpm.sleep_manager"`.
- **Piezo:** 2 kHz, 3 ms, via `pwm-beeper`.

## Pitfalls

- **Scratch is wiped:** `/media/scratch` empties at boot and whenever monit starts stock nlclient. After either, `run` again (`ssh uptime` shows reboots).
- **Slow guest clock:** about half speed, so double hold times (`hold --hold 7000` for Doom's 3 s quit).
- **Sleep and menu timeouts:** the display sleeps after 40 s and the menus close after 30 s. Check the screen before each input; a black screenshot means asleep.
- **`pgrep -f` over ssh:** it matches its own shell. Use `pidof`.
- **nlsleepd:** ignores SIGTERM; use `kill -9` (monit restarts it).
- **Stock never sleeps here:** its `sleeptimeout` is 86400000 ms. To capture its sleep or Farsight, bind-mount an edited copy:
  `cp /nestlabs/etc/client.config /media/scratch/ && sed -i 's/"86400000"/"40000"/' /media/scratch/client.config && mount --bind /media/scratch/client.config /nestlabs/etc/client.config`, then restart nlclient.
- **Changing `--wires`:** stock starts its wiring wizard. Finish it before changing again, or stock may flip to heat.
- **Heat Link:** nest-qemu is a Display-3.4, so stock treats it as a Heat Link unit: no W1 gives H71, no C gives the Heat Link setup then H72.
- **Stale deploys:** `run`, `deploy` and `watch` ship what `build.sh` staged last. Re-run `build.sh` after edits; files copied into `build/client-ng` by hand are lost.
- **Skin choice:** comes from `display.skin` in the guest's `settings.json`. `run -- --no-settings` starts on Classic.
- **Skin reloads:** after a file's size or time changes and holds still for 0.5 s. A skin that fails to load keeps the previous one and logs why.
- **Real unit only:** suspend, the backplate and real timing can only be checked on real hardware. Report those results separately.
