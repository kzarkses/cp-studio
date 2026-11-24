# FX Constellation — SPEC part: STATE, PRESETS, PERSISTENCE, CEDRIC'S DECISIONS

Reconstructed 2026-11-25 from what survived. Read-only research: nothing in the sources was changed.

## 0. Sources and reference shorthand

| tag | path | state of |
|---|---|---|
| `S/<f>` | `<recovery>\survivors_CP_Scripts_20261121\FX Constellation\Modules\<f>` | **latest** (PresetSystem.lua and Persistence.lua 2026-10-25 17:24, UI.lua 10-25 19:41, GestureSystem.lua and LinkEngine.lua 10-11 / 10-25) |
| `H/Core.lua` | `<recovery>\claude_file_history\0c5c4909-…\a9c6ba36624bb656@v2` | latest, 2026-10-25 17:24 |
| `H/FXManager.lua` | `…\0c5c4909-…\3d2fc94ff4d62d7c@v2` | latest, 2026-10-25 17:24 |
| `H/FXDatabase.lua` | `…\0c5c4909-…\74a231ded94e0f0c@v2` | latest |
| `CLAUDE` | `<recovery>\winfr_utile\fichiers\CLAUDE.md` (1805 lines, superset of the H copy) | end of October |
| `RM` | `…\survivors_CP_Scripts_20261121\ROADMAP_FXConstellation.md` | 2026-08-05 → 10-25 |
| `RA` | `…\survivors_CP_Scripts_20261121\ROADMAP_Autonomie.md` (dated journal) | → 2026-10-25 |
| `M:<path>` | the bare git mirror `github_2026-08-22.git`, HEAD | **2026-08-22** (old) |

- The `H` copies of PresetSystem.lua and Persistence.lua are **byte-identical** to `S`.
- The undeleted `Core.lua`/`FXManager.lua` under `undelete\Recovery_20261121_132718\…` are 0-line, zero-filled shells: useless.
- The main script `CP_FXConstellation.lua` survives only in M (August, 160 lines). Its November version is lost.
- Line numbers are those of the files above. "~" means approximate.

---

## 1. Every piece of persisted state

### 1.1 Files: `<script>/FX Constellation/Data/` (all lost, see §3.4)

`data_path = script_path .. "Data/"`, `presets_file = data_path .. "presets.dat"` (M:CP_FXConstellation.lua:8-9). Three of the four files are Lua table literals written by `Persistence.serialize`. `fx_database.dat` uses its own `return {…}` writer.

**Serializer** (`S/Persistence.lua:28-54`):
- strings use `%q`; numbers use `%.17g`; booleans are written as such;
- tables are written as `{[k]=v,[k]=v}`, with pairs order (not stable), no spaces;
- functions and other values are written as `nil`.

**Reading** uses `load("return "..s)`:
- `tryDeserialize` returns `(nil, "empty"|"syntax"|"eval"|"not-a-table")` (`S/Persistence.lua:64-72`);
- `deserialize` returns `{}` on any failure (74-78).

