# Backplate protocol (stock nlclient)

How stock nlclient 6.4-8 drives the gen 3 backplate, which the client must reproduce; gen 2 differences at the end. Payload layouts: `src/backplate/link.rs`.

Sources: nlclient disassembly (Ghidra; addresses are Ghidra's = ELF vaddr + 0x10000), nlclient traces against nest-qemu's backplate (`ui/backplate.py --debug`) and that script's notes.

## Hardware

| | |
| --- | --- |
| UART | `/dev/ttysensormcu0` (→ `ttymxc5`), 115200 8N1, raw (`cfmakeraw`), RTS/CTS (`backplateflowcontrol` 1), VMIN 1, VTIME 1 |
| Detect / reset | `/sys/bus/platform/devices/0.sensor-mcu-gpios/detect#` → gpio141, input, both edges (1 on nest-qemu with backplate) / `…/reset` → gpio142, output, 0 at rest |
| Config | `product.config`: `backplateserialdevice`, `backplateflowcontrol`, `backplatedevice` |

## Framing

```
d5 aa 96 | type u16le | len u16le | payload | crc u16le     CRC-16/CCITT, poly 0x1021, init 0, over type+len+payload
```

- Build `0x329f4c`; check `0x32a188` (logs "crc (%04x) in msg does not match computed crc", drops frame).
- Bad CRC: frame consumed whole. Header with len > 0x400: header consumed alone.
- A bad-CRC frame claiming wires (0x04), pins (0x09) or type > 0x32, while attached: reset (CorruptType, `0x319174`).
- Every reset request is preceded by a UART break: `tcsendbreak(fd, 10)` (`0x32a100`).

## Message types

Type → event: `0x32c528`. Types with no event, dropped unseen: 0, 0x03, 0x0d, 0x0f, 0x1a, 0x20, 0x2a, 0x2c–0x2e, 0x33, 0x34, 0x37, > 0x38.

| Id | Dir | Purpose | Id | Dir | Purpose |
| --- | --- | --- | --- | --- | --- |
| 02 | ← | temperature + humidity | 82 | → | set switch (circuit, state) |
| 04 | ← | wires | 83 | → | tick / restart messages |
| 06 | ← | switch ack | 85 | → | get BP status |
| 07 | ← | near PIR (gen 2) | 86 | → | get IPS status (`00 00`) |
| 09 | ← | pins | 89 | → | set safety temp (16 B) |
| 0a | ← | ALS | 8a | → | get safety temp data (`01 00 00 00`) |
| 10 | ← | firmware id (3 B) | 8b | → | set MCU mode (u32) |
| 11 | ← | update ack | 8c | → | BP status ack |
| 12 | ← | firmware id 2 | 8f | → | wires echo |
| 13 | ← | wake-up (1 B reason) | 90 | → | get firmware id |
| 14 | ← | wake vector (2 B) | 91/92/93 | → | update begin / S-record / end |
| 17 | ← | BP status (2 B) | 98/99 | → | get FW version / FW info |
| 18/19 | ← | FW version / FW info | 9b/9c/9d | → | get BSL version / BSL info / BSL id |
| 1b/1c/1d | ← | BSL version / info / id (2 B) | 9e/9f | → | get model / serial |
| 1e/1f | ← | model / serial | a1 | → | quiet (longest sleep, s) |
| 21 | ← | IPS status | a2/a3 | → | flush / end-of-buffers ack |
| 22 | ← | buffered samples | a4 | → | wake sources |
| 25 | ← | near PIR history buffer | b1 | → | temperature lock |
| 2f | ← | end of buffers (count) | b3/b5/b6/b7/b9/ba | → | sleep sensor settings |
| 30/31/32 | ← | error vector / over-current (13 B) / thermopile errors | c0 | → | power steal mask |
| 35 | ← | safety temp data | c2 | → | temperature wake list |
| 36 | ← | MCU mode set ack | c3 | → | temp comp mode |
| | | | c4/c5/c6 | → | get error vector / OC errors / TP errors |
| | | | ff | → | reset |

## State machine

`nlBackplateController`, Nuovations OSM, built in `0x31bb94` (103 transitions). Events queue (`0x3161a4`); pump `0x31af48` runs every queued event, then one deferred event only in Idle or Error. A transition's action runs in the state it leaves (`0x31a71c`). No transition: `0x318cac` handles in place, defers (FIFO of 32; not when simulating) or drops (`0x31ad9c`). Sender: `0x3111e8(controller, type, buffer, length, drain)`.

States: Unknown, Idle, WaitingAcks, WaitingWireReport, WaitingPinReport, WaitingFwId, WaitingFwIdBsl (never built), WaitingFwVers, WaitingFwInfo, WaitingBslId, WaitingBslVers, WaitingBslInfo, WaitingSerial, WaitingModel, WaitingIPSStatus, WaitingBPStatus, WaitingUpdateBeginAck, WaitingUpdateSRecordAcks, WaitingUpdateEndAck, Error, WaitingError, WaitingErrorAcks, WaitingSafetyTempSetAck, WaitingSafetyTempData, WaitingMCUModeSetAck, Suspended, Exiting.

Events: 0 Unknown, 1 ReceiveMsgLog, 2 Temp, 3 Wires, 4 Pir, 5 AckSwitch, 6 NearPir, 7 Prox, 8 Pins, 9 Als, 10 Pwr, 11 Sensor, 12 Power, 13 FwId, 14 UpdateAck, 15 FwId2, 16 Wakeup, 17 WakeupVec, 18 Thermopile, 19 ThermopileWakeCrossing, 20 FwVers, 21 FwInfo, 22 BslVers, 23 BslInfo, 24 BslId, 25 Model, 26 Serial, 27 IPSStatus, 28 BPStatus, 29–31 TempBuf/2/3, 32 NearPirBuf, 33 ThermopileBuf, 34 PirBuf, 35 ProxBuf, 36 AlsBuf, 37 PwrBuf, 38 EndBuf, 39 ErrorVector, 40 OCError, 41 TPError, 42 PirData2 (1–42 are ReceiveMsg*), 43 ReceiveError, 44 ReceiveErrorClr, 45 AllAcksIn, 46 LastSRecordAckd, 47 GetError, 48 Reset, 49 SetSwitch, 50 ClearSwitches, 51 RequestDescription, 52 FlushBuffers, 53 BeginUpdate, 54 Quiet, 55 Tick, 56 RestartMessages, 57 TemperatureLock, 58 Exit, 59 Suspend, 60 Resume, 61 SetTempCompMode, 62 RequestIPSStatus, 63 RequestBPStatus, 64 SetSafetyTemp, 65 GetSafetyTempData, 66 ClearSafetyTempData, 67 ReceiveMsgSafetyTempData (type 0x35), 68 SetMCUMode, 69 ReceiveMsgMCUModeSetAck (type 0x36), 70 GoIdle, 71 Retry, 72 SetPowerStealMode.

Transitions. Every state but Suspended: Reset → WaitingWireReport, ReceiveMsgWires → WaitingPinReport.

| From | Event → To |
| --- | --- |
| Idle | Quiet, RestartMessages → Idle; SetSwitch, FlushBuffers, ClearSwitches → WaitingAcks; RequestDescription → WaitingFwId; RequestIPSStatus / RequestBPStatus → WaitingIPSStatus / WaitingBPStatus; SetSafetyTemp / GetSafetyTempData / SetMCUMode → WaitingSafetyTempSetAck / WaitingSafetyTempData / WaitingMCUModeSetAck; BeginUpdate / Exit / Suspend / ReceiveError → WaitingUpdateBeginAck / Exiting / Suspended / Error |
| Handshake | WaitingWireReport: GoIdle → Idle. WaitingPinReport: Pins → WaitingFwId. WaitingFwId: FwId → WaitingFwVers, FwId2 → Idle. WaitingFwVers … WaitingSerial: its answer → next wait. WaitingModel: Model → Idle |
| WaitingAcks | AllAcksIn → Idle; SetSwitch, FlushBuffers, ClearSwitches → WaitingAcks |
| Update | WaitingUpdateBeginAck: UpdateAck → WaitingUpdateSRecordAcks; there UpdateAck → itself, LastSRecordAckd → WaitingUpdateEndAck; WaitingUpdateEndAck: UpdateAck → Idle |
| Error | ReceiveErrorClr, RestartMessages → Idle; Quiet, ReceiveError → Error; GetError → WaitingError; SetSwitch, FlushBuffers, ClearSwitches → WaitingErrorAcks; Exit / Suspend → Exiting / Suspended |
| Others | WaitingError: ErrorVector, OCError, TPError → Error. WaitingErrorAcks: AllAcksIn → Error. WaitingIPSStatus: IPSStatus, GoIdle → Idle. WaitingBPStatus: BPStatus → Idle. WaitingSafetyTempSetAck, WaitingSafetyTempData: SafetyTempData → Idle. WaitingMCUModeSetAck: MCUModeSetAck → Idle. Suspended: Resume → Idle |

In place (`0x318cac`): Tick (83), TemperatureLock (b1), SetPowerStealMode (c0), SetTempCompMode (c3) send in any state; live reports (temperature, PIR, ALS, power, wake-ups, buffers, end of buffers, switch acks, IPS status) and Retry handled in any state. Deferred: Reset, switch and flush events, Quiet, RestartMessages, Exit, Suspend, Resume, status and safety requests, SetMCUMode, GetError, a first ReceiveError. Everything else out of place: dropped.

## Actions

| Event | Sends / does |
| --- | --- |
| Reset | abandon awaited (`0x31531c`), `tcflush` both ways, parser reset, break + 0xff. Hard: reset line high 20 ms instead (`0x315438`, `0x30c8a0`) |
| ReceiveMsgWires | nothing; outside WaitingWireReport = backplate reset itself (Async, `0x31551c`), abandon awaited |
| Pins / FwId (3 B) | 8f (wires echoed), 83, 90 / 98, clears reset count, comm up |
| FwVers / FwInfo / BslId (2 B) / BslVers / BslInfo / Serial | 99 / 9d / 9b / 9c / 9f / 9e |
| BPStatus (2 B) | non-zero: 8c; bit 3 raises GetSafetyTempData and holds the reset until its answer. Else reset (`0x3185a8`) |
| RequestDescription / RequestBPStatus / RequestIPSStatus | 90 / 85 / 86 `00 00` |
| GetSafetyTempData / SetMCUMode / SetSafetyTemp | 8a `01 00 00 00` / 8b mode u32 / 89 (16 B) |
| FlushBuffers | a2; zero buffered-report count |
| SetSwitch / ClearSwitches | 82 (circuit, state) per circuit |
| Quiet / RestartMessages | sleep settings then a1 (drained) / 83 |
| ReceiveError / GetError | raise GetError / c4, or c5 / c6 for the over-current / thermopile reasons the vector named (each drained) |
| Tick / TemperatureLock | 83 / b1 |
| SetPowerStealMode | c0 mask (`0x315f44`) from `power_steal_mode`: 0 normal; all ones disabled; disabled while heating: circuits of wired emergency, alternate, auxiliary, stage heat switches; disabled while cooling: wired cool stages |
| Exit | reset, reason Exit |
| BeginUpdate / records / end | 91 / 92 / 93 |

## Handshake (cold start, backplate already up)

```
c0 00000000, 85 (→ 17 0000), 83, break + ff (→ 04 wires, 09 pins), 8f <wires>, 83,
90 (→ 10), 98 (→ 18), 99 (→ 19), 9d (→ 1d), 9b (→ 1b), 9c (→ 1c), 9f (→ 1f), 9e (→ 1e),
8b 01000000, c0 00000000
```

Each request follows its answer by a few ms. Once identified (`0x305d80`): 8b mode 1 (2 under a setting), once per process (`0xb3c`), WaitingMCUModeSetAck until 36, deferred work waits; nest-qemu never answers → repeated, reset after the 6th, second identification asks nothing.

## Safety temperatures (89)

Built by `0x316c84`, `0x313d20`: u16 enable (1 heat, 2 cool); s16 heat low, high (lower safety temp ∓ 0x63d7, 16.16, centi-°C); u16 heat circuits; s16 cool low, high (upper safety temp ∓ 0x8f5c); u16 cool circuits; u16 lockout.

- Heat circuits: first wired of emergency, alternate 2, alternate 1, auxiliary, stage 3–1 heat (none if stage 1/2 heat with O/B wired: heat-pump compressor), + fan unless the heat runs its own. Cool: first wired cool stage from 3 down, fan, O/B when cooling energises it.
- Disabled side (±1000 °C): off. Wiring error: both sides disabled.
- Sent only with detect attached, comm up, unit outside the Heat Link gate (`0x305c38`; never on gen 3 / Display-3), not simulating, R powered or any equipment wired per MSG_WIRES. Sent after every description (between SetMCUMode and SetPowerStealMode) and on change of safety temps, STAR, dual fuel, O/B orientation.
- Waits in WaitingSafetyTempSetAck for 35, resending 89 on retries; the answer re-sends every switch that is on (last-switched state zeroed), unless a held BP-status reset comes first.
- Every 24 h from start (`0x3168c0`, `0x30c6b0`), all units, while attached and up: GetSafetyTempData (8a).

## Live reports

| Report | Handling |
| --- | --- |
| Temperature (`0x318724`, 4 B) | keep last valid (20 °C, 0 % before first); humidity unless 0xffff (cap 100 %); temperature unless error code. 0x8000 or humidity 0xffff → reset with line (CorruptedTempMsg); 0x8001 GrilleTempErrorMsg, 0x8002 HUTempErrorMsg, same. Then FlushBuffers; counts as the post-wake temperature |
| End of buffers (`0x316718`) | empty: ends flush. Count: ends it only if = TEMP_BUF + TEMP2_BUF + PIR_BUF + PROX_BUF + ALS_BUF reports since the flush, acked a3. Mismatch ignored; flush re-sent 5 s after it was sent |
| Switch ack (`0x316320`) | clears wait only if acked state = asked; AllAcksIn when nothing awaited and no flush out |
| ALS / wake-up (1 B) / wake vector (2 B) | visible count, 0xffff keeps last / reason 1 → FlushBuffers, 10 → ReceiveError / PIR bit = motion, 0x400 → ReceiveError |
| Errors (WaitingError) | vector adds reasons and asks details, none → clear. Over-current (13 B): faulted circuits that should be on asked on again, 10 times, then ClearSwitches + alert (clears with errors). Thermopile: inaccessible → thermopile wake off; else reset (TPError), max 5/week |

## Timing, retries, resets

- Loop (bin `0x2f51d4`): `PR_Poll` ≤ 2 s; each wake: read UART, pump, Retry if 2 s since last send (`0x3162d0`), silence check (`0x318424`).
- Starts in quiet mode; any read or send leaves it, restarting the silence watch (outside wire/pin waits) and the missed-temperature watch.
- Tick: 30 s since last Tick (83s from pins answer and RestartMessages don't count); not in wire/pin waits or quiet mode.
- Silence (TimeoutComms): 5 s with nothing that has an event (wire/pin-wait frames don't count); skipped while UART holds unread bytes, at most twice.
- MissedTemp (`0x317ccc`), after each pumped event: no temperature 35 s after a wake.
- Retry (`0x317e48`): handshake states resend their request; WaitingAcks / WaitingErrorAcks resend each unacked switch and the flush if 5 s without end of buffers (`0x31311c`); WaitingError asks errors again; WaitingBPStatus, Exiting reset; WaitingSafetyTempData / WaitingMCUModeSetAck resend 8a / 8b. 6th repeat in a row → reset (Retries). Idle, Error, wire/pin waits clear the count; nothing to repeat leaves it.
- Reset (`0x317b94`), logged with reason (Init, TimeoutComms, Exit, CorruptType, CorruptMessages, TimeoutMessage, Retries, Async, UpdateComplete, Attach, MissedTemp, I2CDown, TPError, CorruptedTempMsg, BPPowerMismatch, GrilleTempErrorMsg, HUTempErrorMsg, TempSensorIssue, TempSensorIssueDelayed). Resets 1–3: break + ff. Later: reset line if battery (first `Battery` supply's `voltage_now`) ≥ 3.7 V or caller forces. 7th: link down ("comm is down"), no resets until 10 answer (clears count, comm up) or re-attach. Every call restarts the silence watch.
- Detect line (`0x315bf4`, `0x30573c`): settles 750 ms. Detached: no frames read, none of the above runs, description forgotten, awaited abandoned, reset count cleared. Attached: comm up; first time RequestBPStatus (answer → reset, Attach), later times reset at once.

## Sleep / wake

On each imminent suspend: Quiet (deferred outside Idle/Error, never cancelled), wake settings (`0x314860`):

```
c2 <(seconds, low, high) centi-°C ...>   i16::MIN unwatched; one step, held until woken
b7 6400   b6 64006400d4ffbbff   b3 ffffffff   b9 0000ffff   ba 000000000000   a4 ef0f (wake sources)   (default sensor settings)
a1 <longest sleep, s>                    countdown to next timer, max 300
```

On waking: RestartMessages while attached.

## Switching

- After each temperature: a2; backplate answers buffered samples (22) and 2f (count); stock acks a3. b1 follows user input.
- One SetSwitch event per circuit, 82 each (deferred outside Idle and WaitingAcks: after a reset, one per turn as each ack returns Idle).
- HandleSwitching (`0x319580`): compares HVAC control's switches with last switched (`0x7b8`; zeroed by every reset, detach, backplate-initiated reset, `0x30efe4`), SetSwitch per change. Off first: water heater, emergency, auxiliary, stage 2 cool, stage 3 cool, stage 2 heat, stage 3 heat, alternate 1, alternate 2, stage 1 cool, stage 1 heat, O/B (when wired, on or off), humidifier, dehumidifier, fan, stage 2 fan, stage 3 fan. Then on: fan, stage 1 cool, stage 1 heat, stage 2 cool, stage 3 cool, stage 2 heat, stage 3 heat, alternate 1, alternate 2, auxiliary, emergency, humidifier, dehumidifier, stage 2 fan, stage 3 fan, water heater.
- SetSwitch (`0x31245c`) maps switch → circuit; stage 3 cool, fan cool, AC dehumidifier never sent. O/B (`0x330150`): `ob_orientation` while a cool stage runs, opposite while stage 1/2 heat runs, else off.
- Logs: `/media/log/nlclient`.

## Switching decisions (`nest::energy`)

Request built on every temperature report (`0x4f76ac`, via `0x510e48`, which falls back to drift for an invalid request), through controllers: safety temps (`0x51e654`), learning band (`0x51d180`), overshoot, lockouts, more. Logged `C : Tstat:… Act:… Op:…`.

| | |
| --- | --- |
| Controller | NONE, AIRWAVE, AQUASTAT, BOILER_MODULATION, COMPRESSOR_HEADS_UP, DRIFTWOOD, EMERGENCY_HEAT, HEAT_PUMP_BALANCE, LEARNING_BAND, LOCKOUT, MAINTENANCE_BAND, OVEREAGER, OVERSHOOT, SAFETY_TEMPERATURES, SWITCH_MANAGER, SYSTEM_TEST |
| Action / Op | NONE, DRIFT, HEAT, COOL / NONE, OFF, STAGE_1/2/3_HEAT, STAGE_1/2/3_COOL, STAGE_1/2_HEAT_AUX, AUX_ONLY, EMERGENCY_HEAT, STAGE_1/2_ALTERNATE_HEAT |

- **Safety** (any mode, first): heat at/below `lower_safety_temp` (when enabled) until 1 °C above; cool at/above upper until 1 °C below; SAFETY_TEMPERATURES; no minimum cycle holds them or what follows.
- **Band** (`0x51df40`): heat on while below top, starts at/below bottom; cool mirrors; heat-cool tries heat then cool. Unlearned: target ± 0x63d7 (16.16: 0.39 °C, 0.7 °F), from Rethink (`0x38183c`).
- **Overshoot** (`0x4e11ac`): ends heating at its limit; OVERSHOOT, DRIFT.
- **Lockout** (`0x51d948`; timers `0x51d5b0`; bookkeeping `0x51dc94`), last: action change → minimum cycle 300 s if compressor runs before or after, else 180 s (defaults `0x51d920`, µs). Until up, a change keeps the running action (`Lockout:MINIMUM_CYCLE Op:<wanted> Sec:<left>`) unless mode/target just changed or last decision held the compressor. Compressor stop → 150 s lockout, compressor ops become DRIFT/OFF (`Lockout:COMPRESSOR`); a shorter minimum cycle yields to it. Re-decides the moment a lockout ends.
- **Airwave** (`0x4db53c` before, `0x4db7e4` in band chain, `0x4db944` after; constants `0x663f14`): on with `fan_cooling_enabled`, a fan, cooling mode. Cycle start records time and stop temp = band bottom + learned offset (0.139–0.56 °C, 0.5 °F unlearned). After 300 s compressor, no minimum cycle holding, humidity ≤ 45 %: cooling at/below stop temp → AIRWAVE COOL/OFF, fan alone (no overshoot gate); fan runs learned time (0.017–0.17 h, 0.1 h unlearned) unless room falls to band bottom or rises 0.056 °C past stop temp; then AIRWAVE DRIFT. Publishes `fan_cooling_state` (device bucket), posts Airwave alert.
- **Equipment**: single stage HEAT → STAGE_1_HEAT, COOL → STAGE_1_COOL, else OFF; multistage by requested stage. 19 logical switches (fan, fan cool, stage 1–3 cool, stage 1–3 heat, stage 1–2 alternate heat, aux, emergency, humidifier, dehumidifier, AC dehumidifier, OB, water heater, stage 2/3 fan) → circuits by wired pins, O/B, Y2/STAR functions (`0x2e47d0`, `0x2e336c`, `0x2e3280`; `hvac/wiring.rs`).
- **Order**: on, fan before heat; off, heat before fan. Electric forced air (`heater_source`, `heater_delivery`): fan runs with heat.
- Publishes `hvac_heater_state`, `hvac_ac_state`, `hvac_fan_state`, `current_temperature` (shared bucket).

## Stock simulator (`nlbackplatesimulator.cpp`)

- `backplatesimulationrun` (or `sim_backplate_exhibit`): replaces the backplate, no frames reach the UART; never a fallback for a missing backplate.
- Over a real backplate: `backplatesimulatetempandhum` = room from sim (backplate temps only flush and count as heard); `backplatesimulationhvactype` = wiring from sim (backplate wires/pins unused, no switch sent): 1–19 terminals in `backplate/sim.rs`, negative = `…hvacwires` + `…hvacpins` (both needed; strtok tokens, untrimmed), 0, ≥ 20 or none = none.
- `…model`: `Backplate-Demo-NonAmber` Backplate-5, `Backplate-Demo-AmberOrAgate` Backplate-6.
- Keys (strtod/strtol, default): `…mode` daily (or hourly), `…buildinginefficiency` 16/65536, `…actualhumidity` 2.0, `…noisemagnitude` 0.0033, `…overundershoot` 0, `…radiantheating` 0, `…temperaturemin` 0, `…temperaturemax` 29.5, `…ambienttemperature` 10, `…starttemperature` 20.
- Start (`0x32bd40`): room = start temp (hourly) or sensor's 20 °C (daily), humidity 40 %, radiator 2.5 °C above a reading of its owner's.
- Every 30 s (`0x32bc18`, 16.16): room → ambient at inefficiency × 30 s, + equipment, + `random()` noise (seeded /dev/urandom). Ambient: hourly constant; daily (`0x32b624`) minimum until 05:00, curve overshooting past maximum, max through 13:xx, back down. Humidity → 100 × actual / (5.018 + 0.32321 T + 0.0081847 T² + 0.00031243 T³).
- Equipment (`0x32ba6c`), per second: any cooling (AC dehumidifier included) −0x222 stage 1, −0x111 fan cooling / stage 2 / stage 3; else heat +0x222 stage 1 / alt stage 1, +0x111 stages 2, 3 / alt stage 2 / aux, 0 emergency; humidifier +0x111 humidity, else dehumidifier −0x111. Radiant: effect stored, room gets what it radiates (`0x32b7ec`). Over/undershoot: effect averaged over last 10 steps.

## Gen 2

nlclient 5.9.4-5 on j49, same `nlbackplatecontroller.cpp` (frames, type→event table, states, payload lengths, retries, timeouts, update flow); addresses are the ELF's. Backplate-2.x (TFE BP_D2/BP_D2D 4.2.8) or 3.x Amber (TFE 1.0.26), BSL 3.1.

| | Gen 2 |
| --- | --- |
| UART / GPIO | `/dev/ttyO2`, no flow control (`backplateflowcontrol` 0) / `detect#` gpio65, `reset` gpio104 under `/sys/devices/platform/diamond-backplate/` (`backplatedevice`) |
| Once described | only c0 (`0x1bd160`); no SetMCUMode (`0x1c9268` no caller), no SetSafetyTemp (`0x1bd048` none); safety/STAR/dual-fuel/O-B change re-sends c0 |
| Safety data | no daily 8a; only after BP status bit 3 |
| Temperature | only 0x8000 / humidity 0xffff are errors: reset with line at once, uncounted (`0x1ca858`); 0x8001/0x8002 are temperatures; no TEMP2 sensor-issue check |
| Power mismatch | same five resets, with line, uncounted |
| Near PIR | 07 (u16) ≥ `nearpirthreshold` (15) = approaching (`0x1c3b18`); wake vector 0x80 likewise |
| Quiet | c2, `b5 <nearpirthreshold>` instead of b7/b6, b3, b9, ba, `a4 07e7` (0x567, VBAT, near PIR; no HE 0x800), a1 |
| Heat Link gate | backplate version 3 or 5, or nlmodel Display-5 (`0x4e200`) |

## Open

- What drives TemperatureLock (event 0x40000115).
- Not ported: proximity, near PIR history buffer (25), firmware update. SetTempCompMode (c3) sent only as Sunblock starts/stops correcting; Suspend/Resume only close/reopen the UART for the hex firmware update.
- How the detect GPIO and missing answers lead to "please attach" / "please reattach" and to stock's simulator.
