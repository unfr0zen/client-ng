# TODO: client-ng vs stock nlclient 6.4-8

`[x]` done, `[ ]` open: tick an item once done, in the same commit. **(verify)** not yet compared with stock on nest-qemu/unit; **(unsure)** stock's behaviour not pinned down. Backplate: `BACKPLATE.md`; comparing: `TESTING.md`. Evidence: addresses are Ghidra's for 6.4-8 (`unfr0zen/.work/nlclient`), `g2 0x…` the gen 2 decompile's string xrefs.

## Look

- [x] Settings sub-line "nest.com/w5" in #00bfff under "Wi-Fi connection error" (7e1c5c0)
- [x] Mode symbol colours: Off #b8b8b8, Heat • Cool blue and orange (7e1c5c0)
- [x] True Radiant control: won't do, needs an OpenTherm Heat Link (modulation, early start; nest::energy's radiant path)
- [x] Settings sub-line "System issue" + "nest.com/m13" while the short-cycle flag stands, after "Wi-Fi connection error" (0x1a9c00 case 5) (5fcf24c)
- [x] Settings' notices and badge off in stock's demo mode (globals vtable +0x1e8 = +0xf22, config `initialstate` "demoview", 0xda3a8); the client has none, so always (fbe32fb)
- [x] Equipment tile "System Issue" + "nest.com/m13" under U+E037 while the short-cycle flag stands (0x139a9c) (fbe32fb)
- [x] Settings sub-line "Software updated on %s" until Settings is selected in the ring; the update is a changed firmware/client version at start (8e732f1)
- [x] "Software updated on %s" date as stock's GetFormattedTime 0x408 (libnlappcore): abbreviated month, no year; "Oct 17" en_US, "17. Okt." de_DE, "17 Oct" others (b0d220f)
- [x] Software page's "Updated %s" under the version while an update is known (0x1f1184), centred with it; matched on nest-qemu (b0d220f)
- [x] Software page's "Version %s" translated: a version (6.4-8) is a text's value (bd37e2e)
- [x] Heat • Cool symbol's colours: checked on nest-qemu, lower two strokes 0xe075 (#0080ff), upper two 0xe074 (#f14f00) (b77e28e)

## Home screen