A Rust reader needs a Lua-literal parser that handles:
- `%q` escapes: `\"`, `\\`, `\` + real newline, `\r`, `\0`, `\ddd`;
- integer and float keys (`[0]=` for fx_chain slots);
- nested tables.

| file | written by | content / format | ref |
|---|---|---|---|
| `presets.dat` | FXC only | `{ [preset name] = preset }` (model in §2.2). No memo. Biggest file (1.1 MB in August, 1.6 MB before the 10-10 sparsification) | `S/Persistence.lua:142-153`, `181-208` |
| `selections.dat` | FXC **and the FX Browser** (two writers, so never memoised) | `{ [track GUID] = track_data }` (structure in §1.7). 1.98 MB in August | `S/Persistence.lua:12`, `275-306`, `522-531` |
| `settings.dat` | FXC only (memoised) | `{["state"] = { <PREF_KEYS only> }}` (§1.8) | `S/Persistence.lua:8`, `486-510` |
| `fx_database.dat` | FXDatabase | `return { favorites = {[plugin id]=true}, plugins = {…}, categories = … }` (pretty-printed). Example favourite keys: `"CP FX Constellation/CP_FC_Phaser.jsfx"`, `"reaeq.dll"`, `"kHs_Filter.vst3"`, `"kHs Filter Table (Kilohearts)"` | `H/FXDatabase.lua:8-11`, `274-310`; M:Data/fx_database.dat |
| `snapshots.dat`, `granular_sets.dat` | **legacy** (no reader in the latest code) | snapshots keyed `"<trackGUID>_<fx name>:<nparams>;<fx name>:<nparams>;…"` → `{Snapshot1 = {gesture_*, param_data}}`. Grain sets keyed by name. Superseded by `presets[name].snapshots` / `.granular_sets` | M:Data/*.dat; grep of S/H finds no reader |

**Write protocol**: `writeFile` (`S/Persistence.lua:118-140`) is atomic with one backup generation:
1. write `path.tmp`;
2. check BOTH `write` and `close`;
3. remove `path.bak`;
4. rename `path` → `.bak`;
5. rename `.tmp` → `path`;
6. if that rename fails, rename `.bak` back.

An optional memo skips a payload identical to the last one this process wrote (single-writer files only).

**Preset read: three states, not two** (`S/Persistence.lua:155-208`):
- **absent**: first run, saving allowed;
- **unreadable**: try `presets.dat.bak`. If it reads, load it and show a message box. If it does not, set `presets = {}` and do NOT set `_presets_loaded`, so `savePresetsToFile` refuses to write;
- a legitimately empty table may be written (deleting the last preset must stick). The only guard is "file never loaded" (146).

**Cadence** (`S/Persistence.lua:348-392`):
- `scheduleSave`: settings flag, trailing debounce 2 s;
- `scheduleTrackSave`: 0.75 s, pushes the timer forward and never pulls it back;
- `schedulePresetSave`: flag only;
- `checkSave`: runs `saveSettings` when the timer has passed and any flag is set;
- `flushAll` on window close forces settings and selections. The close handler first calls `saveTrackSelection` + `flushTrackSelection` (M:CP_FXConstellation.lua:131-146).

**Pruning**: `MAX_TRACK_SELECTIONS = 512`, MRU by track GUID, never pruned on load (`S/Persistence.lua:394-443`). It was 32, and that cap lost data at 51 tracks ("the LFO isn't mapped like it was").

### 1.2 Global ExtState: section `CP_FXConstellation` unless stated

| key | persist | format | meaning | ref |
|---|---|---|---|---|
| `state` | **false** (RAM) | `serialize(PREF table)` | Cross-process copy of settings. Preferred over `settings.dat` when non-empty (fresher). Lost at REAPER restart | `S/Persistence.lua:245-252`, `519` |
| `track_selections` | **false** | `serialize(track_selections)` | Cross-process channel with the FX Browser. Re-read by `loadTrackSelection` only when no local save is pending | `S/Persistence.lua:275-305`, `527`; `H/FXManager.lua:1137-1147` |
| `filter_keywords` | true | comma list | Parameter-name filter words. Default `MIDI,CC,midi,Program,Bank,Channel,Wet,Dry` | `S/Persistence.lua:213-221`, `542-543` |
| `param_filter` | true | string | Parameter search text | `S/Persistence.lua:223-226`, `544` |
| `step_grid` | true | int 1..`ST_GRID_MAX` (1 = Off) | LFO STEPS snap grid ("Off", "2"…"12") | `S/LFOPanel.lua:1240-1262` |
| `goto` | false | `"<time_precise>|<track GUID>"` | Written by a CP_Session strip chip: "show this track". Read once, deleted, refused after 5 s. Temporarily lifts the lock | `S/UI.lua:8909-8930`, `8949-8961` |
| `cmd` | true | `"_RS…"` | Registered action id of the script (for the view selector) | M:CP_FXConstellation.lua:126-131 |
| `alive` | false | `time_precise` heartbeat every 0.5 s; deleted on close | Lets other windows know it runs (August main script; November version unknown (?)) | M:CP_FXConstellation.lua:146-161 |
| `win_state` | true | `"0,541,80,1143,756"` (dock, x, y, w, h (?)) | Window geometry, written by the toolkit | live `reaper-extstate.ini` |
| `license_key` | true | key | License (ignore) | `S/License.lua:12-16` |
| section **`CP_Mod`**, keys `unlink`, `edit` | false | `"<trackGUID>|<fx>|<parm>|…"` | Hints from the standalone CP_ModLFO panel: a link it removed or edited on a parameter FXC manages. FXC drops its `mod_source`, or pulls base/range back. Seeded as already seen at init | `S/LinkEngine.lua:284-285`, `1931-1975` |

The live `<home>\AppData\Roaming\REAPER\reaper-extstate.ini:1693-1699` holds today, under `[CP_FXConstellation]`: `filter_keywords=MIDI,CC,midi,Program,Bank,Channel,Dry,delta,bypass,mix,LFO,SLOT,Mute,Input,Output,InOut,wet,param`, `param_filter=`, `license_key=…`, `win_state=0,541,80,1143,756`, `cmd=_RSd8e3d73a…`, `step_grid=3`. These are **his real preferences: reusable**.

### 1.3 Project ExtState (saved in the .RPP `<EXTSTATE>` block, keys upper-cased there)

| key | format | meaning | ref |
|---|---|---|---|
| `CP_FXConstellation` / `pad_src:<track GUID>` | `"<x>,<y>"` (slot codes, §1.9, 0 = none) | Which modulation source drives pad X / pad Y, **per track**, since 2026-10-10 evening. Written only when it changes. Arriving on a track with `0,0` and no key writes nothing (a write would mark the project dirty) | `S/GestureSystem.lua:1002-1075` |
| `CP_FXConstellation` / `pad_src` | same | **Legacy**, one per project. Read when the track has no key of its own. Seen in his projects as `<CP_FXCONSTELLATION PAD_SRC 0,0>` | `S/GestureSystem.lua:1036-1038` |
| (read only) `CP_Sampler` / `ACTIVE_KIT` | | The Sound Gen's "From Instrument" looks up the kit | `S/SoundGenerator.lua:4235-4236` |

### 1.4 Track P_EXT (saved with the project, inside `<TRACK … <EXT … >>`)

| key | format | meaning | ref |
|---|---|---|---|
| `P_EXT:CP_FXC_detached` | FX GUIDs separated by spaces, sorted. In the RPP: `CP_FXC_detached "{A…} {B…}"` | **Pool**: FX excluded from the ROLL (never rolled: params, bypass, position). Only GUIDs still on the track are kept. Shared with the FX Browser (`FXManager.POOL_EXT`). Re-read at signature cadence (0.25 s) to catch external changes | `H/FXManager.lua:126-232`, `490-519` |
| `P_EXT:FXC_ROUTES` | `v1;<portkey>=<src>@<depth>[@u|@b][@x],…;…` (§1.9) | Link model of **internal ports** (bank-slot settings rate/phase/curve/slew, macros, sequencer settings, and depth ports `r:…`), stored on the track that holds the destination. Imported once from existing plinks if absent, then written even when empty (`v1`) | `S/ModGraph.lua:30-41`, `47`, `263-370` |
| `P_EXT:FXC_FASTCH` | `"<ch 1..255>|<track GUID>"` | Fast-route gmem channel (`CP_FXC_FAST`) of the track. A GUID mismatch means a duplicated track, which gets a new channel. Collisions are resolved project-wide | `S/LinkEngine.lua:2314-2358` |
| `P_EXT:CP_FXC_SG_PARK` | a serialized Lua literal of `state.sound_generator`, minus `enabled`, `jsfx_index`, `ui_osc_tab` | Patch of a **removed** Sound Gen (turning it off removes it from the chain). Restored when it is added again. A track without a generator shows its parked patch, or the birth patch | `S/SoundGenerator.lua:30-105`, `128-132` |
| `P_EXT:CP_FXC_SG_OSC1_PITCH` | `"hz"` or `"note"`; absent = NOTE | OSC 1 NOTE/HZ mode. It is not a slider. Written only on change | `S/SoundGenerator.lua:4855-4925` |
| `P_EXT:CP_FXC_SEQ_PARK` | `"<p>=<raw %.17g>;<p>=…"` | Sequencer settings stored by an explicit **Remove**. ADD SEQUENCER restores them | `S/UI.lua:7086-7103`, `7220-7275` |
| `P_EXT:CP_FXC_SEQ_PARK_PAT` | `Sequencer.encodePat`: `"cur;mask;k=<v,v,…>;k=…"` | The 16 sequencer patterns at Remove | `S/Sequencer.lua:1915-1937` |
| `P_EXT:CP_FXC_NOTES_PARK` | values `%.6g` joined by `,`, from `NOTES.P_LO` to `GLIDE_BASE+COUNT-1` | Settings of a removed `FX Constellation - Notes` instance (NOTE range, glides). Cleared when restored | `S/LinkEngine.lua:2578-2640` |
| (read only) `P_EXT:CP_KIT_PADS`, `P_EXT:CP_KIT_INSTR` | CP_Sampler data | "From Instrument" sample source | `S/SoundGenerator.lua:4194-4240` |

### 1.5 State that lives on the FX instances (saved in the .RPP by REAPER itself)

This is what really survived in his projects, and the Rust rebuild must read it.

**Bridge `FX Constellation Bridge.jsfx`** (`S/GestureSystem.lua:458-747`). Found by name substring `"FX Constellation Bridge"`. Sliders are **append-only**: an index never changes meaning. Slider values in the RPP are RAW.

| 0-based index | slider | meaning |
|---|---|---|
| 0, 1 | `x_pos`, `y_pos` | pad inputs: mouse, envelopes, or the plink of a source-driven axis |
| 2 | `slew` | 0..2 s |
| 3, 4, 5 | `x_out`, `y_out`, `xy_mix` | link sources (outputs) |
| 6..13 | `macro1..8` | **macro values live ONLY here**: saved with the project, automatable |
| 14..21 | `macroN_out` | macro outputs, smoothed with the same slew |
| 22 | `eng_mode` | 0 off, 1 walk, 2 figures |
| 23 | `eng_mask` | 1 = X, 2 = Y, 3 = both: the axes the engine owns |
| 24..27 | `walk_rate` (Hz, 0..20), `walk_jit`, `walk_jump`, `walk_sync` (0..5) | walk settings |
| 28..30 | `fig_mode` (0..5), `fig_rate`, `fig_size` | figure settings |
| 31..34 | `zone_x0`, `zone_x1`, `zone_y0`, `zone_y1` | pad zone |

- Versions by `GetNumParams`: v1 = 2, v2 = 6, v3 = 22 (macros), v4 = 31 (engine in `@block`), v5 = 35 (zone). Constants: `BRIDGE_PARAM_COUNT=35`, `MACRO_BAND_COUNT=22`, `ENGINE_BAND_COUNT=31`, `ZONE_BAND_COUNT=35` (`524-531`).
- **Raw values**: `TrackFX_SetParam`/`GetParam`, never normalised. A divide-by-range bug broke Walk and Figures (`S/GestureSystem.lua:949-982`).
- `adoptEngine` (`1097-1165`) reads the instance **before** talking to it: mode, speeds, zone, position from `x_out`/`y_out`, then `readPadSources()`. The project is the authority over its pad.
- Upgrading (`createAutomationJSFX`, `767-924`) keeps every slider value, every envelope chunk and the chain position (`-1000-pos` insertion).

**Native parameter modulation (REAPER `<PROGRAMENV>` / `PLINK`)** on target FX. This is how the mapping is actually heard (`S/LinkEngine.lua:1-46`, `288-300`):
- target param N carries `param.N.mod.active` / `.baseline` (= the anchor) and `param.N.plink.active` / `.effect` / `.param` / `.scale` / `.offset`;
- for a pad link, `plink.param` is 3, 4 or 5 on the bridge, `scale = range × gesture_range` (negative = inverted), `offset = -0.5`;
- a global-bank link uses the MIDI plink (`effect=-100`, `midi_msg=176`, `midi_msg2=128+cc`; RM §12.1);
- a link through a Mod Mix bus has scale 2 and offset -0.5;
- also `param.N.lfo.*` and `param.N.acs.*` are read only to detect foreign modulation;
- RPP example (darkmatter.rpp): `PLINK 0.412188 1:-2 3 -0.170543` (scale, FX offset relative to the target, source param 3 = `x_out`, offset).

**Other internal FX** (identified by name, never listed, rolled or deleted by a preset; `H/Core.lua:393-419` `PROTECTED_IDENT`, `S/PresetSystem.lua:981-999`):

| FX | param count | where its state lives |
|---|---|---|
| `FX Constellation - Sound Generator` | `PARAM_COUNT 147` (`S/SoundGenerator.lua:3929`) | sliders + `<JS_SER>` @serialize (sample path) |
| `FX Constellation - Sequencer` | 230 (`S/Sequencer.lua:98`) | its 16 patterns in @serialize / gmem `PAT_GBASE` (2^21) |
| `FX Constellation - Mod Mix` | 240 sliders | sum bus |
| `FX Constellation - Notes` | 13 | NOTE / VEL / RAND / OCT sources |
| `CP_Mod LFO` (local bank, on the track) | 147 sliders, `options:gmem=CP_FXC_FAST` | — |
| `CP_Mod MIDI` (global bank) | 147 | lives on the hidden track **named `CP MOD`**, `MAINSEND 0`; reaches other tracks as 14-bit CC through MIDI sends (`AUXRECV`) |

Other protected identities: `reasamplomatic`, `cp_kit_choke`, `cp_kitsampler`, `cp kit sampler`.

### 1.6 In-memory key formats (they become the on-disk keys)

- **Param key** (memory): `<trackGUID> .. "_" .. fx.full_name .. "||" .. param.name`, with suffixes `_range`, `_invert`, `_x`, `_y` (`H/Core.lua:336-358`). FX key: `<trackGUID>_<full_name>` (`360-371`).
- **Disk keys inside a track entry, a preset or a snapshot** drop the GUID: `"<fx full_name>||<param name>"`. `full_name` is REAPER's display name, e.g. `VST3: kHs Filter (Kilohearts)`.
- **ModGraph port keys**:
  - `p:<param key>` (parameter);
  - `f:<lfo|global>:<slot>:<rate|phase|curve|slew>` (bank-slot setting);
  - `m:<n>` (macro);
  - `q:<p>` (sequencer setting);
  - `r:<parent port key>|<src>` (depth port; nestable, e.g. `r:r:P|1|211`; the source is read at the END, `S/ModGraph.lua:66-160`).
- **Snapshot / preset `dmods`** = `ModGraph.depthSnapshot(parent)`: `{ ["|211"] = {{src,depth,pol,off},…}, ["|211|3"] = … }`. The chain of sources is relative to the parent port, so it is portable across tracks (`S/ModGraph.lua:1142-1210`).

### 1.7 `track_selections[guid]`: one entry per track GUID (`H/FXManager.lua:846-972`)

Rebuilt WHOLE on each flush (`flushTrackSelection`). Sparse: only non-defaults are written (measured: 85 % of a 1.4 MB blob was defaults).

```lua
track_selections["{TRACK-GUID}"] = {
  selection     = { ["<fx>||<param>"] = true },          -- tracked by the pad
  ranges        = { [k] = r },                          -- only r ~= 1.0
  xy_assign     = { [k] = { x = bool, y = bool } },     -- only when not (x and y)
  invert_assign = { [k] = true },
  fx_random_max = { ["<fx full_name>"] = n },           -- default 3
  base_values   = { [k] = anchor },  -- selected params only; the ANCHOR (param_base_values), never the applied value
  gesture_base_x, gesture_base_y, gesture_x, gesture_y,
  current_preset = "<preset name>" | "",                -- the current preset is a TRACK fact
  native = 1,        -- positive marker: written by the native-link version, so no re-anchoring (must be rewritten at every flush)
  norm = 2,          -- normalised values are REAPER's (FXManager.NORM_V)
  sg_shaped = 1|nil, -- Sound Gen values are in the slider-shape space
  mod_sources = { [k] = slot },                  -- first source (§1.9)
  mod_more    = { [k] = "211:0.5000;103:-0.2500" },  -- up to 3 extra sources, signed depth
  mod_pol     = { [k] = "211:u;103:b" },
  mod_off     = { [k] = "211;103" },             -- bypassed links
  macro_names = { [n] = "name" },                -- names only; values are on the bridge
  pad = GestureSystem.capturePad(),              -- §1.8
  morph_a = { ["<fx full_name>"] = { enabled = bool, params = { ["<param name>"] = norm } } },
  morph_b = same,
  -- legacy (August): linked = false (ignored, never re-read)
}
```

**Load: `loadTrackSelection`** (`H/FXManager.lua:1128-1375`):
- **Pad position, pad settings, morph and `current_snapshot` are restored ONLY on a track change** (`new_track`, 1161-1176). A plain chain re-scan keeps the live pad (« si j'ouvre le Sound Gen, il est deplace sans aucune raison »).
- **A never-set track arrives virgin** (1177-1220):
  - pad at the centre, `current_loaded_preset = ""`;
  - **every param deselected**: otherwise just looking at a track would write permanent plinks into the project.
- Migration markers, each positive and rewritten in the same gesture:
  - `norm ~= 2` → `convertEntry` (linear → REAPER normalised for `:log`/`:sqr` sliders);
  - `sg_shaped`;
  - `native ~= 1` → `links_reanchor` once (1241-1322);
  - `SoundGenerator.migratePitchKeys`: Hz keys → semitone `pm` keys (1221-1235).
- Missing keys = defaults: range 1.0, invert false, X and Y true, base = live value. `param_base_values[key]` is seeded at the same time (1324-1370).

**Arriving on a track** (`arriveOnTrack`, `H/FXManager.lua:1095-1126`):
- `applyPad(track_data.pad or Persistence.legacy_pad)`;
- morph A/B from the track;
- `morph_amount = 0`, `current_snapshot = nil`;
- `SoundGenerator.trackArrived()`, which restores the parked patch when the track has no generator.

**Ordering rule**: flush under the OUTGOING track before switching `state.track` (`S/UI.lua:8957-8964`). The same rule closes the keyboard gate and any Capture arming.

**Locking**: the first track is auto-locked. The lock is valid only in the ACTIVE project (`ValidatePtr2(0, …)`); changing project tab drops it (`S/UI.lua:9039-9076`). A new project re-runs the shape migration pass and `ModGraph.forget()` (`9078-9095`).

### 1.8 Preferences vs patch: the three lists

**`Persistence.PREF_KEYS`** (`S/Persistence.lua:459-469`) are machine-wide preferences, the ONLY keys `settings.dat` writes and reads:

```
randomize_intensity randomize_min randomize_max x_curve random_min random_max exclusive_xy
random_bypass_percentage random_lfo_probability lfo_random_settings layout_mode range_min range_max
auto_jsfx random_fx_count random_fx_favorites_only fxmanager_auto_open
zone_left_w zone_right_w zone_right_collapsed center_tab fx_show_all show_param_values
ultra_random_settings fx_view_size
```

- `fx_collapsed` was dropped: indexed by rank, it folded the 3rd FX of any chain.
- `jsfx_automation_enabled` was dropped: it is a fact, not a preference.
- After the merge, defaults are back-filled for missing sub-keys (`315-343`):
  - `ultra_random_settings = {params=false, bases=true, ranges=true, xy_assignments=true, invert=true, bypass=true, fx_order=true, mod_sources=false, sound_frequency=false}`;
  - `lfo_random_settings = {slots=8, shape=true, rate=true, sync=false, phase=false, pad_meta=false}`;
  - `show_param_values = true`, `auto_jsfx = true` when nil. A display setting reads `~= false`.

**`GestureSystem.PAD_FIELDS`** (`S/GestureSystem.lua:1296-1303`) is the pad as a **track/patch setting**, and the single list that capture, restore, defaults and change detection all read:

```
navigation_mode random_walk_speed random_walk_jitter random_walk_jump random_walk_sync
figures_mode figures_speed figures_size pad_zone_x0 pad_zone_x1 pad_zone_y0 pad_zone_y1
gesture_range gesture_min gesture_max link_slew smooth_speed max_gesture_speed
pad_src_x pad_src_y pad_mode granular_grid_size
```

- `capturePad()` (`1323-1333`) adds:
  - `nav_on` (Walk/Figures running);
  - `lfo_meta` (pad → LFO-rate meta links `{slot, axis "x"|"y"|"xy", bank, invert, base_rate}`);
  - `grains`, only when `pad_mode == 1` (granular).
- `applyPad(t)` (`1356-1379`): any missing field takes its DEFAULT (`snapshotPadDefaults`, captured from Core at init; zone full), never the previous value. `nil` means all defaults.
- `padChanged()` (`1387-1407`) is polled each frame (22 compares) so slider-driven pad edits get stored.
- Core defaults (`H/Core.lua:8-327`):

| field | default |
|---|---|
| `navigation_mode` | 0 (Manual; modes Manual / Random Walk / Figures) |
| `random_walk_speed` | 2.0 |
| `random_walk_jitter` | 0.2 |
| `random_walk_jump` | false |
| `random_walk_sync` | 0 |
| `figures_mode` | 0 (Circle, Square, Triangle, Diamond, Z, Infinity) |
| `figures_speed` | 1.0 |
| `figures_size` | 0.5 |
| `gesture_range` | 1.0 |
| `gesture_min` / `gesture_max` | 0 / 1 |
| `link_slew` | 0 |
| `smooth_speed` | 0 |
| `max_gesture_speed` | 2.0 |
| `pad_src_x` / `pad_src_y` | 0 |
| `pad_mode` | 0 |
| `granular_grid_size` | 3 |

**`Persistence.PAD_SET`** (`S/Persistence.lua:477-484`) lists the pad keys an OLD `settings.dat` carried. They are read once into `Persistence.legacy_pad` and given to a track whose entry has no `pad` (`H/FXManager.lua:1107-1118`).

Not persisted anywhere, on purpose:
- `random_walk_active`, `figures_active` (session facts; the bridge `eng_mode` is the truth);
- `pad_link_x/y` (published by `syncPadAxes`);
- `current_snapshot` (« où en suis-je dans cette session »);
- `param_mod_cur`;
- `links_active`;
- `jsfx_automation_index/enabled`;
- `preset_modified`, `initial_fx_chain_state`, `baseline_guid`;
- Map mode (« Pas rangé », RM §21.7).

### 1.9 Encodings

**Mod-source slot codes** (`S/LinkEngine.lua:87-215`; the ONLY decoder is `bandOf`, which tests from the highest band down):

| code | source |
|---|---|
| 0 / absent | pad XY |
| 1..8 | local CP_Mod LFO slot |
| 101..108 | global CP_Mod MIDI slot (CP MOD track) |
| 201..204 | Sound Gen ENV 1..4. **201 is never renumbered** (it is on disk). Outputs `{31,63,68,73}` (`SG_ENV_OUT`) |
| 211..218 | bridge macros 1..8 |
| 221..222 | sequencer MOD A / MOD B (p16 / p17) |
| 231..234 | NOTE, VEL, RAND, OCT |
| -1, -2 | pad axes as a source, only inside `FXC_ROUTES` (`S/ModGraph.lua:263-266`) |

**Extra-source strings** (`H/Core.lua:421-513`):
- `mod_more`: `"%d:%.4f"` joined by `;`, at most 3 entries, depth clamped to -1..1;
- `mod_pol`: `"<slot>:u|b"` joined by `;`;
- `mod_off`: sorted slots joined by `;`.

**`FXC_ROUTES`** grammar (`S/ModGraph.lua:263-321`):

```
v1;<key>=<src>@<depth %.4f>[@u|@b][@x],<src>@…;<key>=…
```

- keys are sorted;
- at most 4 routes per port;
- `@u`/`@b` give the link polarity;
- `@x` marks a bypassed link, which keeps its depth.

### 1.10 Other per-instance markers

- `shp` marker inside the instance for slider-shape conversion (CLAUDE ~1015-1020; exact slot unknown (?)).
- Data markers: `norm = 2` (tables, presets, snapshots), `sg_shaped`, `_shaped` (in `lfo.internal`).

---

## 2. Presets and snapshots (`S/PresetSystem.lua`, latest)

### 2.1 Hierarchy (RM §1 L38-46)

```
preset (the whole chain + generator + sequencer + LFO banks + links + pad + macros + morph)
 ├─ snapshots    (mapping + pad, INSIDE a preset; a snapshot needs a loaded preset)
 └─ granular_sets (grain grid, INSIDE a preset)
