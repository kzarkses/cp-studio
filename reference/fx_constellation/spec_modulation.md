# FX Constellation — behavioral spec: MODULATION ENGINE & LINKS

Reconstructed 2026-11-25 from the surviving sources. Purpose: rebuild natively (Rust, CP Studio).
Code wins over roadmap; disagreements are listed in §10. `(?)` = uncertain / not recoverable.

## 0. Sources and reference keys

| key | file | state |
|---|---|---|
| LE | `S/FX Constellation/Modules/LinkEngine.lua` (4597 l.) | 2026-10-25, latest |
| MG | `.../Modules/ModGraph.lua` (1239 l.) | 2026-10-10 |
| MM | `.../Modules/ModMix.lua` | 2026-10-11 |
| NS | `.../Modules/NoteSource.lua` | 2026-10-11 |
| MD | `.../Modules/ModDrag.lua` | 2026-10-10 |
| LS | `.../Modules/LinkStrip.lua` | 2026-10-10 |
| LP | `.../Modules/LFOPanel.lua` | 2026-10-25 |
| UI | `.../Modules/UI.lua` (9410 l.) | 2026-10-25 |
| SG / SQ / GS | `SoundGenerator.lua` / `Sequencer.lua` / `GestureSystem.lua` | Oct |
| J:LFO, J:MIDI | `%APPDATA%/REAPER/Effects/CP_Mod LFO.jsfx`, `CP_Mod MIDI.jsfx` | 2026-11-09, generated |
| J:MIX, J:NOTES, J:BR | `FX Constellation - Mod Mix.jsfx`, `- Notes.jsfx`, `FX Constellation Bridge.jsfx` | 2026-10-25 |
| MJ | `CP_Engine/ModJSFX.lua` from mirror M (**2026-08-22, OLD**: 136 params; Nov bank has 147) | only copy |
| CORE / FXM | H: `Core.lua` / `FXManager.lua` (claude_file_history, end Oct) | |
| RM | `S/ROADMAP_FXConstellation.md` | latest |
| CL | `winfr_utile/fichiers/CLAUDE.md` l.940-1110 (FXC rules) | end Oct |
| T | `S/Tools/*.py|lua` — surviving harnesses = test oracles (§11) | |

`SliderShape.lua`, `ModJSFX.lua` (Nov), `QuietFX.lua`, `CP_Toolkit/Widgets.lua` (ModKnob) are LOST; their
behavior below is inferred from callers, the generated JSFX and the harnesses.

Index convention: **param indices are 0-based** (REAPER API), JSFX `sliderN` is 1-based (`sliderN` = param N-1).

---

## 1. The banks — `CP_Mod LFO` (local) and `CP_Mod MIDI` (global)

### 1.1 Identity and placement
- Two JSFX, **byte-identical DSP and slider map**; only `desc:` differs, and the MIDI one appends a 14-bit CC
  emitter at the end of `@block` (J:MIDI 1585-1682). Both declare `options:gmem=CP_FXC_FAST`.
- Names are search keys: `ModJSFX.LFO_NAME = "CP_Mod LFO"`, `MIDI_NAME = "CP_Mod MIDI"` (MJ 54-55).
  Anything containing `"CP_Mod"` is classified as a bank by `linkKind` (LE 378) and `getParamLink` (MJ 875):
  **no other internal FX name may contain "CP_Mod"** (MM 74-79, NS 48-52).
- `CP_Mod LFO` lives on **the track itself**, added at **end of chain** (`TrackFX_AddByName(..., -1)`, MJ 427-442;
  Nov: through `Core.addFX` = QuietFX, no floating window, `TrackFX_Show(idx,2)`).
  Created lazily: `LE.ensureModLFO` (LE 767) — first gesture that writes (arming a slot, linking).
- `CP_Mod MIDI` lives on the hidden track **`CP MOD`** (exact `P_NAME`, MJ 56, 671-694): created at the END of
  the track list, `B_SHOWINTCP=0`, `B_SHOWINMIXER=0`, `B_MAINSEND=0`. `LE.ensureGlobalMIDI` (LE 863).
- Files are rewritten from Lua at every window open (`refreshBankFiles`, LE 281, MJ 422); REAPER compiles at
  load, so new DSP reaches a project at its next load. **Layout is APPEND-ONLY** (links point at sliders by index).
- No `@serialize`, no `@sample`, no `midirecv` (MIDI passes through untouched; the MIDI bank adds CCs).
  All state = sliders (saved by REAPER in the project).

### 1.2 Slider map (J:LFO 3-149; identical in J:MIDI). 8 slots, `i` = 1..8
| param idx (0-b) | slider | name | range / shape | meaning |
|---|---|---|---|---|
| `(i-1)*5 + 0` | 1,6,..36 | `lfoI_on` | 0/1 `{Off,On}` default slot1=1, others 0 | slot enabled |
| `(i-1)*5 + 1` | | `lfoI_shape` | 0..7 `{Sine,Triangle,Saw Up,Saw Down,Square,Random,S&H,Steps}` | type+waveform (§1.4) |
| `(i-1)*5 + 2` | | `lfoI_rate` | 0.01..20 Hz step 0.01 **`:log`**, def 1 | free rate |
| `(i-1)*5 + 3` | | `lfoI_sync` | 0..10 `{Free,1/16,1/8,1/4,1/2,1 bar,2,4,8,16,32 bars}` | old instances: 0..6 (`slot.sync_max`) |
| `(i-1)*5 + 4` | | `lfoI_phase` | 0..1 step 0.01 | phase offset |
| `40 + i-1` | 41..48 | `-lfoI_out` | 0..1 step 1e-4, def 0.5, hidden | **THE OUTPUT** a link reads (`OUT_BASE=40`) |
| `48 + i-1` | 49..56 | `-lfoI_pho` | 0..1 hidden | raw cycle phase, display only (`PHASE_BASE=48`) |
| `56 + (i-1)*4 + 0` | 57,61.. | `lfoI_slew` | 0..2 s step 0.001 **`:sqr`** | one-pole smoother (`EXT_BASE=56`, stride 4) |
| `56 + (i-1)*4 + 1` | | `lfoI_curve` | -1..1 step 0.01 | power bend / pulse width on Square |
| `56 + (i-1)*4 + 2` | | `lfoI_mode` | 0..2 `{Loop,One-shot,Loop resync}` | |
| `56 + (i-1)*4 + 3` | | `-lfoI_trig` | 0/1 hidden, momentary | restart now; JSFX clears it |
| `88 + i-1` | 89..96 | `lfoI_uni` | 0/1 `{Bipolar,Unipolar}` | polarity (`UNI_BASE=88`) |
| `96 + i-1` | 97..104 | `lfoI_sh` | 0..8 `{Noise,LFO 1..8}` | S&H source (`SH_BASE=96`) |
| `104 + (i-1)*4 + k` (k=0..3) | 105..136 | `-lfoI_stK` | 0..16777215 int, def **8255455** | 4 steps × 6 bits packed (`ST_BASE=104`) |
| `136 + i-1` | 137..144 | `lfoI_len` | 1..16 int, def 16 | STEP length (`LEN_BASE=136` (?) name) |
| 144 | 145 | `-shp` | 0/1 | "shapes converted" marker (§3.13) |
| 145 | 146 | `-fch` | 0..255 | fast gmem channel, 0 = off (§1.6) |
| 146 | 147 | `-fmask` | 0..255 bitmask | which slots publish fast (bit i-1) |

Existence counters (an instance has a band iff `GetNumParams >= COUNT`; **one counter per band, never PARAM_COUNT**,
CL 1444-1450, memory reference-jsfx-param-bands): `BASE_COUNT=56` (phase sliders), `EXT_COUNT=88`, `UNI_COUNT=96`,
`SH_COUNT=104`, `ST_COUNT=136`, `LEN 144` (?), shapes marker 145 (?), fast 147 (?). Total today = **147**.
Default packed steps 8255455 = 31·(1+64+4096+262144): all 16 steps at 31/63 ≈ 0.49 (flat mid, so a linked
param does not plunge when STEP is chosen, RM 1336).

Step packing (MJ 515-563, J:LFO 163-323): slider `st(k)` holds steps `4k+1..4k+4`; step `4k+j+1` = `floor(v/64^j) mod 64`.
Decoded only in `@slider` into `stp[(i-1)*16 + s]`. 24 bits are exact (REAPER round-trips params in double;
measured exact to 2^28-1, RM 1478-1531).

Field → param (`slotFieldParam`, MJ 975-983): `rate = (i-1)*5+2`, `phase = (i-1)*5+4`, `slew = 56+(i-1)*4`,
`curve = 57+(i-1)*4`. Only these four fields are modulation DESTINATIONS (`FIELDS`, MG 48; curve/slew require EXT_COUNT, LE 4139).

### 1.3 Slot DSP — exact (J:LFO 325-488, repeated per slot; `N` = samples processed this pass)
Per block (or per 32-sample sub-block when fast-publishing, §1.6), for slot i, if `on`:
1. Trigger: `trig>0.5` → trg, clear `trig`. Transport rising edge (`play_state&1` from 0 to 1) AND `mode>0.5`
   (One-shot or Loop resync) → trg. On trg: `ph=0; start_bp=beat_position; done=0; cyc=-1`.