- [x] Farsight weather failure as stock's far view: the cloud alone, no text (f034344; the texts of 83915b5 belong to the approach transition)
- [x] Farsight weekday-date line: none in the far view, as ours; stock draws it only as someone comes near (0x144744 state 5); checked on nest-qemu, analog and digital (f034344)
- [x] Farsight weather failure: stock's far view is the cloud alone, 287×187 centred, no text or nest.com/m2; matched on nest-qemu (f034344)
- [x] Farsight's near view as someone comes near (thermopile, near PIR; 0x145640 states 5 then 3): clock or weather shrinks, weekday-date, dial or failure text fade in over 500 ms, held until an input (3139da2)
- [x] Farsight near view's unsure parts: libnlappcore read: GetDayString 0x200 whole weekday, without it short, 0x100 capitals; GetFormattedTime 0x400 locale order, no 8 whole month, no 0x20 no year (dates already right); failure text is stock's list block (glyph line 101, text 32, nest.com/m2, OK highlighted) laid out as the notice's (653652f)
- [x] Farsight weather's near forecast: nestlocal's forecast.hourly read; three hours picked as 0x142f3c (first >3 h away, +4, +8, 1–7 AM moved to 8), drawn as 0x1423fc (653652f)
- [x] Farsight to home on an input: clocks and weather fade out over 500 ms (750 analog) on sin³, growing to 1.45, analog hands gone in the first fifth (state 6) (653652f)
- [x] Farsight far view's arrival: 500 ms on curve 4 (1.68 sin² then a damped wave, peak 1.26), scale 1 + 0.2(1 − v) (state 4) (653652f)
- [x] Farsight weather failure's near view as 0x4197b0 lays it out: lines ≤ 408 or the face's width less 24 (0x418ac8), least-squares breaks (0x418b8c), first line from 0x418254's bounds, nest.com/m2 61 under; OK shown, highlighted (0x4176a8 selected 0); far text hidden by vtable +0x5c = 0x41264c zeroing its colours at nearness 0 (0f65fed)
- [x] Farsight online weather failure's far view: nest-qemu's stock is online (libnlnetworkmanager logs "Updating connected to connected"; IsConnected 0x74bd4 is that tribool), so 0x143fe4 takes the online table (+0x1c) and shows the cloud alone; nest.com/m2 fades in with nearness (bd31571)
- [x] Farsight forecast's font: AkkuratNest-Medium (0x1441a0 sets gfx globals +0x40, akkuratfontmedium, before 0x1423fc) (0f65fed)
- [x] Farsight temperatures as stock's: far target is home's number ×1.45, far current the room's label ×7.21 centred (0x1d3b84, 0x1ddfb4); arrival grows from 0.8 on curve 4, the target fading in; near or an input zooms out to home over 500 ms on sin³ (state 5), dial, words and symbol fading in, the label sliding to its place white → its colour (0x1dcf78, 0x1db3e0); temperatures have no state 6 (0x145640) (64069d4)
- [x] Farsight off, heat-cool and Eco far views: home's middle ×1.45 (OFF under its system, the range's numbers in home's face, ECO), fading in on arrival; state 5 shrinks it whole as the rest fades in (0x1d3b84's text and range paths); matched on nest-qemu (24be129)
- [x] Farsight's far target number, home's face ×1.45: matched on nest-qemu glyph for glyph; far current fades in arriving and centres its whole part, as stock's (24be129)
- [x] Farsight's state-5 zoom draws the ticks as lines and words, labels and symbols at the size they show (`sharp`), the layers kept at rest (24be129)
- [x] Farsight clocks and weather leaving (state 6): the home screen fades in under them (0x1da9d4 at 0x1dae6c: background, dial and temperatures at v), the view fading as one (a layer); seen on nest-qemu (24be129)
- [x] Farsight clocks' and weather's scale: 0x144744's 1 + 0.2 (1 − v) and 1 + 0.45 v divide their size: in from 0.85, peak 1.07, out to 0.86 as stock's on nest-qemu (bd31571)
- [x] Farsight analog clock leaving: hands at (0.2 − v) / 0.2 under alpha 1 − v (0x144444), stock's reach/cyan pairs (42 at 0.84) as ours; nest-qemu runs the 750 ms ~4× slow; second hand #0bdbff as 0x144444's (bd31571)
- [x] Alerts, notices and dialogs over Farsight: Farsight under them, hidden in states 2/4, fading in with v in 5 and in 6 from afar, whole over the near view (0x227364, 0x2273d8; 4's growth is unseen at opacity 0); near, the clocks and weather give way instead (0x145408 answers 1) (24be129)
- [x] Alert's own fade-in no longer restarts as Farsight gives way: alert, notice and dialog keep their screen time, shown at 0x227364's opacity alone (bd31571)
- [x] Farsight's far views (not current) draw a drained battery's U+E037 ×1.45 about the centre, fading in with v in state 4, whole in 2, 5 and 6 (0x1d3b84 → 0x1d0624 param_6) (bd31571)
- [x] Room label's half: 17 units, set right after the whole: far 87 tall, 45 after, as stock's (bd31571)
- [x] Heat-cool's range at rest: 99 units, dot gap 42, baseline 275: far 365 wide vs stock's 366, dot 27/30 from its neighbours vs 26/29 (bd31571)
- [x] Off's system line (HEAT • COOL) 27 units, not 25: 152 wide at home as stock's (bd31571)
- [ ] Farsight's far U+E037 not seen on stock: its place ×1.45 (centre ~478) only peeks at the face's bottom; nest-qemu had no drained battery
- [ ] Farsight's far cloud-program symbol (0x1d0624's local_8c branch at param_6): ours has no cloud programs
- [ ] Heat-cool range's dot gap 42 taken for the picked end's (108) layout too (unsure)
- [ ] Farsight clocks leaving: stock draws the home screen over the clock or under it unread (0x1da9d4 vs 0x144744 order)
- [x] `alert.notice` as stock's list block (nlAlert's draw 0x225790 → 0x417c78 → 0x41df18 → 0x41d944 → 0x4197b0, as Farsight's failure 0x143fe4): balanced, face-fitted breaks shared (`alert.block`); stock's strings have no breaks (0x5e90c0) (64069d4)
- [x] Line over the number in stock's order (0x1d3644 tries 0x1d0f8c's cases 2, 1, 4, 0, 9, 0xe, 6, 10, 7, 0xb, 0xc, 8, 0xd): SAFETY, PRESS TO COOL/HEAT, EMERGENCY HEAT (mode 4), AUX. HEAT (aux running), PREHEATING/PRECOOLING, COOL TO DRY, AIRWAVE, stages 2–3, time to target, ALT. HEAT, HEATING/COOLING, SET TO (2b2eade)
- [x] SAFETY while Safety Temperatures run the system (case 0: globals vtable +0x94, byte +0x19ab) (2b2eade)
- [x] SAFETY as gen 2 computes it (g2 0x5f9dc): room within 0.39 °C of the low safety temperature (0.56 °C of the high) while off, Eco not covering it or the other mode; 6.4-8's +0x19ab setter (vtable +0x98, 0xb72c8) is in no call (b5bd876)
- [x] PRESS TO COOL in heat at ≤ 10 °C (51 °F) with Stage1Cool, PRESS TO HEAT in cool at ≥ 31 °C (89 °F) with Stage1Heat, by the shown whole degrees (case 9); a press at either end opens the menus on Mode when it heats and cools (0x1d8aa0) (2b2eade)
- [x] RUSH HOUR, FOR PEAK TIME, PEAK TIME, UNTIL %ls: won't do: cloud (cases 1–3 read Rush Hour/energy events 0xbab48, 0xd0e8c, 0xd0d98; UNTIL is their end; case 4 the cloud's own captions, globals +0x1a4)
- [x] HOLD UNTIL %ls: won't do: cloud (case 0x10: the hold at globals +0x18b4, set only by events 0x40000110/0x40000111 via 0xd0cec from the `hold_*` fields; no local entry starts a hold)
- [x] Hold mode alert: won't do: cloud (alert 0x29 HoldModeAlert, 0x242e08, for the cloud's hold)
- [x] SYSTEM ISSUE under the number, first of that line (case 0x11 in 0x1d2fcc), and U+E037 #ffbb00 in the leaf's place after Sunblock (0x1d0624), while HvacBatteryDrained stands (b5bd876)
- [x] NIGHTTIME HEAT, FOR THE NIGHT: won't do: cloud (`heatpump_setback_active` +0xf40: setter vtable +0x25c 0xbd37c in no call, device field id 0xa7 0xf6d38; gen 2's setback g2 0x22e298 needs the cloud's forecast, absent from 6.4-8) (b5bd876)
- [x] MANUAL: won't do: Heat Link's manual mode (vtable +0x454 0xb9278: +0x1658, set only to −1 by nlTurnOffManualModeAlert 0x1ea140, else nlController +0x3c → nlBackplate +0x28 → its +0x8c's +0x34; device field `heat_link_manual_mode`) (b5bd876)
- [x] Lockout countdown under the number (case 5, 0x1ce2fc): STARTS IN m:ss while the compressor lockout runs (`compressor_lockout_enabled`, checked first), else DELAYED FOR m:ss while a minimum cycle holds; from at most 150 s / 300 s (0x1cd760) (2b2eade)
- [x] Minimum cycle countdown's cap 1800 s while the compressor heads-up is in a cool state or rests: +0x1344 is `compressor_heads_up_state` (0xdb84c, setter vtable +0x3a0 0xbc6a0 in no call; states 0x4dddc0) (b5bd876)

## Top menu, Eco, Fan, Mode

- [x] Fan entry only with a fan; Mode and Eco 45° apart round 12 o'clock without it (bf08c9a)
- [x] Mode's sub-line "Emergency heat" during emergency heat (7e1c5c0)
- [x] Fan's ring sub-line with the fan schedule on: "On every day …", "Always on" (7e1c5c0)
- [x] Fan's "Until %s at <day>" from 24 h away, in the clock's 12/24 h format (7e1c5c0)
- [x] Ring's middle crossfade 150 ms out / 150 ms in (7e1c5c0)
- [x] Fan "%d week left" from 7 days (7e1c5c0)
- [x] Eco while Off: "This thermostat is off. Turn on and start using Eco Temperatures?"; START ECO turns on in the last mode (83915b5)
- [x] Mode on heat-only or cool-only systems (type 2 "power", 0x1a6e04): the mode or Off, "Press to turn off" / "Press to turn on" coming in (0x1a9c00); the press turns the system off, or on in its last mode, stops Eco and goes home (nlThermostatView 0x1e0af0); in Eco, Mode says only its press, its symbol white (0x1a5f64) (3b6067c)
- [x] Fan's ring sub-line speed variants: with a fan of two or three speeds (`fan_capabilities`; 0xcb60c: of two the second is high) the timer's or the schedule's speed (`fan_timer_speed`, `fan_schedule_speed`) (3b6067c)
- [x] Ring's "Press to change": stock's 4 s state 3 repeats while the entry is selected: the hint comes in 0.53–0.93 s, the value back 2.53–2.93 s, linearly, the text going out sliding down a line and the other coming from above inside the line (0x5e4b54, 0x1a5e0c, 0x1a9c00) (3b6067c)
- [x] Fan page speeds: the running timer's "Running for the next %d … at low/medium/high speed" with a fan of two or three speeds (0x13e050) (3b6067c)
- [x] Off-and-away alert: from Settings' Fan pressed while off (nlSettingsView 0x1b007c, client vtable +0xe0 off); TURN ON, first, turns the system on and opens the fan's page, LEAVE OFF stays on Settings (0x1e89e8) (3b6067c)
- [x] HOT WATER ring entry: won't do: it needs hot water equipment (client vtable +0x438: equipment flag 16 and a Heat Link hot water type, 0xb9de4) and stock's HotWaterActor's boost and schedule, which ours lacks (HVAC control's Hot water)
- [x] ACTIVE SENSOR(S) ring entry: won't do: cloud (the sensor count 0xce5c0 comes with the cloud's `rcs_settings`/`kryptonite` buckets; their beacons need a key fetched from the cloud, `%s/kryptonite/devicekey`)
- [x] Heats and cools from the wiring as stock's equipment flags 5 and 2 (0x22ecd8 → 0x2e34d0: W1 or O/B, Y1), for Mode, the ring, PRESS TO, SAFETY and Your System; nestlocal's `can_heat`/`can_cool` until the wiring is known (cfd19bf)
- [x] Mode with Heat Link manual heating ("Manual", "Press to end", its alert): won't do: Heat Link (vtable +0x454 reads `heat_link_manual_mode`, 0xb9278; see Home screen's MANUAL) (cfd19bf)
- [x] Ring hint's cycle from when the ring settles on the entry, as stock's from the wheel's scroll ending (+0x9c scroller timer done → 0x1a5e0c, and at open 0x1a7110) (cfd19bf)
- [x] Ring hint's slide: a line's height (stock's measured height is the face size, 0x425e54) seen between 21 over and 6 under the baseline, as stock's black masks at title baseline −3…+19 and +46…+68 (0x1a9c00) (cfd19bf)
- [x] Ring hint over two lines: the value's or hint's lines, whichever more (n), slide n × size + 6 (n − 1), the window n lines less 22 under the first baseline (0x1a9c00, `local_f8` > 1) (95bb727)
- [x] Ring hint frames only while it slides, a `gfx:wake` for the next slide between (cfd19bf)
- [x] Fan page's running sentence: stock's state 5 list (table 0x706568: one text, STOP FAN flagged 0x20000000 under it, BACK at the foot) through 0x418254: text from 79, centred on 240, STOP FAN midway to BACK's top 372; `alert.list` and `alert.block`, no arc (95bb727)
- [x] Settings' Fan with a fan of two or three speeds: tile Fan → "Fan settings" list (state 0: fan glyph, Schedule, System fan speed, BACK); System fan speed asks by heats/cools (0x13e264) over auto/low/(medium)/high, DONE (common string 1); `fan.timer_speed`, `fan.schedule_speed`, `fan.heat_cool_speed` sync with nestlocal both ways (95bb727)
- [x] Fan timer with a fan of two or three speeds: state 4 page "Run the fan for the next" [run] "at" [speed] "speed.", DONE (common string 1) starts it, rows by 0x418254 (95bb727)
- [x] Fan page's time left as stock's 0x13dd1c (the page's: state 5 at 0x13e070, 0x140480): minutes up below 3541 s, hours to the nearest below 84601 s, days to 561600 s, then 1 week (cfd19bf)
- [x] Fan schedule editor's speed rows: "at" [speed] "speed." under the hours (state 2, 7 rows); the page runs past the face, moved up to show SET/DONE once reached (95bb727)
- [x] Fan timer page with speeds: as stock, no way out but DONE: state 4's press (0x13cd04) edits a row (0xe0548) or starts the timer and closes; else timeout (20745c5)
- [x] System fan speed page's rows: as ours; table 0x7064e8 (state 1) is the question, an empty row, the chooser's row (0x13d28c draws it at row 2), DONE; its flags 0x70e244 sit in .bss with no writer in the decompile, so 0 (20745c5)
- [x] Fan schedule with speeds: the page moves up in proportion to the focus, none on the first field, all its overflow on DONE (nlClientLayerScrollable 0x23aba8/0x23b40c) (20745c5)
- [ ] Fan schedule's scroll constants: 0x23b40c's ratio from the measured rows and FUN_000cb128's mode (client +0xe60) unread; ours linear from 0 to the overflow
- [x] Settings' Fan tile with speeds: no summary; nlFanScreen 0x1420f4 names the schedule's Enabled/Disabled only for a fan of one speed, else "FAN" alone (20745c5)
- [x] The fan schedule runs at `fan_schedule_speed`, the timer at `fan_timer_speed` (stock's HVAC input carries both, 0x37eb64 via 0x2eec54/0x2eec64) (cfd19bf)

## Backplate link

- [ ] Firmware update's hex path (0x30609c, update kind 5): suspend controller, write `/tmp/tfe.hex`, `/usr/sbin/stm32flash` (0x32e1a4), 2 tries (`HexUpdate` Failed / FailExceeded), delete file, resume, reset UpdateComplete; not done: backplate.py speaks no STM32 bootloader (only the 0x91 BEGIN S-record update), so it cannot be tested
- [ ] Update screens' texts: client shows updatelayer.updatingnest "Updating the thermostat." for update_backplate (unsure: step text, failure text; updatelayer's keys are not referenced in nlclient's decompile, its layer is libnl's)

## Wiring and equipment

- [ ] Wiring report pages' DIAGRAM and SETUP choices; code 46's MORE INFO page: the choices are the report layer's own strings (0x2070b0: +0x31, +0x38, +0x46, +0x276, +0x27d, their source unread) with common strings 0 BACK, 1 DONE, 2 CONTINUE, 5 MORE INFO, 9 CONTACT A PRO; the codes each set is for are FindElement arguments the decompile lost
- [ ] After CONTINUE: stock's wizard ends on home screen; ours opens Equipment (`engine.rs:1219`) and its DONE lands on the Equipment tile (wizard)
- [ ] Stock's flag letting Y2 as G2 / STAR as G3 be heat stages: 0x2e34d0's 6th arg is globals+0x1078 (vtable +0x2a8, 0xb6e2c), set only by the setup layer's heating answer (0x1870c4) from whether stage 1 heat needs the fan (+0x2b0 → 0x2f018c, ours `heat_fan`); never stored (only setter 0xb7a8c), so off after a restart, as ours (wizard)
- [x] Power-off alert when the report's last code is 46: BackplateConnectionsPowerOffDueToCompressorLockout (alert 0x31) instead of PowerOff (0x2320cc), "Your thermostat isn't getting power. Troubleshoot it to avoid losing heating." (backplateconnectionsalert.poweroffduetotemp, g2 0x17798c) (f17e5ff)
- [x] Power-off alerts' press: the wiring report (0x22dd88, 0x208efc) over Equipment, R's own codes included, until R has power; the alert not raised again till then (0x23284c posts on a change) (edd9154)
- [ ] R without power: "power save mode delay timeout" 3600 s unless globals 0xcf548 (0x232aa8, timer +0x18), cancelled on power: it sets power save mode (globals+0x18a6, 0xd0cd8), with which the battery check turns Wi-Fi, 15.4, BLE and dynamic scan off as for a low battery (0xcf934); client has no radio power saving

## Switching

- [ ] O/B's circuit (3) is switched with cooling on wiring without O/B (Rh, C, W1, Y1, G: backplate.py shows circuits 1, 2, 3); stock's 0x330150 likely checks O/B is wired
- [ ] Setup wizard's fan defaults (0x1911a8: W1 gas forced air fan type 1; electric for aux/heat pump); nest-qemu stock publishes electric for W1, branch unread (wizard)

## HVAC control

- [x] Safety's 2.3 °C hold (0x51e228 → +0x24): the last safety run kept while room < low + 2.3 (heat) or > high − 2.3 (cool), the safety band then following the last decision's action (0x51e654, 0x51e78c); +0x20/+0x21's readers (0x4f76ac band and humidistat skip, 0x51c42c, 0x4e939c) all gate on SAFETY_TEMPERATURES (f17e5ff)
- [x] Short-cycle detection (nlHvacShortCycle, nlShortCycleTracker): 10 cycles < 90 s in a row (gaps ≤ 270 s), each followed by a power loss within 300 s; 12 power losses within 1799 s; timeouts 1810 s / 4 h; logged only, as stock (`hvac/shortcycle.rs`) (5fcf24c)
- [x] HvacShortCycle flag and alert from the cloud's `hvac_cycle_status` "heating_short_cycle" (0xface0 → 0xbea00), not in cool mode, 48 h from the cloud's time, kept across restarts with power stealing off while heating (0x233b5c–0x233d0c) (5fcf24c)
- [ ] Short-cycle detection's power input: stock's update struct word +0x44 (0x3b0cd8; energy+0x210 vtable +0x18, implementation unread); the struct's +0x48 is the HVAC power (Rethink's "HVAC power off - forcing drift", furnace detection 0x3b0728); ours R powered per MSG_WIRES for both (unsure)
- [x] Short-cycle and furnace shutdown detection skipped while the clock is before the build date (0x3b0728, 0x3b0cd8 via vtable +0x1e4); ours `BUILD_DATE`, a const in code (edd9154)
- [x] Short-cycle detection raising the flag locally: won't do, as stock: its decision only reaches its logs (0x3b0cd8, 0x3b0af8 HvacShortCycling / HvacShortCyclingDecision), the flag only comes from the cloud's `hvac_cycle_status` (0xface0) (f17e5ff)
- [x] Short-cycle flag's start restore: 0x400001a7 is posted once by client globals' start (0xdad88) after its settings are read, as ours on the first tick (edd9154)
- [x] HvacBatteryDrained alert (0x33, 0xcf934): power steal mode (+0x18b0) 2, short-cycle flag (+0x18a7), highest of the last five battery readings < 3.55 V; clears at mode 0 or > 3.65 V (b5bd876)
- [x] HvacBatteryDrained's forced drift: with ThermostatManager +0x3b9 set, the decision's switched circuits (0x2e4a74) against MSG_DISABLE_PS_WHILE_SWITCHED's mask (backplate +0xb50, 0x307c5c) force drift (0x37e764, "Battery too low while power steal is disabled - forcing drift") (f17e5ff)
- [x] HvacBatteryDrained's forced drift: stock replaces only the executed operation state, the requested one (its decision, learning) logged beside it (0x381330 "current, requested, executed"), as ours (edd9154)
- [x] Battery check's period: stock's 0xcf934 on each wake and denied sleep (0xa237c sends 0x40000115) and on every battery or flag query (0xd0b10–0xd0c74); ours once a minute and at each wake (f17e5ff)
- [x] Furnace shutdown detection (nlFurnaceShutdown 0x3afd7c, nlFurnaceCycleTracker, `furnaceshutdownrecoveryfile`): ≥ 4 days, 50 heat cycles, 20 provoking a power loss (< 300 s after), max duration < 901 s, < 10 normal power losses (> 1200 s after) in 4 days, ≥ 11 provoking longer than mean, ratio ≥ 4/5 → FurnaceShutdown alert (0x1f), a week, at most every 182.5 days (0x3b01ac); flag again at start (`hvac/furnace.rs`) (f17e5ff)
- [ ] Furnace shutdown detection's heat input: stock's 0x3b0728 picks it from the update structs (+0x24, +0x2c, +0x34; unread); ours the first heat stage, or a heat pump's first alternate stage
- [x] `is_furnace_shutdown` to the device bucket as it differs; `show_furnace_headsup_test` kept once the detection raises it (0xbdd60) (edd9154)
- [x] FurnaceShutdown alert's look: no heading, U+E034 (a speech bubble, traced from Akkurat), OK; CONTACT A PRO first only with a Nest Pro's contact (0x147734) (edd9154)
- [x] Hot water: stock's HotWaterActor (SCHEDULE, BOOST, BACTERIA_PREVENTION; 0x4e97f4, `hot_water_*`, `bacteria_prevention_enabled`): won't do, Heat Link: `has_hot_water_control` follows `heatlinkhotwater` / `heatlinkheat` (0xca6c8, 0xca580); client never switches WaterHeater (f17e5ff)

## Backplate simulator

- [ ] backplate.py answers no 0x89 (SET_SAFETY_TEMP): a conventional desktop unit (`--model Display-2.7`) resets the link in WaitingSafetyTempSetAck for ever and never switches
- [ ] Radiator start/stepping under `…temp…andhum` alone (unsure): stock starts 2.5 °C above client globals' vtable +0xa8 (0x32bd40), which is 0xb818c: globals+0x48's +0x188, its writer unread

## Heat Link

- [x] RESET PAIRING and REPLACE HEAT LINK: won't do: they redo the Heat Link's Weave pairing (PASE: `SetPreviousHeatLinkJoiningAttemptFailedPASE`, `nlIWeavePairingHeatlinkContext`); the client has no Weave (3889b8a)
- [ ] H71 page scroll per detent as stock's (needs stock seen on nest-qemu: wiringreportlayer's wheel handling has no xref to its keys in the 6.4-8 decompile)
- [x] H73-H78: won't do, the Heat Link's state: stock's checks (0x2e4678) give 49/50/64 (H73, H74, N10) for OpenTherm with vtable +0x448 (globals +0x1698, else nlController +0x38: the Heat Link's) by vtable +0x668 (`heat_link_connection` 2/3, wireless); 51 (H75) by an Agate's battery state (0xce2d8 +0x14); 53/54 (H77/H78) by an Agate's Weave fabric (0xc8980); 52 (H76) not raised there (3889b8a)
- [x] Wired setup flow on wiring change: with `heat_link_connection` 1 a change of wiring opens "Heat Link connected / wired only" (no notice; without W1 the cannot-control notice stays), an error page after (3889b8a)
- [x] Technical Info's HEAT LINK section's other rows: won't do, Weave: Serial Number, Software Version, Model (0x1be4dc: 0xc887c, vtable +0x5e0, 0xc88a8) are kept in memory from the Heat Link's Weave messages (0xbc700, 0xbc780, set from 0x4485fc / 0x448604), Weave connectivity, node ID and battery level (Agate) too (3889b8a)
- [x] Technical Info's HEAT LINK only on Heat Link units (stock's 0x1c0ff4: vtable +0x4b8) (3889b8a)
- [x] HL disconnected delay (0x232aa8, timer +0xb8; 0x2320cc): the first wiring report since start finding H71 on the unit's backplate holds the page 30 s, ended by a report without H71 (3889b8a)
- [ ] HL disconnected delay's 45 s on an Agate (globals vtable +0x4b0: backplate kind 6, 0xb6f34); ours 30 s on every Heat Link unit
- [x] AQUASTAT controller (0x51be80, 0x51bf60): won't do: enabled by the control's input +0x2e0 (0x5108f4), no client source identified for it or its reading; a trip sets BoilerSetpoint 10 °C (+0x40), OpenTherm's (unsure: inputs) (3889b8a)
- [x] Hot water UI (ring HOT WATER, boost, schedule, temp, Bacteria Prevention): won't do: the client cannot drive Heat Link hot water: a Heat Link unit's pins have no hot water circuit (WaterHeater is a STAR wire set as water heater, `wiring.rs:150`), OpenTherm hot water needs wireless (wiringreportlayer.otheatandhotwaterwithoutwireless) (3889b8a)
- [x] TurnOffManualMode "Do you want to turn off manual heating?": won't do: `heat_link_manual_mode` is the Heat Link's (vtable +0x454: +0x1658, else nlController +0x3c), in no backplate frame (0x32c528 drops 0x33, 0x34, 0x37); the alert waits for "received manualmode off" from it (3889b8a)
- [x] HeatLinkLowBattery alert; Technical Info battery level: won't do: an Agate's battery state (vtable +0x4b0, 0xce2d8 +0x14) from Weave (3889b8a)
- [x] Wireless Heat Link (Weave over 802.15.4, pairing, OpenTherm, hot water, firmware): won't do here: the client has no Weave stack and the radio is nestlocal's Thread/Zigbee (Will's call) (3889b8a)

## Schedule

- [x] Browse as stock's momentum scroller: SH_ tuning (0x1a311c: 192 px items, scaler 40, spring 0.054, damping 0.91, curve 6), unsnapped in the week, springing to the dials at 0 and 4416 px; held setpoint within 75 min of the timeline's time (0x1a2c10); dial after Sunday too (5c4cff6)
- [x] Clicks as stock's (0x1a3558): browsing each hour crossed and each strip item off the week, time per quarter, menu per row, temperature each degree past the last click; Schedule and History no longer click per wheel event (5c4cff6)
- [x] °F schedules: stock moves both scales the same degrees a detent (state 2: value ×−2), ours now 1.25 °C a detent too; limits 50–90 °F / 9–32 °C (0x1a4644), ours were 48–89 °F (5c4cff6)
- [x] Open while display sleeps: stock closes on event 0x40000006 (state 5, as History 0x135aa4 and Settings 0x1af80c leave), ours closes at 30 s idle and on sleep; nothing to do (5c4cff6)
- [x] 12-hour clock text: cursor "7:30 PM" (GetFormattedTime 0x404), hour labels "4P" with string.time.PM.abbr (0x2406, as History's axis); flags read from callers, libnlappcore's formatter not on hand (5c4cff6)
- [x] Empty-schedule default: stock's New with nothing in force starts at the scale's top, 32 °C / 90 °F, a range 9–32 °C (0x1a141c state 0); no day-start triangles without continuations; ours were 20/24 °C (5c4cff6)
- [x] Opening's scroll from the dial to now: sin² (curve 0, 0x42bd94), 150 ms each 100 px plus 500/7 ms each 100 px short of 1000 (0x1a141c state 4); no clicks; the wheel takes over, a press lands it first (0f1f1cb)
- [x] A new setpoint's 500 ms slide from the scale's top (a range's low from its bottom) to the temperature in force, curve 2 sin(90t°) (0x1a141c state 0) (0f1f1cb)
- [x] A setpoint's time kept an hour from its neighbours, round the week, crowded on the later bound (0x1a101c) (0f1f1cb)
- [x] Day-start triangle left out with a setpoint under 6 h before midnight (not round the week) or in the hour from it; a week's-end triangle when any other is drawn and no setpoint is in its last 6 h (draw 0x1a2624) (0f1f1cb)
- [ ] A range schedule's midnight triangles: stock draws one for each end (0x1a1fc0 recurses for type 2); ours the low end's only
- [x] U+202F (GetFormattedTime's space before AM/PM) is missing from Inter: schedule and Farsight's forecast_time use a space (bd31571)

## History

- [x] Carousel's spring physics: stock's strip (0x135ebc: 320 px, scaler 12.25, spring 0.054, damping 0.91, curve 1), a click per slot reached (5c4cff6)
- [ ] Carousel's flip from the menu and the thermostat at the ends: stock flips over the first half of the scroll (0x136178: progress 2(x−from)/(to−from), 0x134458 draws it with 1 − progress), through the layer at param_1[7]'s vtable +0xc, unread; ours a slide and a drawn puck
- [x] A press glides the cards 500 ms to the slot: as ours, stock glides (curve 2) and acts in the same event (0x136178, event 0x40000001; 0x135aa4 is the constructor) (0f1f1cb)
- [ ] History's press during a glide: stock ignores it while its 500 ms glide runs (0x136178: anim from ≠ to); ours acts
- [x] Reason glyphs: Away is the Leaf as Eco (0x130c90, 0x42e07c) (5c4cff6)
- [x] Reason glyphs MANUAL U+E007, WEATHER U+E091, SEASONAL U+E053, DRY U+E047 from the unit's AkkuratNest (`unit:`), 63 px, centred on the advance, baseline half the top under the disc (0x130c90, 0x4253e4); drawn ones without it (c4792b1)
- [x] Timeline's clipped-edge marks: runs square, whole pixels, cut at the row's end; a run within 32 px of an end re-cuts that end round in black (0x130884); axis "00 06 12 18 00" in 24-hour time, inferred from 0x2406 as the schedule's hours (5c4cff6)
- [x] Seasonal Savings reason: won't do; 0x34d4e4 gives 4 only from Seasonal Savings' state (+0x1b38 time, +0x1b40..0x1b4c sums), a service feature (nlPhoenixSeasonalSavingsAction) (5c4cff6)

## Settings

- [x] Location's tile summary from the weather's city (217d540)
- [x] Fan Schedule tile only with a fan (bf08c9a)
- [x] Nest Sense entries per equipment: Cool to Dry with cooling, Airwave with cooling and a fan, Early-On (bf08c9a)
- [x] Display › Weather offline: "Connect to Wi-Fi to show the weather." (83915b5)
- [x] Technical Info's NEST WEAVE only on a Heat Link unit (bf08c9a)
- [ ] Home/Away Assist's learned arrival and wake times (PIR summaries, histograms, punishment)
- [ ] Auto-Schedule learning: not in stock (service learned); needs own design, Will's call
- [x] Auto-Schedule's "active for …" day count: per mode, +1 a local day on in that mode, as stock's preference learner raises globals+0xc00/0xc04/0xc08 through vt+0x110 (0x365f80) (a0c931d)
- [x] Reminders: second threshold from the bucket's `second_filter_reminder_threshold_sec` (dad6116)
- [ ] Reminders: second threshold's default when the service sets none: its setter 0xc50f8 keeps globals+0x1108 (64-bit), the constructor's value for it not found (globals' constructor zero-fills most; ours none)
- [x] Reminders: PRO INFO on the alert: won't do, needs a Nest Pro ID (cloud lookup)
- [x] Reminders tile only with a wired stage, water heater aside, whose delivery (vt+0x31c: heater_delivery, stageN_delivery, 0xcbdb8) is not 1 or 2 (dad6116)
- [x] Cool to Dry with a dehumidifier unless it is stand-alone (vt+0x280: dehumidifier_type, 0xc43f0) (dad6116)
- [x] Heat Pump Balance page: O/B, aux, no dual fuel, not True Radiant (vt+0x26c); levels confirmed, Off, MAX DURATION, status (0x1498f8, 0x148e84); `heatpump_savings` (dad6116)
- [x] Heat Pump Balance's symbol: the unit font's U+E103 over its question (0x1498ec) (a0c931d)
- [ ] Heat Pump Balance: MAX DURATION taken as Early-On's night limit (heatpumpcontrollayer.limit.choice; its page unread)
- [x] Eco/Safety one-field pages and "not connected", by heating and cooling (0x22ce30, 0x19ebd8) (dad6116)
- [x] Safety Temp's `safetytemplayer.wwn.aw` page: won't do, globals+0x17d0 is `using_wwn_boiler` (setter 0xbcc44), a Works-with-Nest boiler over Weave the client never has (a0c931d)
- [x] Offline Lock and Software texts (dad6116); Nest Pro's only shows with a stored Pro ID (0x18fe50): won't do, the ID is cloud
- [x] Display › Weather offline: the low-battery text while Wi-Fi is off for the battery (globals+0x1500 flag 2: under 3.6 V until over 3.8 V, 0xcf934) (a0c931d)
- [x] Display › Weather's unpaired text: won't do, it needs vt+0x504, globals+0x18a1, "Device is using Service Phoenix" (0xc5630): Google's service (a0c931d)
- [x] WAKE DISPLAY: won't do, nlWakeUpScreen shows it only without the Farsight object (globals+0x1638, 0x1f4624), which the client always has; FUN_000bb4f4 is the current view's on-screen flag (view+0x5c, vt+0x20/0x24) (a0c931d)
- [x] Reset › Schedule while Auto-Schedule is off, "basic schedule or just clear" (0x19d28c) (dad6116)
- [x] Reset › Schedule BASIC: stock's quick schedules (0x37b140, 0x3a25e4; times 0x70a978, temperatures 0x9b2c8): 69/65/69/64 °F heating, 75/77/75/74 °F cooling; on a Heat Link unit 20/16/20/15 °C in Ireland and the UK, 20/17.5/20/16.5 °C elsewhere (vt+0x500) (a0c931d)
- [x] Reset countdowns: 10 s, a press cancels (0x13bf88, 0x13c768) (dad6116)
- [x] Reset: "Resetting schedule/Away/Auto-Schedule" after the count: won't do, stock shows those only for kinds started with 0 (defaults, network, pairing: 0x13c020, 0x19d28c); these three leave at the count's end, as ours (a0c931d)
- [x] Auto-Schedule confirmations: Continuing / Turning on / Turning off, off's question (0x1605b0) (dad6116)
- [ ] Auto-Schedule: "Turning on" taken as with the mode off; learninglayer's state tables (0x7070a8) are built at run time, so the transition stays unread (vt+0x4a8 5 is Display-4, globals+0x294 3)
- [x] Date & Time › Set time offline: the country's time zones (0x1210a0 over 0x122014), the one in use first chosen (0x121a88); set as the client's TZ (a0c931d)
- [ ] Set time's time zone: its box's place unchecked against stock; TZ needs the unit's zoneinfo (unverified); stock's AM/PM field in 12-hour format missing
- [ ] Hot Water Schedule tile (`hotwaterscreen`, vt+0x438; NEW/CHANGE/REMOVE): needs Heat Link hot water
- [ ] Placement: paired unit's where bucket
- [x] Keyboard: stock's strip (0x130234, WH_: 33 places, impulse 6, spring 0.065, damping 1.1, power curve 5) and its opening over 0.4 s (fade, 90° turn, 36 out, keys 481/445 out; 0x12e62c) (a0c931d)
- [ ] Keyboard's opening: its 0.4 s and linear easing are the layers' transition (0x4155b4), the turn's direction assumed
- [x] Equipment TEST: systemtestlayer's list (0x1c3910: stages the wiring has, heat, cool, fan, humidity, fan speeds), stage pages, DONE / TROUBLESHOOT (nest.com/t01-t12), completed, 35-min limit (10 for an AC-integrated dehumidifier) with SKIP TEST / TEST AGAIN, outside limits 29 °C/85 °F heat, 16 °C/60 °F cool (0x1c46bc, 0x1c46fc), the lockout countdown (0x37); requests go through the HVAC control (src/systemtest.rs) (81c643c)
- [x] Pro Setup's TEST opens the system test (0x191c54) (81c643c)
- [x] System test's answers: won't do, stock only logs them (SelfTestResult, FHUSystemTest log events, 0x1ca0c8) to the cloud and ticks the list; globals+0x1620 (0xca1c8) has no reader; ours logs them (81c643c)
- [ ] Client's control using Pro Setup's answers: delivery and source feed the fan (heat_fan); OpenTherm and hot water not (needs the Heat Link)
- [ ] System test's pages' layout unseen on stock (stock never gets weather on nest-qemu): ours heading, wrapped text, link, choices 57 apart
- [ ] System test's Wi-Fi gate (0x33): stock's second choice is common string 6, unread; ours CONTINUE only. Its Phoenix page (0x34) won't do
- [ ] System test while wires change: stock stops it for nlBackplateConnectionsAlert (0x1c4dac, event 0x40000049); ours stops when the notice or error page takes the screen
- [ ] System test's screen timeout (0x1c5240) unread: ours keeps a running stage's page up past the menus' idle and the display's sleep, closes the rest
- [ ] Heat Link units: stock's vt+0x4b4 (backplate kind 6) hides heating's wire line; ours by the Heat Link gate
- [ ] Nest Pro ENTER PRO ID: stock's 6 boxes (XXX-XXX, DONE, CANCEL); contact lookup is a cloud URL nothing serves
- [x] Technical Info: Last data upload, at each push to nestlocal (0x1bf428) (dad6116)
- [x] Technical Info: Weave fabric id: won't do, stock reads its running Weave stack (0xbf8a4, "n/a" without); the client runs none
- [x] Technical Info: EQUIPMENT outside the Heat Link gate (0x1bc930) and DISPLAY's Time Zone, %Z (0x1bd424, 0xc2110) (dad6116)
- [x] Technical Info: NEST WEAVE with vt+0x504: won't do, globals+0x18a1 is the Phoenix (Google) service flag (0xc5630) (a0c931d)
- [ ] Technical Info: EQUIPMENT's star as "*" for stock's U+E035 (Inter, the fonts' fallback, has no U+E035)
- [x] Tile symbols: Lock's U+E050 (0x1d6920, else the padlock) and Nest Pro's U+E034 (0x145fec) from the unit font; a tile draws its screen's glyph (0x1ad3dc) (a0c931d)
- [ ] Tile symbols: Date & Time's screen sets U+E039, a calendar with "!" (0x1200e4), which looks like the wrong-time state; when its tile shows it unread
- [ ] Fan Schedule "00 to 00" meaning (unsure; ours the whole day, `engine.rs:354`)
- [x] Your System lists Cooling from the wiring: `tech.system` takes `heats_cools()` (b027339)
- [x] Display › Weather keeps its check mark offline (dad6116)

## Equipment Settings pages (UI; control exists)

- [x] HEAT PUMP: O/B, compressor/aux lockouts with Balance off, its note with it on (equipment.rs) (281dd80)
- [x] DUAL FUEL: breakpoint with Always/Never ends, PRIMARY/ALTERNATIVE without the weather (281dd80)
- [x] HUMIDIFIER/DEHUMIDIFIER: SET HUMIDITY, off, QUIET TIME (281dd80)
- [x] STAR WIRE TYPE, HEAT TYPE (281dd80)
- [x] Pro Setup for conventional wiring: wire list, per-wire fields (281dd80)
- [ ] Equipment pages' gates guessed: DUAL FUEL as stage 1 alternate heat (stock's 0x179ed0 unread), HEAT TYPE's 0x3c0 != 2 unread, STAR's G3 as G with Y2 G2 (0xfe8c8's flags unread)
- [ ] HUMIDIFIER's SET TYPE: not done (its page unread; g2 humidityequipmentlayer code 3); prompt without % for 0x23c/0x244 (unread) not done
- [ ] DUAL FUEL: which page opens first unread (ours: the slider with the outside temperature, else PRIMARY/ALTERNATIVE)
- [ ] HEAT PUMP: nowifi page ("needs an Internet connection") not shown, trigger unread; stock shows the compressor slider before Balance's note, ours goes straight to the note
- [ ] HEAT PUMP CONFIRM's BACK back to the compressor (stock's state 3 choice 0: unsure)
- [ ] STAR WIRE TYPE's Pro Setup warnings: ours sets the type then warns with OK; stock's buttons and whether it sets it unread
- [ ] Pro Setup wires: layout unchecked (ours a titled list, "W1: Heat"); "Completed"/"Check" column, W2/Rc/C/G pages, stage-2 heat, fan activation, humidifier type fields not done; stock's bucket values for star_type/y2_type ("hum", "emer-heat") unverified
- [x] Equipment pages' texts through tr(), stock's keys in every language (45da92a)
- [ ] "* Wire Type" (menus.lua) untranslated: stock's equipmentlayer.starwire.choice has its star glyph (U+E035)
- [ ] Info pages break a word too long for a line mid-word (German HEAT PUMP Balance note: "Wärmepumpenausglei" / "ch")
- [x] Form prompts too wide (German sliders) wrap, buttons below the fields they push (adfc06e)
- [ ] Form prompts wrap at 380 px whatever their height on the face; stock's widths unread
- [x] HEAT LINK only on a Heat Link unit (582f6ed)
- [ ] HEAT LINK's other gate: stock's Heat Link list gives HEATING EQUIPMENT instead when its controller's vcall +0x490 is non-zero (g2 0xbb91c), unread
- [ ] Humidifier/Dehumidifier entries lag the STAR choice by one (seen headless: HUMIDIFIER shows only after a second * WIRE TYPE choice)
- [x] Desktop `--model NAME` (Display-2.7: conventional Equipment pages), Display-3.4 by default (dd0e9e8)
- [ ] Desktop's `/menu/open` leaves an open Equipment page (equipment.rs) over the menus it opens
- [x] Fan speed UI: "System fan speed" questions, timer page and running sentence (see Top menu, Eco, Fan, Mode) (95bb727)

## Setup wizard, battery and updates

- [ ] Setup wizard: none (boots to home; reset.rs "restart into setup" starts nothing). Stock: chapter page `chapterlayer` (Language, Internet connection, Your location, Equipment, Temperature, System test, Account, Finish; 6.4-8 FUN_0023a6b0), chapter→screen FUN_001b368c (language, wifi, datetime+name+interview+location, equipment, oobtemp, systemtest, phoenixoobappsetup, account), flags `oob_{startup,wifi,where,interview,wires,temp,temps,test,summary,account}_completed` (chunk_000f2e34.c:522-544)
- [ ] Wizard's start page: startuplayer "Connect to the Internet for remote control, updates, and local weather." / lowbattmsg (6.4-8 FUN_001fe7b0)
- [ ] Wizard's Equipment chapter: "Are you a Nest Pro?" HOMEOWNER (proviewselectionlayer.homeorpromsg, g2 0xfff54); heating type / fuel source (systemchecklayer, g2 0x11cf8c/0x11ce54); "Equipment detected" (wireslayer, equipmentdetectionlayer, g2 0xbb610)
- [ ] Wizard's Temperature chapter (nlOOBTempScreen, chunk_00176910.c): "begin by heating or cooling your home/business?" (awaytempslayer.heatcool*, g2 0x176c0c), Eco temps, dual fuel, humidity
- [ ] Wizard's Finish: summarylayer, Auto-Schedule or BASIC SCHEDULE, "Setup is done. It may take a few minutes…" (6.4-8 FUN_001b5420, g2 0x119410)
- [ ] Wizard's Account chapter (accountlayer.oobprompt g2 0x16af84, phoenixpairinglayer): cloud, Will's call
- [ ] Low-power shutdown: lowpoweralert (recharge / save battery) then shutdownlayer "The thermostat is shutting down." (alert LowBattery, 6.4-8 FUN_002251dc; g2 0x1780dc, 0x1168d8); thresholds unread
- [ ] Low-battery texts: tile "Low battery" (wifiscreen/templockscreen/accountscreen), screensettingslayer/updatemenulayer/proinfolayer "…because the battery is low"
- [ ] Software › Update: ours always "up to date" (`menus.lua:390`; Software hidden by `plugins/unfr0zen.lua`); stock offline: "Unable to check for a software update." + nowifi / connectwifiorpair (g2 0x14f42c); "Updated %s" line (unsure)
- [ ] Software update flow: checking/downloading/installing/failed/retry, "Attach to base to install update", battery waits (updatelayer.*, g2 0x14b720); update source Will's call
- [ ] Update speed bump: "Heating will pause during the update. Continue anyway?" LATER (updatelayer.hvacspeedbump.*, g2 0x14bb00)

## Alerts

- [x] DateTime at start, no home Wi-Fi network and the clock before 2026-01-01 (0x49740: Wi-Fi not powered, globals +0x1b0 time before the build date); DONE (common 3) / SET DATE/TIME, to Settings' Set time (bdf4865)
- [x] DateTime's conditions: Wi-Fi not powered, the clock before `BUILD_DATE`, and the time once set by hand (g2 0x49740: IsPowered, vtable +0x1b0, `date_time_adj_completed`, ours `clock.adjusted`) (edd9154)
- [x] EndOfRange: won't do: no trigger: 6.4-8 has no EndOfRange texts (a target at its end opens Mode, 0x1d8aa0, as ours); g2 posts it only from its named-alert command (0x429d8 → 0x170c04) (bdf4865)
- [x] Switchover: the ring turned while off (0x1d9880; Safety Temperatures' case, +0x94, has no setter): HEAT / COOL / HEAT • COOL by the system, OFF ticked, CANCEL (0x1b8428); a mode stops Eco and sets it (0x1b8298) (bdf4865)
- [ ] Switchover's layout: its answers in a column, scrolled to keep the selected on 408 at most (stock's list layout unread; needs stock frames); a wheel event moves one answer however many detents
- [x] TurnOffOff: Settings' Fan (3b6067c, 0x1b007c) is one of its three callers of 0x1e8d3c; the other two are Equipment's (bdf4865)
- [x] TurnOffOff from Equipment while off: SET HUMIDITY (0x13b83c) and Emerg. Heat (0x13ba54, ".equipment.emergencylayer") (281dd80)
- [x] Airwave alert on Airwave's first run, once (g2 0x4000006b: `fan_cooling_ready` unset, then set; 0x1713b0), U+E046, DONE (bdf4865)
- [x] ContactNest: won't do: cloud (event 0x40000015 from nlCZBucket's "contact-alert", 0xf19c8) (bdf4865)
- [x] HvacShortCycle alert's texts: "POTENTIAL SYSTEM ISSUE", "If your system isn't working normally, learn more about solutions at" nest.com/m13, DONE (g2 0x193e68, 0x156a60) (5fcf24c)
- [x] HvacBatteryDrained alert's texts: "SYSTEM ISSUE", "Your thermostat can't control your heating system since its battery is too low. …" nest.com/m13, DONE (0x157814) (b5bd876)
- [x] HvacBatteryDrained / HvacShortCycle look: U+E037 traced from Akkurat (a warning triangle); nlAlert's layout (0x225790, 0x418254): glyph on 77, heading lines 39 under its baseline 113, the text as `alert.block` under them (bound 159 + 39 a heading line), link 61 under, DONE 32 under the end; a longer page turns down a line a detent (bdf4865)
- [ ] HVAC alerts' layout unchecked against stock frames: the heading's style (flag 0x08000000, taken as the text's), widths past the button's bound (taken as 408), the turning down; needs stock frames (none of these alerts captured)
- [x] HVAC alerts on the way into Equipment while the short-cycle flag stands: HvacShortCycle with CONTINUE, HvacBatteryDrained with CONTINUE / BACK (0x1b007c, 0x156a60, 0x157440; +0x421 taken as HvacBatteryDrained) (edd9154)
- [x] GetCommonString(3) is OK (libnlappcore 0x2fbb0's table: BACK, DONE, CONTINUE, OK, CANCEL, MORE INFO, …): DateTime, Airwave, the HVAC alerts (edd9154)
- [x] HVAC alerts' texts: no trigger here: FurnaceShutdown's "Your thermostat noticed that your furnace shuts down within 15 minutes of heating. Learn how to troubleshoot this problem:" nest.com/furnace (g2 0x192de4) waits on HVAC control's furnace shutdown; PowerOffDueToCompressorLockout's "Your thermostat isn't getting power. Troubleshoot it to avoid losing heating." (0x31, U+E038, common 2; g2 0x177800) on Wiring's code-46 item (ours: power-off with R unpowered, when the report gives no 46) (bdf4865)
- [ ] Equipment's HvacBatteryDrained with CONTINUE / BACK: two answers in nlAlert's layout and the wheel picking one unseen on stock (ours 57 apart under the link)
- [ ] NetworkDisconnected (0x1779a0) has one answer, OK; ours WIFI SETTINGS / DONE
- [x] System Test's Furnace Heads-Up, offered by `hvac.furnace.headsup_test` with a forced-air furnace (0xcc370): RUN TEST, 15 minutes counted down, a power loss stops it with the minutes it ran (0x1c5240) (81c643c)
- [ ] `is_furnace_shutdown` from the cloud: stock's 0xbdd60 takes it (no event, no heads-up); ours only sends it

## Cloud and service features (Will's call)

- [ ] Rush Hour / Energy Programs: utilitylayer FUN_001f42a0, drcancellationalert, templayer RUSH HOUR/PEAK TIME
- [ ] Seasonal Savings (utilitylayer.ss.*)
- [ ] Remote sensors: ACTIVE SENSOR, Sensor Schedule, RCS overshoot/onboard alerts (remotecomfortlayer FUN_0019a544)
- [ ] Nest Protect shutoff + alarm alerts (topazlayer FUN_001e54b4, showtopazalert)
- [ ] Temp lock with PIN: templockscreenalert, pinlayer, minmaxlayer (FUN_001d593c); ours the unpaired stub
- [ ] Access Info / managed device (leaseinfolayer FUN_00211d14, manageddevicealert)
- [ ] Works With Nest boilers (wwnequipmentlayer, wwnboiler, g2 0x19f7a0)
- [ ] Multi-thermostat Eco START ALL/STOP ALL (ecolayer.multi.*)
- [ ] Auto-Tune done dialog (tuneupdonedialog, g2 0x1892b0)
- [ ] Demo mode (demoview FUN_000a5608, alert.demo)

## Environment

- [ ] nest-qemu: SNVS RTC alarm, so `rtcwake` and timed wakes resume the unit