```

- Presets are a single machine-wide library (`presets.dat`), not stored in the project.
- The track only remembers WHICH preset it carries (`track_selections[guid].current_preset`).
- RM §8(b1) says modulations became LOCAL so that « un preset (rangé par piste) » can carry them. (?) on the wording.

### 2.2 Preset data model: `captureCompleteState()` (`S/PresetSystem.lua:1277-1424`)

```lua
presets[name] = {
  norm = 2,                                   -- values are REAPER-normalised (absent = old linear)
  gesture_x, gesture_y, gesture_base_x, gesture_base_y,
  track_guid = "{…}",                         -- informational, not used on load
  fx_chain = {                                -- 0-based slot index, chain order, keep-FX skipped
    [0] = {
      name = "<display name>",                -- TrackFX_GetFXName
      ident = "<fx_name>",                    -- GetNamedConfigParm "fx_name": original plugin name (pre-rename)
      renamed = "<renamed_name>" | nil,
      chunk = "<base64…>" | nil, chunk_kind = "vst_chunk" | "clap_chunk" | nil,  -- WHOLE plugin state
      enabled = bool,
      preset = "<TrackFX_GetPreset name>" | "",
      param_count = n,
      raw_params = { [1..n] = normalized },  -- EVERY param, incl. filtered ones and the host tail
      detached = true | nil,                  -- pool membership travels with the preset
      params = {                              -- ONLY params that carry something (since 2026-10-10)
        [param_id] = { name, current_value, base_value (ANCHOR), selected, range,
                       x_assign, y_assign, invert,
                       mod_source, mod_more, mod_pol, mod_off,  -- strings §1.9
                       dmods },               -- modulated depths §1.6
      },
    }, [1] = …,
  },
  sound_generator = { enabled = bool, osc = { {…},{…},{…} }, <every PARAM_SPEC path> },
  sequencer = { vals = { [p] = raw }, pat = { cur, mask, s = { [k] = "v,v,…" } } } | nil,
  lfo = {
    track_slots  = { [i] = { on, shape, rate, sync, phase, slew, curve, mode, uni, sh, len, steps } },
    global_slots = { same },                  -- global bank slot SETTINGS
    meta = { { slot, axis, bank, invert, base_rate } },
    internal = {                              -- links ON internal ports (local bank + macros + seq)
      ["s<i><rate|phase|curve|slew>"] = { src, scale, baseline, routes = {{src,depth,pol,off}…}, dmods },
      ["m<n>"] = …, ["q<p>"] = …,
      _seq = true,     -- a sequencer was present: the preset describes its links even if none
      _shaped = 1,     -- RATE/SLEW link values are in the shaped slider space
    },
  },
  pad = capturePad(),                          -- PAD_FIELDS + nav_on + lfo_meta + grains (format marker)
  macros = { [1..8] = value }, macro_names = { [n] = "name" },
  morph_a = {…}, morph_b = {…},                -- deep copies (§2.11)
  sg_mod = { [p] = { mod_source, mod_more, mod_pol, mod_off, range, invert,
                     x_assign, y_assign, selected } } | nil,   -- sparse; key = JSFX slider index p
  snapshots = { [name] = snapshot },           -- carried over by savePreset
  granular_sets = { [name] = { grid_size, grains = { {x, y, param_values = {[fx_id]={[param_id]=v}}} } } },
  -- legacy keys IGNORED on load: linked (script vs native mode), old flat SG fields (normalizeSGPreset)
}
```

**Keep-FX** (`isKeepFX`, 994-999) are never captured in `fx_chain` and never deleted by a load:
- internal FX: bridge, Sound Generator, `CP_Mod`, Sequencer, Mod Mix, Notes;
- protected instruments: RS5K, kits.

The generator, the sequencer and the banks are captured in their own blocks.

**Sound Gen capture is derived from `SoundGenerator.PARAM_SPEC`** (907-916). A slider added later is saved automatically: the old hand-written list lost the filter, glide and the rest in silence.
- `enabled` is written by hand.
- `jsfx_index` and `ui_osc_tab` are not saved.
- p=16 (manual trigger), p=31 (`env_out`) and p=52 (`peak_out`) are excluded on purpose.
- `osc[1].hz` and `osc[1].fine` are `PARAM_SPEC` entries without `p` (RM §17).
- The sample lives in `sound_generator.sample = { path, root=60, rs=0, re=1, start=0, loop=false, ls=0, le=1, xf=10, rev=false }` (`S/SoundGenerator.lua:534-548`). It is re-sent to the instance on load (`applyPresetSample`, 4301-4311).

### 2.3 Snapshot model: `saveSnapshot(name)` (534-611)

Requires a loaded preset; otherwise a message box says « Please load a preset first ».

```lua
presets[cur].snapshots[name] = {
  norm = 2, gesture_x, gesture_y, gesture_base_x, gesture_base_y,
  fx_list = { "<full_name>", … },
  fx_bypass_states = { ["<full_name>"] = true_if_bypassed },
  param_data = { ["<full_name>||<param name>"] =      -- params that are selected OR modulated
      { base_value (ANCHOR), range, x_assign, y_assign, invert, selected,
        mod_source, mod_more, mod_pol, mod_off, dmods } },
  pad = capturePad(), macros = {…}, macro_names = {…},
  lfo = captureLFOState(),                            -- banks + meta + internal links
}
```

- A snapshot is « le mapping et le pad ». It stores no plugin values except the anchors.
- Names come from `getNextSnapshotName()`: `"Snapshot"..(max N + 1)` (25-37), computed **at click time** on the current track's preset. Otherwise « + » could silently overwrite an existing snapshot (`S/UI.lua:1987-1993`).

### 2.4 Granular sets (53-123)

- `saveGranularSet(name)` requires a loaded preset and grains. It stores `{grid_size, grains={x,y,param_values[fx_id][param_id]}}`. Note the keys are **fx_id / param_id indices**, not names: fragile (?).
- Names: `"GrainSet"..N`.
- Load and delete are available from the preset menu.

### 2.5 Save: `savePreset(name)` (1426-1450)

1. `preset_data = captureCompleteState()`.
2. **If the name exists, carry over `snapshots` and `granular_sets`**: the bug fix in §2.9.
3. Set `current_loaded_preset = name`, `preset_modified = false`, `initial_fx_chain_state = captureFXChainState()`, `baseline_guid = trackGUID`.
4. Call `schedulePresetSave()` and `saveTrackSelection()`: the current preset is a TRACK fact.

UI behaviour (`S/UI.lua:1890-1905`):
- The disk button saves under the current name without asking.
- With no preset loaded, or on « Save as… », it asks with `GetUserInputs`.

### 2.6 Load: `loadPreset(name)`, exact order (1452-1802)

1. Set SWS `fxfloat_focus` to 0 if SWS is present, so no plugin window pops (restored at the end).
2. `Undo_BeginBlock`.
3. **Mute the track** (`B_MUTE`) for the whole rebuild; unmute at the end. There is no early return past this point.
4. **Delete every non-keep FX**, in reverse order.
5. Re-add the `fx_chain` entries in slot order:
   - `AddByName(ident)` first, then `AddByName(name)` (a renamed FX used to be declared « MISSING FX (not installed) »);
   - then `renamed_name`, enabled, `detached` (on the NEW GUID), then the named `preset`;
   - **then the chunk**, written AFTER the named preset so the preset cannot overwrite it;
   - the param-count warning is raised only when no chunk could be applied;
   - missing plugins are collected for the note.
6. **Sound Gen before the rescan** (inserting it shifts indices):
   - `normalizeSGPreset` maps old single-oscillator presets onto the 3-osc model and keeps width 10 for old presets;
   - if `enabled`: `applySoundGeneratorState` (copies only described paths; an absent osc table is skipped, absent fields become nil and are then filled by `migrateState` with « silent » late defaults), `createGenerator`, `updateJSFXParams`, `applyPresetSample`;
   - otherwise remove the generator if present.
7. **Sequencer** `restore(preset.sequencer)`, only if the block exists:
   - creates the instance, writes the raw settings (missing ones get the spec default) and `P_RUN=0`;
   - sends the 16 patterns, or resets them.
8. `scanTrackFX()`.
9. `restoreLFOState(preset.lfo)`:
   - creates a bank only if a saved slot is on;
   - writes the slots and steps (global bank too);
   - then the internal links;
   - `lfo_meta` is replaced entirely (nil = none);
   - an old preset without the block leaves the banks but clears the metas.
10. Reset `fx_collapsed`.
11. **Pad, macros, morph BEFORE the params** (`gesture_range` enters the conversion):
    - `applyPad(preset.pad)` only if `pad` exists. An old preset keeps the current pad;
    - morph A/B from the preset, `morph_amount = 0`;
    - `applyMacros`;
    - `current_snapshot = nil`.
12. Gesture x/y/base and the smoothing target; `updateJSFXFromGesture()`.
13. `beginBatch`. **Release every current routing**, except the generator's when the preset has no `sg_mod`: « rien à dire » ≠ « pas de modulation ».
14. Positional apply: `added_list[i] ↔ fx_data[i-1]`, with a name or `ident` safety check.
    - (0) `resetMapping`: the whole mapping of the FX back to defaults;
    - (1) `writeRaw(raw_params)`, skipped when a chunk was applied AND the param count changed. **Host tail fix**: trailing `wet`/`bypass`/`delta` are re-aligned from the END (`HOST_TAIL`, 1035-1060);
    - (2) **re-read the plugin** into `current/base/scan_value`;
    - (3) the described params, matched BY NAME through a name table: `writeStored` (an old preset, `norm ~= 2`, writes the raw value through `TrackFX_SetParam`, or `SliderShape.writeRaw` for a declared shape, then re-reads the REAPER norm); `storedIn` (shape conversion); `collectAnchor`; `selected`; `setParamRange`; `setXYExact` (both axes together, without `exclusive_xy`); `setParamInvert`; mod source, more, pol, off; `dmods`.
15. `restoreSGModState(preset.sg_mod)`, after the scan and the release:
    - an absent `p` means defaults;
    - a pre-PITCH preset reads `PITCH_FROM`, the old Hz key, without its range.
16. `endBatch`.
17. `Undo_EndBlock("Load FX Constellation preset: <name>")`.
18. `captureBaseValues()` and then **`republishAnchors()`**, which re-imposes the preset's anchors over the capture (determinism).
19. Set `current_loaded_preset`, `preset_modified = false`, the baseline, `links_rebuild = true`; `saveTrackSelection()`.
20. Unmute; restore `fxfloat`.
21. **No dialog.** `PresetSystem.note(msg)` goes to the status line for 8 s (`HD.say`, `S/UI.lua:1873-1881`):
    - « Preset 'x': not installed here -- A, B »
    - or « …: plugin version changed, values may differ -- … »

### 2.7 Snapshot recall: `loadSnapshot(name)` (613-774)

- **No question any more** (2026-10-10; Cedric: « il y a toujours un message que je ne comprends jamais »).
- A snapshot touches only the FX it KNOWS: those in `fx_list`, or with a described `fx||param` key.
- Known FX missing from the chain go to the status line (Sound Generator excluded).
- Pad and macros are applied if present. Then gesture position, base and the smoothing target.
- Per known FX: bypass state, then per param:
  - described params: `writeStored(base_value)`, range, XY exact, invert, mod source/more/pol/off, `dmods`;
  - NOT described: **deselected and source released**, so a snapshot restores exactly the set it stored, not a superset.
- Then `captureBaseValues`, `republishAnchors`, `restoreLFOState(snapshot.lfo)` (an old snapshot clears the metas), `links_rebuild`, `saveTrackSelection`.
- UI: `current_snapshot = name` (display only).

### 2.8 « Modified »

`checkPresetModification` (812-845), throttled to 0.5 s:
- compares the current chain (names + bypass, internals excluded) with `initial_fx_chain_state`;
- the baseline belongs to `baseline_guid`: on another track it adopts that track's chain; on the same track, it measures;
- `loadTrackSelection` resets only the flag, never the baseline (`H/FXManager.lua:1163-1174`).

The UI shows an amber dot next to the name, and the tooltip says « (modified) » (`S/UI.lua:1920-1926`).

### 2.9 The two bugs (RM §2, fixed in `ef79e6a`, ~2026-08-05/07)

1. **« (Modified) » written into the key.** `checkPresetModification` set `current_loaded_preset = "<name> (Modified)"`.
   - That string is the key of `presets[]`, and of the snapshots and grain sets filed under it.
   - So `presets[cur]` became nil: the snapshot list vanished, the combo looked empty, and `Snapshot<N>` restarted at 1. Toggling one bypass was enough.
   - Fix: the modified state is a FLAG (`state.preset_modified`), shown as a badge. Rule: « Une clé de table n'est pas une étiquette d'interface » (`H/Core.lua:190-195`; `S/PresetSystem.lua:812-814`).
2. **Save destroyed the snapshots.** `savePreset` did `presets[name] = captureCompleteState()`, a full replacement.
   - The capture produces neither `snapshots` nor `granular_sets`, so re-saving erased all of them.
   - The natural path to it: bypass → suffix → list gone → click Save.
   - Fix: carry both sub-tables across (`S/PresetSystem.lua:1426-1440`). Principle: a capture that does not capture everything must not overwrite what it ignores.
3. A third bug was found at the same time: the "modified" baseline must belong to the track being looked at (`baseline_guid`).

### 2.10 « Un recall doit être exactement ce que j'ai enregistré » / « un nouveau projet garde l'état d'avant » (2026-10-10 evening)

Commits `9e2730d`, `cda7cda`, `84c1d14` (RA L6254-6313). Guarded by `Tools/fxc_preset_test.py`: a fake REAPER with a real chain; set, save, disturb everything, recall, compare 145 lines sent to REAPER. Both the test and the commits are lost.

**Exact recall**:
- The pad was a global preference in `settings.dat`, and a preset carried only its position. Fix: the pad is per track, in presets and in snapshots.
- A renamed FX was declared missing. Fix: recall by `ident`, then `renamed`.
- A plugin's internal state (modes, loaded files, tables) did not come back. Fix: `vst_chunk` / `clap_chunk`.
- X then Y under `exclusive_xy` came back as « Y seul ». Fix: `setXYExact`.
- Bug: the Mod Mix bus entered and left the chain every frame (the sweep ran over an emptied `fx_data`). Fix: no sweep on a stale chain (`last_fx_count = -1`).
- Bug: a preset recalled in Walk or Figures restarted stopped when a bus was inserted (a moved bridge was re-adopted). Fix: compare the FX GUID in `pushEngine`.
- Bug: Init re-read the table from before it. Fix: flush before the rescan.
- Old behaviour: the full LFO slot was captured (previously only 5 fields), internal links were captured, and so were the polarity per link and the depth ports.
- The anchor is saved, not the applied value (§8(j) `a50e549`): otherwise the pad offset was baked in and re-applied, and the sound drifted at each round trip (`S/PresetSystem.lua:390-409`).

**New project = clean**:
- `settings.dat` holds only `PREF_KEYS`;
- a never-set track gets Core defaults (`applyPad(nil)`);
- `pad_src` per track in the project;
- an off Sound Gen parks its patch in the track;
- the lock is valid only in the active project;
- bridge adoption only for a bridge that existed before;
- a new bridge gets the full zone;
- a never-set track is deselected (no plinks written by looking).

### 2.11 Morph A/B: what exists

- `captureToMorph(1|2)` (`S/GestureSystem.lua:1836-1851`): for each FX, `{enabled, params = {[param name] = current_value}}`, **selected params only**.
- `morphBetweenPresets(amount)` (1853-1885): a linear interpolation between A and B by FX name and param name.
  - On a CP-linked param it writes the link **baseline**; otherwise it writes the normalised value.
  - It sets current/base/anchor.
- UI: snapshot menu « Set A = now », « Set B = now », « Clear A / B » (`S/UI.lua:2051-2063`). An `A→B` slider appears in the command bar only when both exist (956-966).
- Stored: in the track entry (`morph_a/b`) and in presets. `morph_amount` is reset to 0 on track arrival and on load; it is not stored.
- **Not built**: the « morphing à la Vital » (RM §22.1, 2026-10-08) is a PROPOSAL only. The proposal: a SHAPE dial per oscillator (triangle asymmetry, PWM, sine phase distortion) before any wavetable. Cedric never approved it; there is no commit.

### 2.12 Init: `initPreset()` (1804-1947)

Cedric: « Il me faudrait un Init preset. » One undo block (« FX Constellation: Init »); the track is muted during it.

1. `releaseAllLinks()` plus `restoreInternalLinks(nil)`. Clear `param_mod_source/more/pol/off`, deselect, range 1, invert false, `param_base_values = {}`, `bumpModGen`.
2. Delete the ROLL chain; **DETACHED FX stay in place**, and keep-FX stay.
3. Sound Gen back to `_sg_default` (captured at init after `migrateState`), **only if present**: Init adds nothing.
4. `Sequencer.reset()`: spec defaults, stopped, one pattern. Only if present.
5. Local bank, if it exists:
   - slot 1 on, the others off;
   - each slot `{shape=0, rate=1, sync=0, phase=0, slew=0, curve=0, mode=0, uni=false, sh=0, len=ST_STEPS}`;
   - steps flat at 31;
   - `lfo_meta = {}`.
6. `applyPad(nil)`; morph cleared; pad at the centre; `clearZone`; `pad_src = 0,0`; macros 0; `macro_names = {}`.
7. `saveTrackSelection` + `flushTrackSelection` BEFORE `scanTrackFX`, then `syncPadAxes`, `captureBaseValues`.
8. `current_loaded_preset = ""`: **Init is not a saved preset** and cannot be overwritten by Save.

Preferences stay untouched.

UI: a confirmation box: « Init clears this track's ROLL chain, releases every FX Constellation link, untracks all params and resets the Sound Gen, the modulators, the macros and the pad. DETACHED FX stay where they are. Undo restores everything. » (`S/UI.lua:2110-2123`).

### 2.13 Header UI (`S/UI.lua:1788-2178`)

Requested as « Mes presets, snapshots, sous forme de dropdown plus riche […] la possibilité d'enregistrer, charger, init preset ».

**Command-bar layout, Vital-style**: `[<] preset [>] [disk] [<] snapshot [>] [+]`. Field widths: `PRESET_W = 150`, `SNAP_W = 112`.

**Preset group**:
- the arrows cycle through the sorted preset list without opening it (with no preset loaded, they start from either end);
- the name field shows `no preset` when empty;
- clicking the name opens the menu:
  - title;
  - « Init — start from an empty constellation… »;
  - Save 'x' / Save…, Save as…;
  - the preset list, flat up to 24 entries, otherwise under a « Load » submenu;
  - Rename…, Delete… (with confirmation);
  - Save grain set…, Load grain set ▸, Delete grain set ▸.

**Snapshot group**:
- label: the current snapshot name, `N snapshots`, `no snapshot`, or `snapshots` when no preset is loaded;
- « + » captures `getNextSnapshotName()`;
- menu: « Capture as 'SnapshotN' », « Capture as… », « Overwrite 'cur' », the list (checked = current), « Rename 'cur'… », « Delete 'cur' », and the morph items;
- with no preset: « A snapshot lives inside a preset », then « Save a new preset… ».

Every preset/snapshot verb is license-gated (ignore). The preset list is never replaced while its popup is open (RM §10.5).

**Macros**: bottom-left of the left zone, 8 knobs of 30 px, name below, source pill on the top-right corner. They were moved there from the bar at Cedric's request: « Les macros pourraient etre en bas a gauche, a la place des snapshots, et ceux-ci iraient en haut, dans le header, comme pour la majorite des VST. » Values are read from the bridge at 10 Hz; right-click renames (`S/UI.lua:457-470`).

---

## 4. The old CLAUDE.md rules about FX Constellation (latest: `CLAUDE`, end of October)

Condensed; the original wording is French; line numbers approximate.

1. **`FX Constellation/Data/*.dat` ARE committed** (since 2026-08-22, reversing the earlier rule). Cedric works on two machines and wants the second to match. (~84-91)
2. **A generated JSFX is rewritten on every open** (Sound Gen `init`, sequencer `init`, banks `refreshBankFiles`, bridge `writeBridgeFile`). REAPER compiles at load, so fixes reach existing projects at the next load. A LAYOUT change goes through `PARAM_COUNT` and the lossless upgrade. A computation done in `@block` must also be done at note time (`v_prep`). (~966-974)
3. **« UN PARAMETRE A PLUSIEURS SOURCES EST LIE A UN BUS, PAS A SES SOURCES » (2026-10-06).** A native link is one-to-one. The sum happens in `FX Constellation - Mod Mix`; the param links to the bus OUTPUT (scale 2, offset -0.5); a single source keeps its direct link.
   - The bus goes AT THE HEAD, and links are re-aligned by GUID.
   - It is never inserted during `syncLinks` (the sweep reports `mix_missing`/`mix_unused`, the window loop acts).
   - The first source's range lives inside the bus.
   - `setParamModSource(…,0)` also clears the extra sources. (~977-994)
4. **The normalised value is REAPER's; curved sliders declare their shape (`:log`/`:sqr`, 2026-10-07).**
   - Lua never computes a norm from a raw value (`getParamNorm`/`setParamNorm`).
   - A curved dial is a shaped slider.
   - Changing a shape changes the meaning of every stored norm: convert ONCE (`SliderShape.convertLink`: exact base, peak kept) with markers in the instance (`shp`) and in the data (`norm=2`, `sg_shaped`, `_shaped`), through a project pass.
   - Measure before converting (`SliderShape.verify`). (~994-1026)
5. **On a shaped slider `TrackFX_GetParam`/`SetParam` speak the shape domain** (ln v for `:log`, √v for `:sqr`; measured 2026-10-08). 440 Hz written raw became e^440.
   - Raw values go through the normalised value (`writeRaw`/`readRaw`, `writeSpec`/`readSpec`, `writeField`/`readField`).
   - For shaped sliders the range comes from `PARAM_SPEC`, not `GetParamEx`.
   - Damaged instances are repaired once (`repairRaw`, `repairBank`).
   - Test fakes must tell the truth. (~1028-1046)
6. **A link on a link is a port and its key nests** (`r:P|src`, `r:r:P|1|211`, 2026-10-07).
   - Walk the chain up (`chainOf`); never compare prefixes: preset storage had lost the second level for that reason.
   - The root is computed at port creation.
   - The bus computes lanes in order, the child at a LOWER rank. (~1048-1060)
7. **A link has its own polarity** (2026-10-08): `linkCenter` (the link's `@u`/`@b`, else the source's). A site that compiles or draws ONE link never asks `sourceCenter`. (~1062-1068)
8. **Fast modulation: two JSFX, one shared memory** (`gmem CP_FXC_FAST`, one channel per track, every 32 samples, bank → Sound Gen).
   - EEL2 ignores case.
   - No `]]` in a JSFX source held in a Lua long string.
   - The Sound Gen renders from EFFECTIVE values.
   - A bank that stops publishing is not a bank publishing zero (block counter). (~1069-1088)
9. **Block-rate gains are ramped, then smoothed with a fixed time constant** (two 1.5 ms poles, `ZTAU`). This fixed the clicks: « Le SEQ sur le FM d'OSC 1 : clic et pop », « j'ai toujours des clicks ». It was measured at 64/128/512 blocks. (~1090-1110)
10. **Sequencer MOD A/B are fast routes** (slots 9, 10 of the channel). A line value is a function of the DATE, swing included. Both instances must know `FAST_COUNT`. (~1118-1129)
11. **Sequencer patterns**: the sliders hold the CURRENT pattern and the JSFX memory holds the 16 (2026-10-11).
    - Lua REQUESTS once per audio block.
    - The first block after `@serialize` ADOPTS the stored request.
    - Lua reads and writes the 16 through gmem (`PAT_GBASE` = 2^21), so presets, Remove and upgrades carry them.
    - A new pattern value goes at the END of `PAT_P`. (~1131-1153)
12. **The Sound Gen sample: the generator is the authority** (2026-10-10).
    - The path is in `@serialize`; the window re-reads it and SENDS a path only on a gesture (From Instrument, Load, Clear, a preset), never to « get back in sync ».
    - The mailbox is at 3072 in the fast channel, with a token.
    - An upgrade loses `@serialize`, so the path is re-sent. (~1155-1180)
13. **The live point of a modulated dial is computed by the DLL** (ABI 3.8): `getLiveValue(…, model)`; the default is to refuse; one clock per slot. (~1182-1199)
14. **OSC 2 → OSC 1 phase modulation**: the law is written twice (JSFX and drawing), so change both together. Fast routes are at 19 codes, the ceiling. (~1201-1208)
15. ⚠⚠ **« UN PRESET DIT TOUT CE QU'ON ENTEND, ET UNE PISTE NEUVE PART DES DEFAUTS » (2026-10-10).** « Un recall doit etre exactement ce que j'ai enregistre », and « un nouveau projet garde l'etat d'avant ». Eight rules:
    - (a) **the pad is a TRACK setting.** `PAD_FIELDS` is the only list, stored in `track_selections[guid].pad`, in every preset and every snapshot. A never-set track gets Core defaults (`applyPad(nil)`). `settings.dat` writes and reads ONLY `PREF_KEYS`. Pad sources have a per-track project key `pad_src:<guid>`;
    - (b) **an FX is restored by its ORIGINAL name** (`fx_name`), renamed (`renamed_name`), and receives its WHOLE state (`vst_chunk`/`clap_chunk`); `raw_params` stays the fallback. A preset describes only the params that carry something; recall resets the FX mapping (`resetMapping`) and then RE-READS the plugin values;
    - (c) **no box on recall** (`PresetSystem.note` → the status line). A snapshot touches only the FX it knows;
    - (d) **both axes are restored together** (`setXYExact`);
    - (e) **a chain declared stale (`last_fx_count=-1`, empty `fx_data`) is not swept**, and is re-read without the 50 ms step;
    - (f) **a MOVED bridge is not another bridge** (GUID compare in `pushEngine`);
    - (g) **the lock is valid only in the ACTIVE project** (`ValidatePtr2(0,…)`);
    - (h) **an off Sound Gen parks its patch in the track** (`P_EXT:CP_FXC_SG_PARK`). (~1212-1244)
16. **The track's notes are sources** (NOTE/VEL/RAND/OCT, 231..234, `FX Constellation - Notes`, 2026-10-10).
    - It sits behind the sequencer and in front of the Sound Gen.
    - It passes all MIDI through.
    - It is inserted and removed outside a gesture (`notes_missing`/`notes_unused`); removed, it parks its settings (`P_EXT:CP_FXC_NOTES_PARK`).
    - The window keyboard writes no MIDI.
    - The band is tested before the sequencer's. (~1246-1265)
17. The halo reserve must hold every tint of a frame (`GLOW_TINTS`). **A test harness never reads Cedric's real settings** (`fxc_frame_test` read the real `settings.dat`). **The link strip lives IN the list, never in a popup**; its gestures are deferred after the loop. (~1267-1285)
18. **An internal FX is inserted through `CP_Engine/QuietFX.lua` (`Core.addFX`)**, which sets `fxfloat_focus` to 0 during the add: no window flash (2026-10-11). (~1424-1430)
19. Keyboard: FXC has NO shortcut of its own except Escape. A hosted view gives back to REAPER (`CP_UIKeyPass`) any key its frame did not consume; « une touche qu'on veut, on la CONSOMME ». (~296-332)
20. Harness list (lost): `fxc_preset_test`, `fxc_notes_test`, `fxc_pool_test`, `fxb_pool_test`, `fxc_retire_test`, `fxc_internal_link_test`, `fxc_shape_test`, `fxc_graph_test`, `fxc_multimod_test`, `fxc_frame_test`, `fxc_layout_test`, `fc_fx_test`. (~1570-1650)

Code-level rules repeated in comments that also matter:
- `current_loaded_preset` is a key, never a label;
- save the anchor, never the result;
- « rien à dire » ≠ « rien » (`sg_mod`, `_seq`, `known` FX);
- positive markers (`native=1`, `norm=2`) are rewritten at every flush;
- the pad position is restored only on a track change.

---

## 5. Roadmap decision log (RM; summarised from a sub-agent read; L = RM line)

**§2**: the two bugs of §2.9 (L51-101).

**§8, order of the work (L450-590)**:
- (b1) **Cédric's decision**: modulations become LOCAL (per-track bank) so that a preset can carry them (L459-463).
- (b2) generator env 201, 8 macros 211..218, one single slot resolver.
- (j) `a50e549`: persist bases, never live values (L494-496).
- The adversarial review found 14 defects. For state:
  - "slider count = instance version": the bridge went 6→22, saved projects carried a short instance, and pad links were released silently;
  - a macro link inherited the pad anchor;
  - `offset=-0.5` is wrong for an envelope.
- Set aside: no ADSR in the bank; S&H = Random shape; audio FOLLOW « se tranche avec Cédric »; step sequencer format.

**§9, still to decide (L591-610)**: eight macros (« Je pars sur huit », assistant). Nothing about state.

**§12, regressions reported by Cédric on 2026-08-07 (L1033-1148)**:
- 12.1 « J'ai essayé de linker un LFO global à un paramètre… le paramètre n'est pas réellement affecté » (the MIDI plink was not set).
- 12.2 « impossible de comprendre la range ».
- **12.3** « J'avais linké X et Y du pad à des LFO… Certaines valeurs me semblent pas correctement recall » → fixed by (j): the result had been saved as the base.
- 12.4 « la sensation générale du script paraît moins stable, moins cohérente ».

**§14 (2026-08-22)**, « il est temps d'arrêter de construire sans réfléchir »:
- REAPER norm is the only language, `.dat` lines included;
- one owner per value;
- **Cédric's choice**: on release the live value returns to the BASE (« je préfère un modèle stable qui clique un peu que quelque chose qui n'est pas prédictible »).
- Unchecked: release-to-base, `from01`, ModKnob receiving norm, `fxc_space_test`.

**§15, four envelopes (~08-22/23)**:
- Cédric: « 4 ENV et 8 LFO », « cliquer sur ENV 2 et voir l'ADSR dans la partie droite ».
- Built in the generator: `PARAM_COUNT` 59→74, slots 201..204; « 201 ne se renumérote jamais ».
- **Open**: the Matrix was blind to envelopes (?); `reconcileModSources` does not adopt env links; per-voice ENV 2-4.

**§16, Capture (2026-08-23)**:
- Cédric: « record tout ce qui se passe dans l'app, directement en automation enveloppe dans l'arrangeur ».
- Record the SOURCE, never the destination. Refusals are stated. No chain restructuring during a pass. Notes go to a real MIDI item.
- **Open**: Walk and Figures cannot be recorded; Continuous mode ignores MIDI.

**§17, OSC 1 pitch (2026-10-04)**:
- FINE is state; OSC 1 is born without width (Cédric); NOTE/HZ without a slider (P_EXT key).
- **Open**: an OSC 1 in HZ keeps its cents only through a preset; the reopen loses FINE.

**§18, lot of 2026-10-04**:
- Pools: detaching releases the rolled links (decided in his stead); the key is the FX GUID in P_EXT.
- Zone on the bridge v5. **Cédric asked** that a driven pad stay inside the zone (`pushPadZone`).
- Header bar plus Init (Init « décidé à la place de Cédric »).
- RATE/PHASE/CURVE/SLEW and macros accept any source.
- **Cédric reversed §3.4**: pad X/Y pills now EMIT (« Ne pas "rétablir" §3.4 »).
- **Open**: global-bank internal links are NOT in presets (the bank is shared on CP MOD); `lfo_meta` should become a native link.

**§19, pitch in semitones (2026-10-04 evening)**:
- Cédric: « Je lie un mod à OSC 1, je change sa base, rien ne se passe ».
- Sliders 87..98; Hz stays the base, `pm` in semitones; a new link gets ±12. One scale per oscillator (decided in his stead). Hz links move to `pm` without their range.

**§20, sequencer tab**: protected FX; sources 221/222. 20.1: the rack becomes a grid. Cédric: « Il y a trop de choses, je dois absolument scroller ; quand tu vois ce que fait Vital… »

**§21, modulation engine (2026-10-07)**:
- Cédric: « Est-ce que c'est vraiment un moteur de liaison réelle… lien sur des liens sur des liens… Je ne veux pas qu'on soit limité à l'avenir. » Then « Oui, go ».
- Phase 1: slider shapes, with tables, snapshots and presets converted at load.
- Phase 2: ModGraph with `FXC_ROUTES`, up to 4 sources.
- Phase 3: depth ports, Mod Mix v2. « Presets et instantanés rangent les profondeurs modulées ».
- Phase 4: LinkStrip, the Links tab, Map mode (not saved).
- Additions: pad plus sources on the same param; **per-link polarity, saved** (`param_mod_pol`, `mod_pol`, `@u`/`@b`).
- Phase 5: fast routes (`FXC_FASTCH`). Live dot: « ce n'est pas tight ».
- Rejected by Cédric: REAPER's native LFO (« je ne vais plus m'en servir »).
- **Open**: per-voice ENV 2-4 « à trancher avec Cédric »; Map depth drag at Lua rate; other live dials.

**§22 (2026-10-08)**:
- FM by phase (slider 133).
- Pad point: restored only on a track change; a never-touched track arrives centred.
- **22.1 Vital morph: proposal only**, never approved, never built.
- 22.2 sample from the Instrument, built 10-10 (Cédric: « transformer en jouant »).
- **Open**: printing back to the Instrument, one sample per oscillator.

**§23, house-effect views (2026-10-25)**:
- Cédric: « que chaque JSFX ait une interface à lui… normale et compacte… EQ avec de vraies courbes ».
- Decided: only the GRAPH is custom. EQ3, Filter and Compressor are done.
- **Open**: the rest of the catalogue, the embedded view, an FX Rack host.

**Open as of November** (nothing was done on FXC after 2026-10-25):
1. per-voice ENV 2-4;
2. FOLLOW;
3. the SHAPE / Vital morph;
4. global-bank internal links in presets;
5. `lfo_meta` → a native link;
6. OSC 1 HZ cents on reopen;
7. recording Walk and Figures;
8. the §14 checklist;
9. Links-tab coverage of envelopes (?);
10. the house-effect display unit;
11. sample resampling, multiple samples;
12. the §23 catalogue and the FX Rack;
13. the memory note: confirm in REAPER that `vst_chunk` exists and that `fx_name` is accepted by `AddByName`;
14. the memory note: « clip MIDI relancé sur clic FX Constellation » (still OPEN).

---