2. `oneshot = (mode == 1)`; if not oneshot `done=0`.
3. `nst = 16`, or for Steps (`shape>6.5`) `nst = clamp(round(len),1,16)`.
4. Phase: if `sync>0`: `cpb` (cycles per beat) = `{4, 2, 1, 0.5, 0.25, 0.125, 0.0625, 1/32, 1/64, 1/128}` for sync 1..10
   (assumes 4 beats/bar). Oneshot: `ph = max(0,(bp - start_bp)*cpb)`; else `ph = bp*cpb` (**phase-locked to the
   timeline: Loop resync has no effect when synced**). Free: if not done `ph += N/srate*rate` (unbounded).
   (`bp` = `beat_position` + sub-block offset × tempo/(60·srate) while playing.)
5. Oneshot end: `if ph >= nst/16 → ph = nst/16; done=1` (one cycle; for Steps, `len` steps).
6. `pp = ph + phase; pf = frac(pp)`; if done and pf≈0 → pf = 0.9999999 (hold end value).
7. Shape → `vv` in 0..1:
   - 0 Sine `0.5+0.5·sin(2π·pf)`; 1 Triangle `pf<.5 ? 2pf : 2-2pf`; 2 Saw Up `pf`; 3 Saw Down `1-pf`;
   - 4 Square `pf < 0.5 + curve·0.45 ? 1 : 0` (curve = pulse width 5..95 %);
   - 5 Random: new `rand()` when `floor(pp)` changes (per cycle), held;
   - 6 S&H: on each new cycle, `held = (sh==0 || sh==i) ? rand() : obuf[sh-1]` (another slot's CURRENT output;
     self-reference falls back to noise; slots computed 1→8 so sampling a higher slot reads its previous-block value);
   - 7 Steps: `sq = floor(pp·16)` (`ceil-1` when done); `si = sq mod nst`; `vv = stp[si]/63`.
     **Each step = 1/16 of a cycle regardless of len**; pattern loops every `len` steps.
8. Curve bend (every shape except Square): `vv = vv ^ (2^(curve·3))` (curve -1 → exp 1/8, +1 → exp 8).
9. Slew: if `slew > 0.001`: `sm += (vv - sm)·min(1, 3·N/(srate·slew))`, `vv = sm`; else `sm = vv`.
10. `out = vv`; `pho = frac(ph)` (Steps: `frac(ph·16/nst)`, done → 0.9999). `obuf[i-1] = out`.
If **off**: `out = uni ? 0 : 0.5`, `pho = 0`. **Polarity changes ONLY the rest value of an off slot**; an on
slot always outputs 0..1; the link's neutral (§3.2) does the rest (MJ header l.46-50).

### 1.4 Slot "types" (UI vocabulary over the single `shape` field, LP 105-147)
| UI type | shape values | DSP | notes |
|---|---|---|---|
| LFO | 0..4 (`SHAPES` Sine, Triangle, Saw Up, Saw Down, Square) | yes | |
| S&H | 6 (with `sh` source) ; 5 = "S&H / Noise" legacy Random | yes | 5 no longer offered, still valid forever |
| STEP | 7 | yes | 16-step editor, `len` |
| ENV | 8 | **no** (`dsp=false`) | ENVs live in the Sound Gen (slots 201..204), never in a bank |
| FOLLOW | 9 | **no** | never built; proposal: local bank only (RM 1532-1559) |
Rule: never widen the `shape` enum for a new type (old instances clamp 8→7 silently); a new type brings its own band.
Switching type writes `shape` only; links on the slot are untouched; returning to LFO restores the last waveform (LP 1706, 1728).

### 1.5 Outputs
- Link source = `lfoI_out` (param `40+i-1`), value at END of block. Phase slider is display-only.
- **MIDI bank emitter** (J:MIDI 1585-1682): per slot `vq = on ? floor(out·16383+0.5) : (uni ? 0 : 8192)`;
  when `vq != last`: `midisend(0, 0xB0, 16+i-1, vq>>7)` (MSB CC 16..23) then `midisend(0, 0xB0, 48+i-1, vq&127)`
  (LSB CC 48..55), MIDI channel 1, block offset 0. `@slider` resets all `last = -1` ⇒ **any slider write
  re-emits all 8 CCs** — used deliberately: after opening a new MIDI send, FXC re-writes `on` of the slot so
  the newly connected track receives the current value (UI 2307-2321 `armSource`).
  `ModJSFX.CC_MSB_BASE = 16` (MJ 103).

### 1.6 gmem fast publication (`CP_FXC_FAST`) (J:LFO 325-333, 412, 1574-1578; SG 1385-1560, 3100-3125; SQ 99-111, 1270-1290)
- Enabled when `fch ∈ 1..255`. Channel base `B = fch·4096` (`FAST_STRIDE=4096`).
- Header: `gmem[B+0] = fns = clamp(ceil(samplesblock/32),1,256)`, `gmem[B+1] = 32`, `gmem[B+2] += 1` (block
  counter = liveness; a consumer whose counter did not move for >4 blocks treats the bank as gone, SG `fstale`).
- Values: slot `k` (1-based) sub-step `s` at `gmem[B + 16 + (k-1)·256 + s]`; bank slots k=1..8 (only those whose
  bit is in `fmask`; masked slots run in 32-sample sub-steps, others run the per-block engine unchanged);
  **k=9,10 = Sequencer MOD A / MOD B** (`gmem[B+2064+s]`, `[B+2320+s]`, SQ 1273-1290, `Sequencer.FAST_SLOT={9,10}`).
  `B+3072..` = Sound Gen sample mailbox (`SMP_MB`, not modulation).
- The channel is per TRACK: `P_EXT:FXC_FASTCH = "<ch>|<trackGUID>"` (LE 2316-2360). Valid if 1..255, GUID matches
  (a duplicated track copies P_EXT → GUID mismatch → new channel) and no other track uses it. New channel:
  `start = guidHash(guid) % 255 + 1` with `h = (h*31 + byte) % 2147483647`, then linear probe for a free one.
  Cached per session per track.
- Only the LOCAL bank is fast-published (`planFast` uses `findBank(track,"lfo")`). Consumer = Sound Gen (§3.12).

### 1.7 The global bank reaches other tracks — exact mechanism
1. Global slot `n` (encoded `100+n`) = slot n of `CP_Mod MIDI` on `CP MOD`.
2. `ModJSFX.ensureMIDISend(mod_track, target_track)` (MJ 1137-1150): if no send CP MOD→target exists,
   `CreateTrackSend(mod, target)`, `I_SRCCHAN = -1` (no audio), `I_MIDIFLAGS = 0` (all channels→all). Returns
   true when just created (caller then re-emits, §1.5). Called from `resolveSource` (LE 650-656) and `armSource`.
3. The destination parameter gets a **MIDI plink** (§3.3 recipe G): `effect=-100, midi_bus=0, midi_chan=1,
   midi_msg=176, midi_msg2=128+cc` with `cc = 16+n-1` (`128+` selects the 14-bit MSB/LSB pair).
4. ⚠ A MIDI link only sees MIDI ENTERING the FX that carries it. Behind an instrument that does not pass MIDI
   (the Sound Gen is one) the CC never arrives → this is why the sum bus sits at chain HEAD (MM 31-41).
   A single global source linked DIRECTLY on an FX placed after such an instrument does not hear it (?) —
   the code does not special-case this.
5. Reading the value (display) is a different address: `TrackFX_GetParam(CP_MOD, bankIdx, 40+n-1)` (LE 674-750
   `resolveReadback`; "two resolvers, not one", RM 312-317).
6. Cross-track: a native plink never crosses tracks. On the CP MOD track, a global slot FIELD can only be driven
   by another global slot (direct plink, same track), and only ONE source (no bus on CP MOD) (MG 39-41, 680-682;
   LE 4177-4184 "a global modulator can only follow another global modulator").

### 1.8 Bank access semantics (MJ 444-669; Nov changes inferred)
- Reads/writes are in **raw slider units** for linear sliders (`TrackFX_GetParam/SetParam`), but RATE (`:log`)
  and SLEW (`:sqr`) MUST go through the normalized path (`ModJSFX.writeField/readField`, CL 1018-1030): on a
  shaped slider GetParam/SetParam speak ln(v) / sign·√|v| and are unbounded (SetParam(1) on RATE = e¹ Hz).
- `getSlot` → `{on, shape, rate, sync, phase, out, ph, slew, curve, mode, uni, sh, len, sync_max}`; band-less
  fields are `nil` (UI greys the control) — never `false`.
- `trigSlot` = write 1 to `trig` (needs EXT_COUNT). `randomizeSlot` (Rnd chip): on=1; shape random 0..5 (?
  Nov may differ); rate log-uniform 0.05..10 Hz (forces Free) or random sync 1..6, coin flip when both; phase random.
- `repairBank` (Nov): fixes out-of-range values written in the wrong domain (2026-10-08 incident).

---

## 2. Source address space — `LinkEngine.bandOf` is the ONLY decoder (LE 87-215)

| encoded slot | band, n | output it reads (track, fx, param) | neutral (`sourceCenter`, LE 514-528) | color (MD 63-157) | short label (UI 3258-3288) |
|---|---|---|---|---|---|
| 0 / absent | pad | (not a slot: means "follow pad X/Y per chips") | gesture_base | — | XY / X / Y |
| 1..8 | lfo, n | own track, `CP_Mod LFO`, `40+n-1` | uni slider ? 0 : 0.5 | PALETTE[n] | L1..L8 |
| 101..108 | global, n | link: MIDI CC `16+n-1` (14-bit); read: CP MOD bank `40+n-1` | idem (CP MOD bank uni) | PALETTE[n]·0.68 | G1..G8 |
| 201..204 | env, n | Sound Gen `SG_ENV_OUT = {31, 63, 68, 73}`[n] (table, not arithmetic) | 0 | ENV_PALETTE | E1..E4 |
| 211..218 | macro, n | bridge `macroN_out` = `14+n-1` (`MACRO_OUT_BASE=14`) | 0 | PALETTE[n]·0.82+0.16 | M1..M8 |
| 221..222 | seq, n | Sequencer `MOD_OUT = {16,17}` (MOD A/B) | 0.5 | SEQ_PALETTE | S1..S2 |
| 231..234 | note, n | Notes JSFX param `n-1` (NOTE, VEL, RAND, OCT) | RAND 0.5, others 0 (`NS.BIPOLAR`) | NOTE_PALETTE | NT VL RN OC |
| -1 / -2 | pad axis X / Y (ModDrag only) | bridge `x_out`=3 / `y_out`=4 | 0.5 | theme accent | Pad X / Pad Y |

Decoding order is the contract (LE 173-215), tested **highest first**: `>230` note (max 4) → `>220` seq (max 2) →
`>210` macro (max 8) → `201..209` env (max 4) → `>100` global (max 8) → `>0` lfo (max 8) → `0` pad. Out-of-band
values (e.g. 205, 209, 210, 9, 109, 223, 235) return nil (= unreachable; a link to a nonexistent index "fails
silently and reads back as healthy", so validation matters). Negative → nil (`-1/-2` handled separately).
**201 never renumbers** (on disk in clear). Readability guards: macro needs bridge `GetNumParams >= 22`
(`MACRO_BAND_COUNT`); ENV 2..4 need SG `>= 74` (`SG_EXTRA_COUNT`); seq/note need `GetNumParams > out`.
`BRIDGE_PARAM_COUNT = 35`, `ENGINE_BAND_COUNT = 31`, `ZONE_BAND_COUNT = 35` (GS 524-531).

Bridge slider map used by links (J:BR): `x_pos 0, y_pos 1, slew 2, x_out 3, y_out 4, xy_mix 5, macro1..8 6..13
(MACRO_IN_BASE=6, the DESTINATION of a macro), macro1..8_out 14..21 (the SOURCE), eng_mode 22, eng_mask 23,
... zone 31..34`. Outputs are recomputed each @block (slewed by `slew`): **a link must target the INPUT (x_pos,
macroN), never an output** (it would be overwritten next block, LE 1562-1566, 4146-4147).

Source availability ("arming", UI 2278-2324): dropping an lfo source → `ensureModLFO` + turn the slot ON if off;
global → ensure CP MOD + bank + MIDI send, then write `on` (re-emission) if the send was just opened or slot off;
macro/env/seq/note → nothing to arm (a macro at 0 = no effect; ENVs publish 0 without notes).
`notes` band: resolving it sets `state.notes_wanted`, and `notes_missing` if the Notes FX is absent (→ inserted
outside the sweep, §6).

---

## 3. The native link (plink) as written to REAPER

### 3.1 API (all via `TrackFX_SetNamedConfigParm/GetNamedConfigParm`, values as strings; LE 288-302)
- `param.<P>.plink.active` "0/1", `.effect` (source FX index on SAME track, `-100` = MIDI, `-1` = none),
  `.param` (source param idx), `.offset`, `.scale`, `.midi_bus` (0), `.midi_chan` (1), `.midi_msg` (176 = CC),
  `.midi_msg2` (128+cc for a 14-bit pair; readers also accept plain `cc`).
- `param.<P>.mod.active` "0/1", `param.<P>.mod.baseline` (normalized base).
- Native REAPER per-param LFO (still exposed in a menu, §7.6): `param.<P>.lfo.active/shape/speed/strength/temposync/dir`
  (enable: seed `mod.baseline = base_value` only if `mod.active != 1`, `mod.active=1`, `lfo.dir=0`, `lfo.active=1`;
  disable: `lfo.active=0`, and `mod.active=0` only if no ACS and no owned link, LE 1486-1525). `param.<P>.acs.active` read only.
- Write order used everywhere: source keys (`effect`,`param` or the 5 MIDI keys) → `offset` → `scale` →
  `mod.baseline` → `mod.active=1` → `plink.active=1`.

### 3.2 Math (REAPER's engine, used verbatim by FXC for display)
```
value = clamp01( baseline + (src + offset) * scale )         src = source param normalized (our sources are 0..1 linear sliders)
offset = -neutral   (neutral = link polarity "u"→0, "b"→0.5, else sourceCenter(slot))
scale  = signed depth in the DESTINATION's REAPER-normalized space
```
(An automation envelope on the destination REPLACES the baseline, it does not add — RM 1820-1837.)
Excursion window drawn (`modWindow01`, LE 3416-3423): `lo = base - c·scale`, `hi = base + (1-c)·scale`, each clamped
0..1; negative scale gives lo > hi (reading direction = source's). Bipolar → centered on base; unipolar → starts at base.
Depth in units (`depthInUnits`, LE 3455-3465): for a linear slider `[min,max] ≠ [0,1]`: `d = scale·(max-min)`,
excursion `(-c·d, (1-c)·d)`; returns nil for 0..1 / normalized-only params (VST Cockos: always 0..1).

### 3.3 Link recipes (who writes what)
| recipe | destination | source | offset | scale | baseline | code |
|---|---|---|---|---|---|---|
| A param, 1 source (non-pad) | FX param | slot output | `-center` | `modSpanEff(1)` | `param_data.base_value` | LE 3065-3209 |
| A' same, global source | FX param | MIDI CC (recipe G keys) | `-center` | idem | idem | LE 3157-3164 |
| B param, pad only | FX param | bridge `x_out`/`y_out`/`xy_mix` (both chips → mix) | `-gesture_base_x` / `_y` / avg | `modSpanEff(1)` = span | `param_base_values[key]` (anchor) | LE 3080-3092, 3168-3198 |
| C param on a bus | FX param | Mod Mix `mixB_out` (param `b-1`) | **-0.5** | **2** (fixed) | `base_value` | LE 3056-3064, 3128-3135 |
| D bus input k | Mod Mix `mixB_inK` (`16+(b-1)·4+k-1`) | source output or MIDI CC | 0 | 1 | 0 | LE 2101-2128 |
| E depth voice input | Mod Mix `dmD_in` (`216+d-1`) | source | 0 | 1 | 0 | LE 2852 |
| F pad axis driven | bridge `x_pos`(0)/`y_pos`(1) | slot output / MIDI | 0 | zone width `x1-x0` | zone low `x0` | LE 1670-1743, 1612-1632 |
| G MIDI keys | — | — | `effect=-100, midi_bus=0, midi_chan=1, midi_msg=176, midi_msg2=128+cc` | | | |
| H internal port, 1 source | bank field / macro input / seq param | slot output (same track) or MIDI | `-center` | route depth (0 if bypassed) | kept, else `GetParamNormalized(dest)` | MG 525-576 |
| I internal port on bus | idem | Mod Mix out | -0.5 | 2 | kept / current | MG 580-604 |

Pad axis (F): zone from `pad_zone_x0/x1` (`y0/y1`), ordered, clamped 0..1; a driven axis rescales the source into the
zone so `x_pos = x0 + src·(x1-x0)` (full cycle inside the zone). `pushPadZone` rewrites scale/baseline only when the
zone changes. Respects `jsfx_automation_enabled` ("Auto JSFX" coupling): off → axis links released. Unresolvable
source → release link, keep the INTENT (`pad_src_x` stays; only the fact flag `pad_link_x=false`). `pad_link_x/y`
flags are published ONLY by `syncPadAxes` (every exit writes them). Live axis value is read from the SOURCE
(GetParam on a modulated param returns the baseline): `padAxisValue = x0 + src·w` (LE 1767-1775).

### 3.4 Depth, range, invert, gesture_range (LE 1037-1049, 1110-1133; FXM 633-665)
- Source rank 1 (first): `span = param_ranges[key_range] (default 1.0) × gesture_range (default 1.0)`, negated
  if `param_invert[key_invert]`. Editing span: `v = span/g` clamped ±1 → `range = |v|`, `invert = v<0`.
- Ranks 2..4: `param_mod_more[key][i-1].depth` (signed, ±1) × gesture_range.
- New extra source depth = the param's range (`addModSource`, LE 1181) — "a value we already hear". First drop on
  an SG PITCH param sets `PM_DEFAULT_RANGE` first (default 1.0 = ±63.5 st would be 10 octaves, UI 2626-2638).
- Removing rank 1 promotes rank 2 with its depth into range/invert (LE 1242-1251).
- `pushDepth` (LE 1793-1810): on a bus → write bus `amt(b,1)`; else rewrite `plink.scale` of the owned link.
- Bypass-aware everywhere: compile sites use `modSpanEff` (0 when bypassed); EXCEPTION: the pad input of a bus uses
  `modSpan(1)` (pad is not bypassed with the first source).

### 3.5 `syncLinks` — the sweep (LE 2889-3365). Event-driven: runs when `links_dirty`, mouse up, no Capture pass (UI 9142-9145)
1. Skip entirely if chain not yet re-read (`last_fx_count == -1 and fx_data empty`) — keep flags.
2. Consume `links_dirty`, `links_rebuild` (preset/snapshot load: rewrite every wanted link from FXC state, sticky
   neutrals rewritten), `links_reanchor` (migration of a track that never had pad links: `gesture_base := gesture_x/y`,
   bases := audible values).
3. `want_links = (pad_mode == 0)` (granular pad = script-driven, no links). Ensure/upgrade bridge (create only if
   `auto_jsfx`; upgrade of an existing short instance always). Bridge missing/short → `want_links=false`.
4. On rebuild/reanchor: snap bridge `x_pos,y_pos,x_out,y_out,xy_mix` to the pad position BEFORE writing links (no slew ramp).
5. `resetSourceCache`; `notes_wanted = notes_missing = false`.
6. `planFast` (§3.12) BEFORE `planMix` (a fast param takes no bus and no native link).
7. `planMix` (§5.4) → bus index or -1, `_mx_of[key] = b`.
8. For each param of `fx_data` (visible params of the current track's FX list):
   - selected and not fast: slot>0 and has bus → `wireBus` + recipe C; slot>0 → recipe A/A' (unresolvable source →
     no link); slot==0 and want_links and X/Y chip → recipe B.
   - **Topological "intact" test** (skipped on rebuild): active and (`effect==src_fx and param==src_param`) or
     (`effect==-100 and midi_msg2 ∈ {128+cc, cc}`). Intact → **scale and baseline untouched** (panel/raw edits survive);
     only fix `offset` to `-center` (non-pad) and `scale` to 2 (bus). Not intact → full write (§3.3).
   - not wanted → release if owned: `bridge` or `mix` always; `cpmod` only if `param_mod_source[key] ~= nil`.
     Releasing a pad link: `SetParamNormalized(rest)` with `rest = param_base_values[key] or base_value`, snapped to
     the param's steps; `base_value` / `param_base_values` are NOT rewritten (§3.7).
9. Write fast routes (SG/bank/seq channel + routes + mask; 0 when none).
10. `ModGraph.syncInternal` (internal ports) then `_syncVoices` (depth voices).
11. Clear buses no longer claimed (only those used last sweep, or all on a first-seen instance); `mix_unused = no claim`
    (unless refused on this track); `notes_unused = Notes present and not wanted` (unless refused).
12. `links_active`, `links_count`, `lfo_links_count`; `applySlew` (bridge slew = `SetParamNormalized(2, link_slew/2)`);
    `syncPadAxes`; on reanchor persist immediately.
Restructuring (insert/remove FX) is NEVER done in the sweep, nor during a gesture (CL 1355-1360): the sweep raises
`mix_missing/mix_unused/notes_missing/notes_unused` and the window loop acts (§5.5).

### 3.6 Polarity of a slot change → `rewriteSlotOffsets(slot)` (LE 1372-1395)
The sweep is topological, so changing a slot's Bipolar/Unipolar would leave old offsets. On toggle (`ctx.repolarize`):
for each selected param whose first source is `slot` and with no per-link polarity → `plink.offset = -newCenter`
(offset only). Params on a bus: `links_dirty` (neutral lives in the bus `ctr`).

### 3.7 Release semantics (RM 1620-1626 rule 6; LE 3220-3279; MJ 941-967)
`releaseParamLink`: read baseline; `plink.active=0`; `plink.effect=-1`; `mod.active=0` unless the param has a native
LFO or ACS active; `SetParamNormalized(baseline)`; emit ExtState hint `CP_Mod/unlink = "<trackGUID>|<fx>|<param>|<time>"`.
**The value returns to its BASE and the base never moves** ("a stable model that clicks a little beats
unpredictability" — Cédric 2026-08-22). Bus inputs are released WITHOUT the hint/reset (`releaseInput`:
`active=0, effect=-1, mod.active=0`, LE 2133-2138).

### 3.8 Ownership, adoption, external edits
- `linkKind` (LE 343-390): `nil` if inactive; MIDI link with `msg2` in CC 16..23 (or 144..151) → `cpmod`;
  source FX name contains "FX Constellation Bridge" → param 14..21 → `cpmod` (macro) else `bridge` (pad);
  Mod Mix → `mix`; contains "CP_Mod" / "Sound Generator" / Sequencer name / Notes name → `cpmod`; else nil
  (**user property, never touched**).
- `reconcileModSources` (after each scan, not on rebuild; LE 1836-1929): adopt `cpmod` links of kind lfo/global
  (`ModJSFX.getParamLink`; ENV/macro/seq/note links are NOT adopted — known gap RM 1786) → set `param_mod_source`,
  mark selected, pull `baseline` into `base_value`/`param_base_values`, pull `scale` into `range=min(1,|scale|/g)`,
  `invert=scale<0`. **The link is the authority at scan time.** Leak sweep: params filtered out of `fx_data` keep
  links → release `bridge`/`mix` always, `cpmod` only if our bookkeeping key exists.
- Cross-process hints (standalone CP_ModLFO panel): `CP_Mod/unlink` → clear our `param_mod_source` entry;
  `CP_Mod/edit` (`"guid|fx|parm|time"`) → pull source/base/range/invert live, persist after 0.75 s of quiet (LE 1936-2025).

### 3.9 `snapshotLinks` / `repairLinks` / `retireFX` (LE 4486-4595)
- Links address their source by **chain position** (`plink.effect`): any insert/delete shifts them.
- `snapshotLinks(track, skip)`: for every FX ≠ skip, every param with `plink.active=="1"` and `0<=effect<n_fx`,
  `effect≠skip`: record `{TrackFX_GetFXGUID(fx), param, TrackFX_GetFXGUID(effect)}`; `drives=true` if `effect==skip`.
  MIDI links (-100) are not recorded (index-free).
- `repairLinks(track, keep)`: after the change, find both FX by GUID; if `plink.effect ≠ srcIndex` rewrite it.
  Assumes nothing about REAPER re-indexing (harness `fxc_retire_test.py` plays both worlds).
- `retireFX(track, idx)` refuses (returns false + English reason for the status bar) if: any param of the FX has an
  envelope ("it has automation"); any param has `plink.active` or `mod.active` ("one of its controls is modulated");
  any link uses it as source ("it drives a link"). Then `TrackFX_Delete`, verify count decreased ("REAPER did not
  remove it"), `repairLinks`. A refusal is remembered per track (not re-asked each sweep).
- Used by: `ensureMix`, `upgradeMix`, `retireMix`, `ensureNotes`, `retireNotes`, Sound Gen / Sequencer removal.

### 3.10 Internal destinations (`linkInternal` & co., LE 3883-4460)
- Targets: `fieldTarget(bank,slot,field)` → (track, bankIdx, slotFieldParam) — bank "lfo" = current track, "global"
  = CP MOD; curve/slew need EXT band. `macroTarget(n)` → bridge `MACRO_IN_BASE+n-1` (the input, never `_out`).
  Sequencer: params in `Sequencer.MOD_P` (continuous settings only: step values of non-bool lanes, lane LO/HI, MUT,
  DICE, swing, transpose — SQ 420-449).
- `resolveInternalSource(track, fx, src)` (LE 4166-4229) → `(eff, sp, cc, center)` or `(nil, reason)`; **never
  creates/upgrades FX** (we are inside a drop). Pad axes -1/-2 → bridge `x_out`/`y_out`, center 0.5 (refused on CP MOD:
  "the pad lives on the track, not on CP MOD"; no bridge: "no bridge on this track (Auto JSFX)"). Destination on
  another track (CP MOD) → only `global` band (direct to the bank's own OUT). Global from the current track → MIDI.
  env → SG (refuse "no Sound Gen on this track", "this Sound Gen predates ENV 2-4 (reload the project)"), macro →
  bridge out, seq → `MOD_OUT`, note → Notes FX ("the note sources are being added to this track" while missing),
  lfo → local bank ("no CP_Mod bank on this track").
- Self-source refused: a bank field refuses its own slot (`slot` or `100+slot`), macro n refuses `210+n`, Sequencer
  MOD A/B lane settings refuse 221/222 — "a source cannot drive its own settings" (MG 202-225).
- `linkInternal(track,fx,parm,src,depth,self_src)` (LE 4371-4419): converts a not-yet-shape-converted bank first;
  then **ADDS through ModGraph** (`G.add(port, src, depth)`); legacy fallback writes recipe H directly
  (baseline = current normalized, scale = depth or 0.5).
- `setInternalBase` writes `mod.baseline`; `setInternalDepth` sets depth of the CURRENT route (`G.setSpan`);
  `releaseInternal(all)` removes current route or clears all.

### 3.11 Sum-bus & depth-voice wiring helpers
- `wireBusList(track, mix, b, list, n)` (LE 2148-2169): for k=1..4: if source reachable → `wireInput` (topological),
  `amt(b,k) = clamp(span,±1)`, `ctr(b,k) = center` (sticky pad center only when freshly wired or forced); else
  release input, `amt=0` (an unreachable source keeps a mute input; the others play).
- Raw writes only if changed by > 0.00006 (slider step 1e-4) (`setRawIfDiff`, LE 2091).
- `pushMixAmt(key, i, span)` writes `amt` immediately for wheel/drag gestures; `_pushVoiceAmt` for voices.

### 3.12 Fast routes (bank → Sound Gen, faster than the block) (LE 2296-2436, SG 1385-1560, 2185-2275, 3100-3125, 4010-4100)
Why: a plink carries ONE value per block (512 @48k = 94 values/s; a 20 Hz LFO aliases, RM 2399-2427).
- `planFast`: requires SG with fast band (`GetNumParams >= 132`), local bank with fast band (147) and/or Sequencer
  fast band (`>= 219`, plus SG `>= 147` = `SEQ_FAST_COUNT`). For each SG param `p ∈ FAST_DST` selected with ≥1 source
  and `FAST_CODE[p] <= fastMaxCode` (15 / 18 / 19 from the max of the dst slider range): ALL its sources must be band
  `lfo` (bank present) or `seq` (seq ok); no depth modulation; total routes ≤ 8. Then the param has NO native link;
  routes `{code, slotK (lfo n, or seq 9/10), modSpanEff(i), linkCenter}`; mask bit `n-1` per lfo slot.
- SG sliders (index): `fch = 99`; route r (1..8) at `100+(r-1)·4`: `dst` (0..19), `src` (1..10), `amt` (-1..1, normalized
  depth of the replaced link), `ctr` (0..1). Write src, amt, ctr, then dst last; only when changed (tol 6e-5).
- `FAST_DST` (SG param idx, code 1..19): `{86,87,88, 42, 43, 18,23,29, 4, 3,22,28, 5,24,30, 38,39,40, 132}` =
  osc1-3 pitch `pm` (st, ±63.5), cutoff (`:log` 20..20000, applied in octaves: `en = log(cut/20)/log(1000) + off`),
  resonance, osc1-3 volume, amplitude, osc1-3 width (cents 0..100), osc1-3 color, osc1-3 pan (±1), osc1 FM index.
- DSP every 32 samples: `n = stale>4 ? 0 : gmem[B]`; `v = gmem[B+16+slot0·256+min(k,n-1)]`; `OFF[code] += amt·(v-ctr)`;
  `value = min + clamp01((base-min)/(max-min) + OFF)·(max-min)` (= what a native link would do). Gains/vol/amp/FM ramp
  over the sub-step then 2×1.5 ms poles; pitch/width/color change per tick. No route → bit-identical to before.
- Live display of a fast param: `base + Σ span_i·(v_i - ctr_i)` clamped (preview at UI rate).
- Not fast: third-party plugins (only the buffer size helps), SG params outside FAST_DST, ENV/macro/global/note sources.

### 3.13 Slider-shape migration (RATE `:log`, SLEW `:sqr` since 2026-10-07) (LE 3907-4122; T fxc_shape_test.py 199-217)
Old links on these sliders store LINEAR normalized numbers. Once per instance (marker slider `shp`), all tracks incl.
master and CP MOD, at startup/new project: measure first (`SliderShape.verify` on a FREE param; mismatch → touch nothing,
message "Slider shapes: REAPER disagrees on <fx> - old links left untouched"); then in ONE undo block "FX
Constellation: links follow the new slider shapes":
`convertLink(shape,min,max,base_lin,scale_lin,center)`: `nb = toNorm(shape, denormLin(base_lin))` (base exact);
`top = denormLin(base_lin + (1-c)·scale_lin)`; `ns = (toNorm(shape, top) - nb)/(1-c)` (**the TOP of the modulation is
kept; the trough becomes its mirror in the curve**; sign kept). Envelopes converted point by point (`fromLinear`).
Then `repairBank` (values written in the wrong domain). `:log`: `v = min·(max/min)^n`; `:sqr`: `v = (n(√max-√min)+√min)²`.

### 3.14 Other link utilities
- `releaseAllLinks()` (LE 4462): status-bar button "Release all links" — release every OWNED link of the current track,
  cancel the pending sweep (`links_dirty=false`), return count ("N link(s) released" / "no link to release").
  Never restructures. ≠ `stopAllEngines()` (LE 3813): sets bridge `eng_mode` (param 22, RAW `SetParam(0)`) on every
  bridge of the project incl. master (`>= 31` params) — links stay.
- `lfo_meta` (legacy, LE 2037-2070): ROLL-created pad→local-slot RATE meta-modulation at defer rate:
  `rate = clamp(0.01,20, base_rate·2^(2·off))`, `off = 2·(gesture - gesture_base)` on x / y / avg, optional invert;
  only `m.bank=="local"`. Coexists with native field links (RM 2151-2163 says it should become a native link).

---

## 4. ModGraph — the single link model (MG)

### 4.1 Ports and keys
| kind | key | resolves to | store |
|---|---|---|---|
| param | `p:<paramKey>`, paramKey = `<trackGUID>_<fx full_name>||<param name>` (CORE 340-358) | FX param | **delegated** to LE (`param_mod_source`, `param_mod_more`, range/invert, `param_mod_pol`, `param_mod_off`) |
| field | `f:<lfo|global>:<slot>:<rate|phase|curve|slew>` | bank field (§1.2) | P_EXT of the track that hosts the bank |
| macro | `m:<n>` | bridge `macroN` input | current track P_EXT |
| seq | `q:<seqParamIdx>` | Sequencer param in `MOD_P` | current track P_EXT |
| route (depth of a link) | `r:<parentKey>|<src>`, nests: `r:r:p:...|1|211` | Mod Mix depth voice | P_EXT of the ROOT's track (param root → current track) |
Source is parsed from the END of the key (`^r:(.*)|(%-?%d+)$`); the root is computed once at port creation.

### 4.2 Store — `P_EXT:FXC_ROUTES` (MG 265-371)
Format: `v1;<key>=<src>@<depth %.4f>[@u|@b][@x],<src>@...;<key>=...` (keys sorted). Max **4** routes per port,
`depth` clamped ±1, src ≠ 0 (may be negative: pad axes). `@u/@b` = per-link polarity; `@x` = bypassed (old versions skip
unknown flags safely). Example: `v1;f:lfo:3:rate=2@0.2500,211@-0.1000;m:2=1@0.5000`.
First visit of a track without P_EXT imports existing internal plinks once (`readInternalPlink` → `{src, depth=scale}`),
then writes the P_EXT (even empty). Re-read at most 2×/s to catch undo/other windows (`refresh` → `links_dirty`).

### 4.3 API (same for every port; sources addressed by RANK 1..n)
`count, src(i), span(i)` (signed depth, port-normalized), `pol(i)` (`u`/`b`/nil), `center(i)` = polCenter(pol) or
sourceCenter(src) or 0.5, `off(i)` (bypass), **`spanEff(i)` = off ? 0 : span** (what compiles), `indexOf(src)`,
`cur/setCur` (current source — legacy, now only for LinkStrip "select"), `add`, `remove`, `clear`, `setSpan`, `setOff`,
`setPol`, `setAll`, `list`.
- `add(p, src, depth)` (MG 635-691): param → `LE.addModSource`. route → append (default depth **0.25**), no reachability
  check (the sweep wires it, mute if unreachable). field/macro/seq → refuse self-source; check reachability with
  `resolveInternalSource` (reason returned); already present → becomes current; max 4 ("four sources at most on one
  setting"); global field: max 1 ("a global modulator setting follows one source"); default depth = given, else first
  route's depth, else **0.5**; then `syncPort` (compile now: 0 → release, 1 → direct; ≥2 → the sweep builds the bus).
- `remove(p,i)`: last one releases the port (value back to its base). `setSpan`: direct link → write `plink.scale`
  immediately; on bus → `pushMixAmt`; route → `_pushVoiceAmt`. `setOff`: depth kept, plays 0, recompile.
- Param ports: `LE.addModSource` (LE 1160-1187): no source → becomes first (rank 1, depth = range); present → current;
  4 max ("four sources at most on one parameter"); else append with depth = param range. `setParamModSource(...,0)`
  clears ALL sources and `cur` (no bus without a first source). Every write bumps `mod_gen` and persists.

### 4.4 Compilation of internal ports (`syncInternal`, MG 866-909)
1 route and no bus → recipe H (`writeDirect`, topological: same source → keep baseline, but scale/offset rewritten if
different — the model owns them). Bus assigned → `wireBusList` + recipe I. ≥2 routes but no bus available → first source
direct, others wait. CP MOD's global fields → direct, first route only. A port with 1 route but ≥1 depth modulation
also needs a bus (`eachBusPort`, `hasDepthMod`: "a direct link has no modulable scale").

### 4.5 Links on links (depth modulation) (MG 995-1133, LE 2682-2885, MM 54-69)
- The depth of link `(P, src)` is port `r:P|src`, which has up to 4 routes, each of which has its own depth port, etc.
- Effective depth of input/voice e: `amt(e) + Σ voices targeting e`. Compiled into the bus's **8 depth voices** (§5.2).
- `_syncVoices`: prune orphan depth ports (parent link gone) **to a fixed point** (pairs order is random); collect nodes
  `(depthPortKey, src)`; order: keys sorted, roots = depth ports whose parent is not a route; DFS so that **what modulates
  a node gets a LOWER voice index than the node** (computed first in JSFX, no block latency). Voice d > 8 → `mix_full`.
  Target = `tgtInput(b,k) = (b-1)·4+k` (k = rank of the source in the parent port; must be ≤4) or `tgtVoice(d') = 64+d'`.
  Voice wiring: input = recipe E; `dm_tgt`, `dm_amt = dead ? 0 : depth`, `dm_ctr = polCenter(pol) or sourceCenter`.
  Unused voices released (`tgt=0, amt=0`).
- **Dead voice**: a voice whose own link is bypassed, or any link above it is bypassed (`linkDead`, recursive ≤8):
  stays wired at amt 0 (a voice ADDS to its target even if the target amt is 0, so bypassing an input alone would not
  silence its depth modulation).
- Pad axes may modulate a depth (src -1/-2 → bridge x_out/y_out, center 0.5).
- Display: `depthDelta(pkey, rsrc, read)` = Σ (depth + depthDelta(child)) · (v - center), recursion guard 6.
- Live value of an internal port (`ModGraph.live`): `clamp01(baseline + Σ_i (depth_i + depthDelta_i)·(v_i - center_i))`
  (bypassed skipped); baseline re-read every 0.25 s.
- Presets store depth ports RELATIVE to their parent (`depthSnapshot`: chain `|1|211`; `depthRestore` rebuilds
  `r:r:<newParentKey>|1|211`, removing the port's previous depth ports first — "a preset says everything we hear").

### 4.6 Reverse lookup
`LE.destCount(slot)` (LE 1415-1455): count of SELECTED params whose first or extra source is `slot` (cached by
`mod_gen`/`sel_gen`/`fx_data` identity), +1 per pad axis driven by it, + `ModGraph.internalDestCount(slot)` (routes
on the current track, plus CP MOD's only for global sources). Shown on rack tiles ("1".."8", "8+").

---

## 5. The Mod Mix sum bus (`FX Constellation - Mod Mix`)

### 5.1 Slider map (J:MIX 6-245; MM 82-113). B=16 buses, K=4 inputs, D=8 voices. All hidden (`-`), step 1e-4
| param idx | name | range, default |
|---|---|---|
| `b-1` (0..15) | `mixB_out` | 0..1, 0.5 — the ONLY thing a destination links to |
| `16 + (b-1)·4 + k-1` (16..79) | `mixB_inK` | 0..1, 0 — copy of source k |
| `80 + (b-1)·4 + k-1` (80..143) | `mixB_amtK` | -1..1, 0 — signed depth |
| `144 + (b-1)·4 + k-1` (144..207) | `mixB_ctrK` | 0..1, 0.5 — neutral of source k |
| `208 + d-1` | `dmD_tgt` | 0..72 int, 0 = off, 1..64 = input `(b-1)·4+k`, 65..72 = voice `t-64` |
| `216 + d-1` | `dmD_in` | 0..1 |
| `224 + d-1` | `dmD_amt` | -1..1 |
| `232 + d-1` | `dmD_ctr` | 0..1, 0.5 |
`V1_COUNT = 208` (first layout, no voices) ; `PARAM_COUNT = DM_COUNT = 240`.

### 5.2 Formula (J:MIX 247-369), per block, no audio/MIDI touched
```
ae[e] = amt(e)  (e = 0..63) ; de[d] = dm_amt(d)  (d = 0..7)
for d = 1..8 (in order):  t = round(dm_tgt(d)); if t > 0:
      v = de[d-1] · (dm_in(d) − dm_ctr(d));  if t ≤ 64: ae[t−1] += v  else de[t−65] += v
out(b) = 0.5 + 0.5 · clamp(−1, 1, Σ_k ae[(b−1)·4+k−1] · (in(b,k) − ctr(b,k)))
destination plink: offset −0.5, scale 2, baseline = base   ⇒   value = clamp01(base + clamp(±1, Σ_k depth_k·(src_k − neutral_k)))
```
Identical to a single direct link when there is one source; the ±1 clamp = one full parameter course.

### 5.3 Position and lifecycle (MM 31-52, LE 2555-2680, UI 7124-7155, 9146-9151)
- Inserted at **chain HEAD** (`AddByName(..., -1000)` = index 0) so it always receives the CP MOD CCs (a MIDI link reads
  only what enters its FX). Price: all indices shift → `snapshotLinks` before, `repairLinks` after; one block of latency
  (sources after it are read at the previous block).
- Never inserted/removed during the sweep or a gesture. The sweep sets `mix_missing` (a param/port wants a bus and there
  is none, or the instance is shorter than 240 params → `upgradeMix`: delete + reinsert at head, it holds no state of its
  own) / `mix_unused` (no bus claimed). The loop, mouse up and no Capture pass, calls `UI._retire.mix()` in an undo
  block ("FX Constellation: add/remove the modulation mix"), then forgets `fx_data` (re-scan). Failure → status
  "Extra sources stand by: <why>" and params keep their first source's direct link. Retire uses `retireFX` rules;
  a refusal is not re-asked on that track.
- Protected internal FX (CORE 393-428): never listed, ROLLed, reordered, randomly bypassed, deleted by presets.

### 5.4 Bus planning (`planMix`, LE 2446-2553)
`wantsBus(pd)` = selected, has first source, not fast, and (≥2 sources, or pad rides, or a depth modulation on one of its
links). Plus internal ports from `ModGraph.eachBusPort`. Claim order: (1) a key already linked to a bus output KEEPS it
(read back from its plink — "the link is the truth"); (2) params multi-source / depth-mod; (3) internal ports; (4) pad-only
riders last, silently falling back to the source's direct link (no `mix_full`). Out of buses → `mix_full` (status/tooltip).
Pad rides (LE 2175-2226): `pad_mode==0`, selected, not fast, has a first source, bridge exists, X/Y chip set, and fewer than
4 sources → input `n+1` = bridge `x_out/y_out/xy_mix`, depth = `modSpan(1)` (the range shared with the first source),
**sticky neutral** = `gesture_base` (written only when the input is freshly wired, or on rebuild/reanchor — otherwise the
first pad click would snap the param to its base).

---

## 6. Notes JSFX (`FX Constellation - Notes`) (J:NOTES, NS)
- Sliders (param idx): 0 `ns_note` (out NOTE), 1 `ns_vel` (VEL), 2 `ns_rand` (RAND, def 0.5), 3 `ns_oct` (OCT) — all
  0..1 hidden "(mod source)"; 4 `ns_lo` 0..127 def 24 (C1); 5 `ns_hi` def 96 (C7); 6..9 glide ms 0..2000 per source;
  10 `ns_knote` 0..127 def 60; 11 `ns_kgate` 0/1; 12 `ns_kvel` 1..127 def 100. `PARAM_COUNT = 13`.
- `@init ext_noinit=1`; targets start from the saved slider values (no jump on reload).
- On each note-on (`(m1&0xF0)==0x90 && vel>0`) via `midirecv`, and on the window keyboard (rising gate edge, or a
  different note while held): `note = clamp01((nt-lo)/(hi-lo))` (hi≤lo → hi=lo+1); `vel = vel/127`; `rand = rand(1)`
  (new per note); `oct = (round(nt) mod 12)/11` (C=0, B=1). **Values are HELD** until the next note (release does nothing).
- Glide per block: `ms<0.5 → target`, else one pole `k = 1 - exp(-N/(ms·0.001·srate))`.
- Every MIDI message is re-sent with `midisend` at its offset (it calls `midirecv`, so it must). No audio processing.
- Placement: right AFTER the Sequencer, else at head (`-1000 - at`), never behind an instrument (it would hear nothing).
  The window keyboard writes SG sliders, not MIDI → mirrored into `ns_knote/kgate/kvel` (gate written last).
- Lifecycle like the bus: `notes_missing` → `ensureNotes` (+ restore `P_EXT:CP_FXC_NOTES_PARK`), `notes_unused` →
  `retireNotes` (parks params 4..9 as `"%.6g,..."` before deleting). Native links only (no fast route). Polarity: RAND
  bipolar, NOTE/VEL/OCT unipolar. Rack tile menu: Range (C1-C7 default, C2-C6, C3-C5, C0-C8, Full 0-127), Glide
  (Off, 10, 30, 100, 300 ms, 1 s) (UI 7498-7533).

---

## 7. UI — what the user sees and does (UI/LP; ModKnob internals lost → (?))

### 7.1 Rack (right zone) (UI 7688-8016; LP 1008-1107, 1545-2323)
- Header: tabs `Track | Global | Links` (`s.lfo_panel_mode` 1/2/3; no "Matrix" tab — legacy `LP.drawMatrix` only as a
  fallback), toggle `Map`, `▶` collapse (also disarms Map). Collapsed = full-height rail "<  SOURCES". Footer caption
  "drag a dot onto a knob · alt+click a knob to unlink".
- Track & Global tabs: first a grid of track sources (ENV 1-4, SEQ A, SEQ B, NOTE, VEL, RAND, OCT), then a grid of the 8
  bank slots (local on Track, CP MOD on Global). Tiles: 1..4 columns (min 118 px), one row high, left color bar
  (alpha 1 on / 0.35 off), name by TYPE ("LFO 3", "S&H 2", "STEP 3"), dest count right ("1".."8","8+", hidden at 0),
  mini curve + live phase dot when on, 10 px drag dot at the left edge. All 8 slots always shown (off = dimmed);
  no "+ add source" in the rack (only in the standalone popup's card list). Off ENV (SG off) / absent SEQ tiles: grey,
  no dot, explanatory tooltip.
- Click tile = select (`lfo_sel_slot`, clears `rack_env_sel`); not if the click starts a drag. ENV tile → its ADSR editor
  (same `SGPanel.drawEnvEditor` as the Sound Gen tab); SEQ → switches to the Seq tab; NOTE tiles → settings menu.
  Hover a tile → highlights what it drives.
- Global tab without bank: hint "Global bank: hidden CP MOD track, modulates ANY track through 14-bit CC." + "Add source
  bank". Global tab extra row: "Map: touch target" (arms: link the next REAPER param you touch to this global slot,
  `ModJSFX.getTouchedParam` + `linkParamToGlobalSlot`: depth 0.5, baseline = current normalized) and "Link last".
- Selected slot editor: row 1 `● ON/○ OFF` · TYPE chip (LFO/S&H/STEP menu) · SHAPE chip (LFO only); row 2 segmented
  `Bipolar | Unipolar` (greyed without band; change → `rewriteSlotOffsets`) · `Retrig` · `Rnd`; row 3 Sync chip
  (menu up to `sync_max`, else "Slower divisions: reload the project") · Mode chip. S&H: combo "Sample" (Noise, <slot
  names by type>) + help text. STEP: 16-step editor (length lane on top: click/drag 1..16; bars paint 0..63 interpolating
  across crossed steps, Shift fine 0.25/px; playing step highlighted; steps past length alpha 0.22; "Grid" combo Off/2..12
  in ExtState `step_grid`; "Randomize" = only the steps that play, on the grid).
  Four knobs: RATE (free: log display "%.2f Hz"; synced: steps over divisions slowest→fastest, double-click = 1/4,
  **not modulatable**), PHASE "%.2f", CURVE "%+.2f" (named WIDTH on Square), SLEW "%.0f ms"/"Off". RATE/PHASE/CURVE/SLEW
  are drop targets and become ModKnobs when modulated (§7.3). Collapsed "Source → source" (legacy LFO→LFO combo).
  One-cycle preview with live dot (not for STEP; S&H drawn as a flat level = held value).

### 7.2 Drag to link (MD; UI 641-644, 3888-3901, 5359-5370, 6976-7041, 7336-7350, 8058-8074, 8273-8277)
- Dots (`ModDrag.Handle`): rack tiles, ENV/SEQ/NOTE tiles (when on), macro knobs (top-right), pad X/Y knobs in the
  info bar (`PAD_X=-1`, `PAD_Y=-2`, "Pad X"/"Pad Y"), Seq tab lanes MOD A/B. Hover = hand cursor + glow, and sets the
  hovered source for highlighting.
- Drag: dashed line from the dot to the cursor in the source color + source name label; redraw kept alive. **The first
  target containing the cursor consumes the drop** (order-independent). `Draw` (last in the frame) ends the gesture on
  mouse up. Cancel on track change / Esc.
- Targets & effect: param knob (knob only, accent circle on hover) → `followSource`: **ADDS** (≤4), selects ("tracks")
  the param, arms the source. A pad dot on a param (`dropOnParam`/`followPadAxes`, UI 2826-2866) RELEASES all its slot
  sources (`releaseSource`) and makes it pad-follow; it ADDS the axis to the existing X/Y chips only if the param was
  already pad-only (Y + X = XY). (Pad+sources together happens via the X/Y chips + bus "pad rides", §5.4.) Sound Gen
  knobs same. Slot fields → `linkInternal` depth 0.5. Macros (need bridge: "macros need the bridge (Auto JSFX)").
  Seq settings ("no sequencer on this track"). Pad X/Y pills: a drop REPLACES the axis's single source
  (`pad_src_x/y`), pad dots refused. LinkStrip line → modulate that link's depth (0.25); LinkStrip foot → add a source.
- Refusals → status bar (top-left, accent) for 3 s, reason string from the engine.

### 7.3 The modulated knob (ModKnob) (UI 2743-2792, 3296-3396, 3508-4042, 5155-5426)
- Single source: thick ring in source color over the excursion window (§3.2, `depth_center` = link neutral); grey when
  bypassed. Several: thick ring = SUM of active sources in accent color; each source also a thin arc (`opts.extra`),
  dimmed if bypassed, thicker when hovered elsewhere. **No "current source" for knob gestures anymore** (2026-10-10).
- Live dot = `getLiveValue` (base + terms, §3.5); frame-rate model for a single bank LFO (`live_lfo`, DLL-bound).
- Gestures: drag = base (Shift fine); **Alt+drag = depth** (signed; crossing zero flips invert; with several sources all
  depths scale proportionally); **Alt+click = release ALL sources**; **Shift+click = bypass all / restore all**;
  double-click = reset to scan value; right-click = source menu; mouse-down = `pokeTouch` (REAPER last-touched).
- Influence knobs (on hover, one per source, even with one; never in Map): depth / bypass / remove per source (exact
  mouse mapping inside lost widget `MKOV` (?)).
- Hover highlight: the source under the mouse (dot, tile, influence knob, carried dot) makes every dial it drives pulse
  in its color and tints its rows (0.26); without hover, the rack-selected slot tints its rows.
- Row source column: one dot per source (hollow = bypassed) or dot + label (L3, G2, M1, E1, S1, NT…); pad-only "XY/X/Y/–".
  Click it → opens the LinkStrip under the row. Tooltip: name, "lo -> hi" in plugin units, depth text per source,
  pad notes ("pad axis rides along with the source (sum bus)", "standing by - the fast route wins", "standing by - no room
  left on a sum bus"), "[INVERTED]", gesture help line.
- Depth text (`depthText`, UI 2414-2490): `:log` Hz param → "±%.2g oct"; linear with unit → "±18.2 st" style via
  `depthInUnits`; otherwise "±%.0f %%" (centered) / "%+.0f %%"; internal ports use percent only.

### 7.4 LinkStrip & Links tab (LS; UI 6419-6576, 6703-6901)
- One line per link (22 px): color dot (hollow if bypassed), name (≤⅓ width), bipolar fader of the signed depth
  (`pos = (d+1)/2`, wheel 0.01, double-click → 0), "x". Under each link, indented, the links of ITS depth (3 levels
  shown, 24 lines max). Foot "+  add a source".
- Name: click = select (current source), Shift+click = bypass, Alt+click = remove, right-click = menu: "Name ±x %",
  "Edit it on the knob (Alt+wheel)" (stale), "Invert", "Polarity ▸ Follow source (unipolar|bipolar) / Bipolar: swings
  around the base / Unipolar: only above the base" (not for pad), "Modulate this depth ▸", "Remove". Foot click = full
  source menu. Actions deferred until after the loop (ranks stay valid). Lives IN the list, never a popup (must receive drops).
- Links tab: "LINKS (n)" + Scan; blocks per destination (params in chain order "Param · FX", slot fields "CP LFO i ·
  RATE"/"Global LFO i · FIELD", "Sequencer · label", macros), each with an "x" (remove all) + LinkStrip; bottom "OTHER
  TRACKS (global bank)": one row per plink of the global bank on other tracks (fader, "x" = unlink).

### 7.5 Map mode à la Bitwig (UI 3231-3253, 6579-6684, 7699-7715, 9185-9235)
Toggle "Map" in the rack header; active only with the rack expanded on Track/Global. Mapped source = `UI._focus_slot` = the rack's
selected slot (local `i`, global `100+i` on the Global tab, or `201+k-1` when ENV tile k is selected; UI 9185-9193). On every dial (params, SG, slot fields, macros, seq): the thick ring shows
the mapped source (empty ring if not linked yet), others thin; **dragging sets the mapped source's depth** (first pixel
ADDS it; < 0.005 adds nothing), Alt+click removes it, double-click zeroes it; base stays on the wheel; no influence knobs;
not applied to the source's own settings. Exit: Map again, Esc (first Esc closes an open LinkStrip), folding the rack.
Not persisted (a mode that changes what drag means must not survive a close).

### 7.6 Right-click source menu on a param (UI 2965-3211)
Title; per source "Name ±18.2 st" (click toggles bypass); "Remove source ▸" (+ "All sources"); "Modulate a depth ▸ Depth
of <src> ▸"; "Follow Pad X / Pad Y / Pad XY"; "CP LFO ▸ 1-8", "Global LFO ▸ 1-8", "Macro ▸", "Note Envelope ▸ ENV 1 (amp),
ENV 2-4", "Sequencer ▸ Mod A/B", "Note ▸ 4" (checked = present; click toggles); "Param LFO (native) ▸ Enable, Shape (Sine,
Square, Saw L>R, Saw R>L, Triangle, Random), Speed 0.1..8 Hz, Tempo sync 1/16..2 bars, Strength 10/25/50/100 %"; "Show the
source rack"; capture arm entry. Polarity is NOT here (LinkStrip only).

### 7.7 Status-bar & pad
"N tracked · N links · N modulated"; buttons "Release all links", "Stop all engines" ("N engine(s) stopped"). Pad pills
"X free" / "X L3"; click a lit pill releases it; drop target "Drop a source here to drive pad X". A driven pad axis refuses
the mouse ("Pad X and Y follow modulators: click the X / Y pills below to free them").

---

## 8. Value-space contract (RM §14, §21.2) and pitfalls (memory + CL)

1. **REAPER's normalized (`norm`) is the only currency between modules** (`TrackFX_Get/SetParamNormalized`). Lua never
   computes a normalized from a raw value. `plink.scale`, `mod.baseline`, bus amt/ctr, fast-route amt are all `norm`.
2. Display curves are applied by the widget only (`to01`/`from01`, both directions); names carry their space
   (`base_norm`, `live_pos`); a bare name = norm. Since 2026-10-07 our curved sliders DECLARE their shape (`:log`/`:sqr`)
   so `norm == pos` and modulation happens in the curve (octaves for RATE/cutoff).
3. ⚠ On a shaped slider `TrackFX_GetParam/SetParam` speak the shape domain (ln v, sign·√|v|), unbounded; a raw value must
   go through the normalized path. A Lua round-trip can look right while REAPER hears e^v. `GetParamEx` min/max of a shaped
   slider is not the slider course. (T fake_params.lua = measured model.)
4. `TrackFX_SetParam` on a JSFX = RAW slider units; `SetParamNormalized` = 0..1 (Walk/Figures broke on this; eng_mode 0..2).
   VST Cockos params are 0..1 in both.
5. **A plink is 1-for-1 on the destination**: one source per param; sums only through the bus; native REAPER LFO+ACS+link
   addition was rejected (RM 211-224).
6. **Everything natively modulable must be a JSFX slider** (an extension/DLL is not in the FX chain; RM 185-202).
7. **EEL2 has no scientific notation** (`1e-6` = `1` then name `e`): a JSFX that does not compile does NOTHING silently.
   EEL2 ignores case (`FK` == `fk`). No `]]` inside a Lua long string carrying JSFX. Unknown EEL variable = 0 silently.
8. **Slider-count change = the only recompile trigger**; layout bands are append-only, one `_COUNT` per band; recreation of
   an instance loses its envelopes unless carried. Generated JSFX are rewritten at each open (code reaches next load).
9. **Never restructure a chain during a gesture or inside the sweep**; always snapshot/repair links by GUID around it.
10. **The link is the truth at scan time** (adoption, bus claims), but the sweep is topological (never rewrites an intact
    link's scale/baseline). Per-link neutral: any site compiling/drawing a LINK asks `linkCenter`/`ModGraph.center`,
    never `sourceCenter` (that is for the SOURCE: rack, legacy conversions).
11. Release returns to the BASE; the base never drifts (re-anchoring only by a positive marker `native=1`).
12. `GetParam` on a modulated param returns the BASELINE: live values are recomputed from the source (and never recorded
    from a destination — record the SOURCE, RM 1820-1869).
13. Lua `a and f() or g` truncates multiple returns / mis-handles false/nil (bandOf `n` lost → wrong colors) — irrelevant in
    Rust but explains several guards.
14. gmem: a bank that stops publishing keeps its last values — use the block counter.

---

## 9. Persistence summary
- Per track, FXC selection table (`selections.dat` + RAM ExtState `CP_FXConstellation/track_selections`, keyed by track
  GUID; entry key `<fx full_name>||<param name>`): `mod_sources[k] = slot`; `mod_more[k] = "slot:%.4f;..."` (≤3, depth
  relative to gesture_range, ±1); `mod_pol[k] = "slot:u;slot:b"`; `mod_off[k] = "slot;slot"` (sorted); ranges, invert,
  base_values, xy_assign, `native=1`, `norm=2`, macro_names, pad (incl. `pad_src_x/y`, zone) (FXM 826-962, CORE 430-512).
- Track P_EXT: `FXC_ROUTES` (§4.2), `FXC_FASTCH` (§1.6), `CP_FXC_NOTES_PARK`, (`CP_FXC_SEQ_PARK`, `CP_FXC_detached` — not mine).
- JSFX sliders (banks, bus, notes, bridge macros/pad) saved by REAPER; plinks saved in the project.
- Presets: per param `mod_source`, `mod_more`, `mod_pol`, `mod_off`, `dmods` (depthSnapshot); `lfo.internal` =
  `captureInternalLinks` (keys per internal target e.g. `m<n>`, `q<p>`, slot fields (?) → `{src, scale, baseline, routes
  [{src,depth,pol,off}], dmods}`, `_seq`, `_shaped`); global-bank internal links are NOT in presets (shared by all tracks);
  Init removes local/macro internal links (PresetSystem 172-270).

---

## 10. Code vs roadmap disagreements, stale text, open items
1. RM §5.4 table lists only 0/1..8/101..108/201/211..218; code has 201..204, 221..222, 231..234, -1/-2.
2. RM §5.3 "click a dot = the source Alt+wheel edits, Alt+click removes the current source" and "internal destinations keep a
   single link (drop replaces)": superseded — code: no current source for knob gestures, Alt+click releases ALL, Shift+click
   bypass; internal ports have 4 sources (ModGraph), except global-bank fields (1). Stale UI strings remain ("Alt+click:
   remove the current source", "Right-click: pick the source Alt+wheel edits", LinkStrip "Edit it on the knob (Alt+wheel)").
3. RM §3.4 "X and Y only receive; the pad is not a source": reversed by RM §18.4 and code (pad dots -1/-2).
4. RM §13.7/13.8 (k3) Track/Global/Matrix + "only used slots and a +": code = Track/Global/**Links**, all 8 tiles, no +.
5. RM §5.1 native REAPER LFO "rejected": code still offers "Param LFO (native)" in the param menu.
6. RM §14.5 checklist all unchecked, but release-to-base and normalized-everywhere are implemented (§3.7, §21.2 phase 1).
7. RM §21.7 phase 5 "fifteen destinations": code has 19 fast codes (balances 16-18, FM 19).
8. LE header (l.9-12) "plink.offset = -0.5 (pad center)": code anchors pad links at `-gesture_base`.
9. Memory (2026-10-04) says bank PARAM_COUNT 144; installed bank has 147 (shp, fch, fmask). Nov `ModJSFX.lua` is lost: the
   exact Nov names of `isShaped/shapeDone/markShaped/hasFast/setFast/readMotion/writeField/repairBank` thresholds are (?).
10. Matrix/`getParamLink`/`reconcileModSources` don't know ENV/macro/seq/note links (RM 1781-1792).
11. Open with Cédric: ENV 2-4 per voice (global today; recommended per-link "per voice" option, off by default).
12. FOLLOW never built; ENV type in bank never built. Map-mode depth drag at Lua rate only.
13. Single global source linked directly on an FX behind a MIDI-blocking instrument: no CC arrives (no special case) (?).

## 11. Surviving test oracles (S/Tools)
`modjsfx_test.py` (bank layout bands), `fxc_multimod_test.py` (4 sources/bus), `fxc_graph_test.py` (ModGraph, depth ports,
pad on depth, polarity), `fxc_internal_link_test.py`, `fxc_release_test.py`, `fxc_retire_test.py` (snapshot/repair both
worlds), `fxc_shape_test.py` + `fake_params.lua` (shapes, convertLink), `fastmod_test.py` (fast routes: same sound at 64
and 512 blocks), `fxc_notes_test.py`, `fxc_frame_test.py` (LinkStrip, Links tab, Map), `modknob_test.py/.lua`,
`livedot_test.py/.lua`, `fxc_space_test.py`, `jsfx_real_test.py` (real EEL2), `fxc_preset_test.py`. They encode the exact
expected numbers and are the best executable reference for the Rust port.
