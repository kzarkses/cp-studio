# FX Constellation — behavioral spec: WINDOW, PARAMETERS, PAD (reconstructed 2026-11-25)

Scope: the window (layout, controls, gestures, menus), the parameter model, the ROLL randomizer, the XY pad
(Manual / Walk / Figures / Granular, zone, bridge JSFX). The modulation rack, Sound Gen tab, Seq tab, LinkStrip and
Capture are only touched where they meet this scope.

## 0. Sources and reference keys

| key | file | state |
|---|---|---|
| UI | S `FX Constellation/Modules/UI.lua` (9410 l., Oct 25 19:41) | **latest, code wins** |
| GS | S `Modules/GestureSystem.lua` (1887 l.) | latest |
| LE | S `Modules/LinkEngine.lua` | latest |
| MD | S `Modules/ModDrag.lua` | latest |
| PS / PER | S `Modules/PresetSystem.lua` / `Persistence.lua` | latest |
| FXMUI / KB | S `Modules/FXManagerUI.lua` / `Keyboard.lua` | latest |
| CORE | H `claude_file_history/0c5c4909…/a9c6ba36624bb656@v2` (= Core.lua, 2026-10-25) | latest |
| FXM | H `claude_file_history/0c5c4909…/3d2fc94ff4d62d7c@v2` (= FXManager.lua, 2026-10-25) | latest |
| WID | H `claude_file_history/90a0a624…/ca777f936157f78c@v2` (= CP_Toolkit/Widgets.lua) | latest toolkit |
| BR | J `Effects/FX Constellation Bridge.jsfx` (Oct 25) — identical to `GS.bridgeSource()` GS:547-746 | latest |
| RM | S `ROADMAP_FXConstellation.md` (2825 l.) | latest |
| MAIN | M `FX Constellation/CP_FXConstellation.lua` (164 l., 2026-08-22) | old but launcher only |
| CFG | M `CP_Config/Constellation.lua` (theme preset = mockup v5 palette) | Aug |
| AN | M `ANALYSE_FXConstellation.md` (Aug analysis; line numbers there refer to Aug code) | Aug |
| CD-A / CD-B / CD-C | M `Claude Design/constellation.jsx`; M `Claude Design/CP_FXConstellation Port.html` (+cp-*.jsx); claude.ai artifact "FX Constellation — v5" (https://claude.ai/code/artifact/93495265-1221-4bd2-86d5-d0287406559f, linked from RM:17-18) | mockups |

Value-space vocabulary used below: **norm** = REAPER's normalized 0..1 (`TrackFX_Get/SetParamNormalized`), the only
inter-module currency (RM §14.2, CORE:600-610). **raw** = slider units (`TrackFX_SetParam`). "bh" = `theme.button_height`
(22 px in the Constellation theme, CFG).

⚠ Homonym: in this product **"portée" = the per-param `range`** (how far a param deviates). In the rendering memory notes
(project-portees-graphiques.md) "portée" means a *binding scope* of the native renderer — unrelated.

⚠ "NAVIGATION" (one of Cedric's 4 most-used features: RANDOMIZER, XY PAD, NAVIGATION, PRESET — memory
project-fx-constellation-refonte.md) is **the pad's motion mode section** (Manual / Random Walk / Figures —
`Core.navigation_modes` CORE:5, CD-A "▼ NAVIGATION"), **not** navigation between FX or tracks. Track following is §2.11.

---

## 1. What the window is (RM §1, RM:22-49; HELP_TEXT UI:742-768)

- No "constellation" object. One **track**, and a **tracked subset** of its FX params. Each tracked param has a
  **base** (`base_value`, rest value), a **range** (`range`, how far it moves), an **axis** (X, Y, both, none) and an
  **invert** flag.
- The XY pad applies an offset **relative to each base**, measured from the pad **anchor** (`gesture_base_x/y`, where
  the pad was when the gesture started).
- The only true interpolation is **Granular**: weighted blend between **randomly drawn** grain points.
- Save hierarchy: `preset` (whole chain + Sound Gen + modulators + links) ⊃ `snapshot` (mapping + pad, inside a preset)
  and ⊃ `granular set` (grain grid, inside a preset).
- Everything that sounds must run without the window: the pad→param path is **native REAPER parameter links**
  (plinks) from the bridge JSFX outputs; only Granular stays script-driven (CORE:119-131, LE:1-45).

---

## 2. THE WINDOW

### 2.1 Frame and geometry (UI:2-25, UI:9169-9408, MAIN:111-115)

Window: toolkit window "FX Constellation", default 1400×800, persisted key `CP_FXConstellation`, padding 8, not
scrollable (MAIN:111). Heartbeat ExtState `CP_FXConstellation/alive` every 0.5 s, `cmd` = named command (MAIN:117-162).

```
┌ COMMAND BAR (BeginBar "fxc_cmd", title "FX Constellation") ──────────────────────────────┐
├ LEFT ZONE ──┬seam┬ CENTER ZONE (tabs Parameters | Sound Gen | Seq) ──┬seam┬ RIGHT ZONE ─┤
│ pad (square)│    │  tab body (scrolls itself)                         │    │ source rack │
│ pad foot    │    │                                                    │    │ (foldable   │
│ nav detail  │    │                                                    │    │  to a rail) │
│ ROLL + 8    │    │  ─────────────────────────────────────────         │    │             │
│ MACROS 2×4  │    │  KEYBOARD bar (+ 3-octave piano when open)         │    │             │
├─────────────┴────┴────────────────────────────────────────────────────┴────┴─────────────┤
└ INFO BAR (AppStatus, 22 px, anchored to window bottom) ──────────────────────────────────┘
```

- Body height = `win_h − STATUS_H(22) − 4 − cursor_y`, floor 4·bh (UI:9296-9301). Columns `{left_w, 0(flex), right_w}`,
  gap = `floor(theme.window_padding)` (UI:9303-9319).
- Default widths: left `floor(14.5·bh)`, right `floor(12.5·bh)`, folded rack rail `floor(0.95·bh)` (UI:415-417,
  442-455). Stored widths `zone_left_w`/`zone_right_w` (nil = default) are clamped **at read** against the current
  window: min `5·bh`, max `win_w − 2·min − 3·padding` (UI:431-440).
- **Seams** (UI:9322-9393): two vertical seams (`UItk.SeamV`, 2-px relief line). Hit band `SPLIT_W = max(4,
  floor(frame_padding_x·0.7))` ±2 px; cursor `size_we`; drag left seam: `w += dx`; right seam: `w −= dx` (grows
  leftward); clamped by `zoneClamp`; **double-click resets to default** (`s[key] = nil`). Right seam not draggable when
  rack folded. Hover shows a 1-px accent line α0.7; active drag paints it.
- Persisted frame prefs (settings.dat, `Persistence.PREF_KEYS` PER:459-468): `zone_left_w`, `zone_right_w`,
  `zone_right_collapsed`, `center_tab` (1..3, by INDEX — order of tabs must never change, UI:5520-5523), `fx_show_all`,
  `show_param_values`, `ultra_random_settings`, `fx_view_size`, plus randomizer prefs (see §4).
- No track: H1 "No track selected." + "Select a track in REAPER, or use the Lock button to pin to one.", info bar and
  modals still drawn (UI:9277-9290).
- Draw order per frame (UI:9169-9408): compute rack focus/hover slot → command bar → `syncTrack()` (all model work) →
  Esc handling → Record poll → MIDI poll → redraw policy → body (left, center, right) → seams → info bar → modals →
  FX-header drag ghost (`fxDragFinish`) → modulation-drag wire (`ModDrag.Draw`, closes the drag gesture).
- Redraw policy: continuous redraw while `GS.IsAnimating()` (pad axis driven, walk/figures active, smooth chase
  pending); else 30 Hz while (bridge has X/Y envelope and transport playing) or Record needs frames or any
  LFO-modulated param exists; else idle (UI:9259-9275, GS:1688-1708).

### 2.2 Command bar (UI:770-969)

Toolkit bar with a fixed width scale `BAR_W = {22, 54, 76, 104, 140}`; a control's `w` is an index into it (UI:653-671).
Right side reserved FIRST so it survives narrowing: **[Settings] (icon Settings2)** toggles the Settings modal;
**[Help] (icon Help)** shows `HELP_TEXT` (UI:796-803). Left side, in order:

| # | control | behavior | ref |
|---|---|---|---|
| 1 | title "FX Constellation" | bar title | UI:791 |
| 2 | label `· <track name>` (muted; "· —" if unnamed; "· no track") | refreshed at 2 Hz | UI:779-805 |
| 3 | PRESET group: `[<] [preset field 150 px] [>] [💾]` | see below | UI:1907-1938 |
| 4 | SNAPSHOT group: `[<] [snapshot field 112 px] [>] [+]` | see below | UI:1957-1994 |
| 5 | **Record** toggle (red accent, icon "Record") | reflects REAPER `I_RECARM`; arming writes `I_RECARM=1`, `I_RECINPUT=6112` (all MIDI, all ch), `I_RECMON=1`; disarm writes 0 | UI:822-844 |
| 6 | **Capture / "Capture N"** (w=3, red when N>0) | opens Capture menu (§2.8) | UI:857-865 |
| 7 | **Lock / Unlock** (w=2, active when locked) | pins `locked_track = track` | UI:868-875 |
| 8 | segment **Single \| Granular** (w=3 each) | `pad_mode` 0/1; Granular inits grid if empty; sets `links_dirty` | UI:879-897 |
| 9 | segment **Manual \| Walk \| Figures** (w=3) | `navigation_mode` 0/1/2; **re-clicking the current Walk/Figures re-arms it** if a drag stopped it | UI:909-955 |
| 10 | `A→B` morph BarValue (w=3, 0..1, step 0.05) — only when both morph captures exist | `GS.morphBetweenPresets(v)` | UI:958-966 |

- Preset field = sunk box, amber 3-px dot when `preset_modified` (flag, never in the name), chevron-down; muted
  "no preset"; tooltip "Preset: name (modified)\nClick: save, load, init, rename…"; click → preset menu.
  `[<]/[>]` cycle the sorted preset list **with wrap-around** and load; `[💾]` saves to current name (or asks a name
  if none) (UI:1846-1938, HD.savePreset UI:1890-1905).
- Snapshot field label: "snapshots" (no preset) / current snapshot name / "N snapshots" / "no snapshot" (muted when
  none recalled). `[<]/[>]` cycle and recall; `[+]` captures `Snapshot<max+1>` (name computed at click,
  PS:25-37); disabled without a preset (UI:1957-1994).
- Walk activation (UI:921-943): `random_walk_active = true`, `next_time = last_time = now` (first event NOW, not one
  period later), `last_slot = nil`, `generateRandomWalkControlPoints()`, `captureBaseValues()` (re-anchor).
  Figures: `figures_active = true`, `figures_time = 0`, re-anchor. Manual: both actives false. All `scheduleSave`.
- Walk, Figures, Granular, ROLL, presets/snapshots are license-gated (ignore for the rebuild).
- ⚠ Code vs comments: UI:10-18 still says "8 macros in the bar" — **outdated**; macros moved to the bottom of the left
  zone on 2026-10-04 (RM §18.3, UI:457-473). HELP_TEXT (UI:749-767) also says "a macro in this bar" and "click the
  engine text to switch" — both outdated (engine text is a read-only status since UI:8380-8394).

### 2.3 Left zone (UI:2183-2224) — one vertical scroll child, in this order

1. **Pad** (`drawPad`, UI:1062-1362). Square; side = zone width − scrollbar gutter (reserved always, avoids flicker),
   min 80, capped at `floor(viewport_h·0.42)` when that is > 80 (UI:2198-2215).
2. **Pad foot** (UI:1427-1485): columns `[readout] [anchor] [Zone ×]? [Reset]`.
   - readout `"%.2f, %.2f"` of `gesture_x, gesture_y` (caption, muted); during a drag it is rendered natively and bound
     to the mouse so it updates at screen rate (UI:1374-1417).
   - `"anchor %.2f, %.2f"` of `gesture_base_x/y`.
   - **Zone ×** (only when a zone exists): `clearZone()` + `reapplyPad()`; tooltip "Clear the zone. Right-drag on the
     pad draws a new one, a plain right-click clears it."
   - **Reset**: pos = anchor = target = (0.5, 0.5), write bridge, `captureBaseValues`, `saveTrackSelection`, re-apply
     (granular or single).
3. Separator, **navigation detail** (`drawNavDetail`, UI:1517-1669) — content depends on `navigation_mode`:
   - Manual: `[Smooth 0..1 "%.2f"] [Speed 0.1..10 "%.1f"]` → `smooth_speed`, `max_gesture_speed`.
   - Walk: row 1 `[☐ Jump] [Sync combo: Free (Hz) | 1/16 | 1/8 | 1/4 | 1/2 | 1 bar]` → `random_walk_jump`,
     `random_walk_sync` 0..5 (changing either resets `last_slot` and sets `next_time = now`). Row 2 only when Sync =
     Free: `[Speed 0.1..10 "%.1f Hz"] [Jitter 0..1 "%.2f"]`. Dragging Speed while active only **pulls the next event
     earlier** (`next = min(next, now + 1/speed)`), never pushes it later (UI:1574-1584).
   - Figures: a grid of **6 square chip cells** (`cell = floor(bh·1.2)`, gap item_spacing) with drawn icons:
     0 circle, 1 square, 2 triangle, 3 diamond (two triangles), 4 "Z" drawn as an X (two diagonals), 5 infinity (two
     circles) (UI:1490-1515); click sets `figures_mode`, `figures_time = 0`. Then `[Speed 0.01..10 "%.2f Hz"]
     [Size 0.1..1 "%.2f"]`.
   - Always: **Range** dual-thumb RangeSlider 0..1 → `gesture_min`, `gesture_max` (drag the middle translates the
     window) (UI:1635-1643). Meaning: clamp of each param's VALUE in the script path (see §3.5), NOT a pad-position
     limit.
   - Granular only: `[grid combo 2x2 | 3x3 | 4x4] [Randomize grains]` (cols 0.4/0.6) (UI:1647-1668).
4. Separator, **ROLL** (`drawRoll`, UI:1718-1787): full-width button "ROLL" (height `floor(bh·1.4)`), then one row of
   **8 axis chips** `P B R XY N BY OR MO` (cell = `floor((w−14)/8)`, gap 2, height `floor(bh·0.95)`), then caption
   "N of 8 axes armed". Chip: **left-click = arm/disarm** that axis (`ultra_random_settings[key]`), **right-click = roll
   that axis alone now**; tooltip "<tip> — click to arm, right-click to roll it now". Details §4.
5. Separator, **MACROS** (`drawMacroZone`, UI:488-651): caption "MACROS"; 2 rows × 4 knobs (§3.10). Without a bridge:
   caption "enable Auto JSFX (Settings) to use the macros".

### 2.4 Center zone (UI:5723-5799)

- TabBar `CENTER_TABS = {"Parameters", "Sound Gen", "Seq"}` → `center_tab` (UI:5523, 5727-5732).
- Body child height = `kb_y − cursor_y − 2`, `kb_y = win_h − 22 − 4 − kb_h` (keyboard anchored absolutely at the
  bottom; body yields first, floor 2·bh) (UI:5752-5777).
- **KEYBOARD** (KB, drawn under all three tabs): collapsible bar "KEYBOARD" (fold state `kb_open`, session only),
  piano of 3 octaves starting C3 (+ top C) (KB:54-64); Triggered mode = press-hold (glissando by dragging), Continuous
  mode = latch (click toggles a held note, turns `cont_note` on). It writes Sound Gen sliders `kbd_note/kbd_gate/
  kbd_vel` (not MIDI). Right of the bar: 4-s explanation when incoming MIDI is ignored (UI:5650-5715).

#### 2.4.1 Parameters tab (UI:4956-5038)

Toolbar row (widths measured from labels): **[Add FX…]** opens the standalone `CP_FXBrowser.lua` script (registers it
if needed; fallback: the FXManagerUI modal) · **[Collapse]/[Expand]** all groups (`all_fx_collapsed`, writes
`fx_collapsed[fid]` for every FX) · **[Untrack all]** (deselects all params of every non-Sound-Gen FX) ·
**[Values]** toggle → `show_param_values` (value column in rows) (UI:4962-5002).

Then two **pool columns** (cols 0.6 / 0.4, gap 2·item_spacing), always both shown even if empty (UI:5029-5037):

| column | label | header | empty caption |
|---|---|---|---|
| left | **ROLL** (text color) | "ROLL  N FX" + **[+]** chip | "No FX in the roll. + adds one, or drag an FX header here." |
| right | **DETACHED** (muted label) | "DETACHED  N FX" + **[+]** | "Drag an FX header here to keep it out of ROLL." |

- Tooltips: ROLL "FX that ROLL re-rolls: tracked params, bases, ranges, X/Y, invert, sources, bypass and chain order";
  DETACHED "FX that ROLL leaves alone: nothing is re-rolled and they keep their place in the chain. Drag an FX header
  here." (UI:4638-4643).
- **[+]** opens an in-column picker (UI:4737-4822): search input (hint "Search FX… (Enter adds the first)", focused),
  then a virtualized scrolling list (max 12 visible rows, row h `floor(bh·0.85)`): section headers FAVORITES, then
  ALL PLUGINS (empty query) or OTHER MATCHES; each row = star icon (click toggles favorite), name, type tag at right;
  click row = add to THIS pool (§3.9). Empty DB: "The FX list is empty — open the FX Browser once to scan."; "No match.".
- Lists: each FX group in its pool column, sorted by visible `fx_id` (chain order), spacing item_spacing (UI:4877-4881).

#### 2.4.2 FX group (`drawFXGroup`, UI:4314-4619)

Header strip (height `floor(bh·0.92)`, hover bg `button_hovered`), left→right:
- chevron triangle (▶ folded / ▼ open, muted), name (`fx_data.name`, truncated), count `"sel / total"` (muted),
- **5 verb icons** (each hh wide; whole band hidden if fewer than 90 px would remain for the name, UI:4130, 4395):

  | icon | tooltip | action |
  |---|---|---|
  | Maximize | Open the plugin window | toggle `TrackFX_Show` 3/2 |
  | Power | Bypass this FX | toggle enabled; icon painted in `bypass` color (amber) when bypassed |
  | Move | Roll X / Y on the tracked params | `randomizeXYAssign(fx)` |
  | Ruler | Roll ranges on the tracked params | `randomizeRanges(fx)` |
  | Gauge | Roll bases on the tracked params | `randomizeBaseValues(fx)` |

- **"…"** → FX menu (§2.8). Tooltip "Everything this FX can do".
- Header body: press records a pending press; on **release without >5 px move = fold/unfold** (`fx_collapsed[fx_id]`);
  **drag >5 px = move to the other pool** (ghost label follows the cursor, target column outlined in accent when it
  would change the pool) (UI:4504-4518, 4894-4952). Tooltip "<name>\nClick: fold · drag to DETACHED: ROLL leaves it
  alone" (or "… — out of ROLL.\nClick: fold · drag to the ROLL column to roll it again").
- Below the header (unfolded): optional **FX view graph band** for house JSFX declaring `// cp-view: graph …`
  (Full/Compact/Hidden) (UI:4133-4226) — out of scope.
- Rows: the params shown = all params (`fx_show_all[fxKey]` default **true**) or tracked only. **Two columns of rows**
  when `list_w ≥ bh·12·2 + 3·item_spacing`; column width `(list_w − gap)/2`; rows paired left/right in sorted
  param-index order (UI:4547-4611). Virtualized: only rows intersecting the clip are drawn. An open **link strip**
  (LinkStrip, the per-param list of sources) is inserted full-width under the pair of its param.
- Empty with "tracked only": caption "nothing tracked — the … menu shows every param".

#### 2.4.3 Parameter row (`drawParamRow`, UI:3485-4042)

```
[S] (knob)  Name……………………   0.42 Hz   [X][Y][N]  ● L3
```
Geometry (UI:3443-3561): `ROW_KNOB = 32`, `ROW_H = 38`; sel chip 10×10 at x+0 (`sel_w = 12`); knob at `x+15`;
name at `knob_x + 38`; source column `src_w = floor(bh·1.7)` at the right; 3 axis chips pitch `chip_w = 20`
(box 18×`CHIP_H 18`) left of it; value column `val_w = floor(bh·2.2)` (0 when Values off) left of the chips; name gets
the rest. Row card: tracked → `Card` tinted accent 10 %; row driven by the hovered/selected rack source → tinted with
that source color 26 % (UI:3647-3660).

| element | display | click |
|---|---|---|
| sel chip | on = tracked (`selected`) | toggle tracked; when turning on, `base_value = current_value`; `updateSelectedCount`, save |
| knob (ModKnob) | base arc + modulation ring + live dot (§6.2) | press = focus param as REAPER "last touched" (`pokeTouch`, for Map/inspector); gestures §2.9 |
| name | truncated | — |
| value | live value in plugin units: step_count 2 → "ON"/"OFF"; 3..5 → "k/n"; else REAPER `FormatParamValueNormalized` (fallback local shape/linear `%.4g`) | — |
| X chip | **on** (accent fill) = X assigned AND pad really writes it; **standby** (accent letter, neutral bg) = assigned but a source wins; off = not assigned | toggle X assign only (never releases a source) |
| Y chip | same for Y | toggle Y |
| N chip | amber fill = inverted | toggle invert |
| source column | 1 source: colored dot r3.5 + short label (`L1..8`, `G1..8`, `M1..8`, `E1..4`, `S1..2`, note short names), hollow/α0.6 when bypassed; ≥2 sources: one 2.5-px dot each, no label; none: muted "XY"/"X"/"Y"/"–"; red dot when armed for Capture | open/close the link strip (if a source exists) else open the modulation menu |

- Drop target for a dragged source dot = the **knob rect** only (accent ring r = 16+2 on hover) (UI:3888-3901).
- Tooltip (not while dragging): name, then `"<win_lo> -> <win_hi>"` in plugin units (the real reachable interval),
  depth text ("±18.2 st" style), source name or "(pad)" / "(no source - nothing drives it)", standby explanation,
  "[INVERTED]", then the gesture help lines (UI:3961-4040).
- `padDrives(pd, src)` = selected AND (pad_mode == 1 OR no source) — the chip "on" state (UI:3419-3423).

### 2.5 Right zone — source rack (summary only; UI:7671-8020, 5957-5994)

Header row: TabBar **Track | Global | Links** (`lfo_panel_mode` 1..3; "Links" replaced "Matrix", RM §21.7 phase 4),
**[Map]** toggle (Bitwig-style: dragging any knob sets the selected source's depth; not persisted; Esc disarms),
**[▶]** folds the rack (`zone_right_collapsed = true`, also disarms Map). Folded: a full-height rail button showing a
left triangle and the word **SOURCES** written vertically; click unfolds. Body: grid of source tiles (ENV 1..4, SEQ A/B,
then 8 bank slots), each with a draggable colored **pastille**; selecting a tile highlights the params it drives.

### 2.6 Info bar (UI:8288-8494), height 22, anchored to window bottom

Left: `"<selected_count> tracked · <links_count> links · <lfo_links_count> modulated"`; transient messages
(`UI._eng_msg`, accent color, 3–8 s) paint over it. Right → left:

1. "⚠ granular suspends native links" (amber `0.85,0.64,0.29`) when `pad_mode == 1`.
2. Capture status text (record color) when non-empty.
3. **Y receiver pill**, then **Y axis dial** to its left; then **X receiver pill**, **X dial** (pill w `floor(bh·3.4)`,
   h 16, fully rounded; dial size 16). Then muted label **"PAD ←"**.
4. Engine status (read-only): "engine: script (granular grid)" | "engine: native links (audio thread)" (accent) |
   "engine: script (no bridge)" (muted).
5. **"Stop all engines"** (hover text): `LE.stopAllEngines()` — stops walk/figures on EVERY track's bridge of the
   project, links stay; message "N engine(s) stopped" / "no engine running".
6. **"Release all links"**: `LE.releaseAllLinks()` on the current track (plugins keep their sounding value, bridge
   stays); message "N link(s) released" / "no link to release".

Receiver pill (`drawPadReceiver`, UI:8046-8163): label `"X <src>"` / `"X free"` in the source color (accent if no
moddrag; muted if free); **drop a source dot** → `pad_src_x = slot`, arm the source, `syncPadAxes`; **click a lit pill
= release** (`pad_src_x = 0`). Pad pastilles cannot be dropped on a pill. Tooltip distinguishes "is driven by" vs "is
assigned to …, but nothing drives it on this track".
Axis dial (`drawPadAxisKnob`, UI:8185-8286): ModKnob of the axis; free axis: drag sets `gesture_x` AND
`gesture_base_x` AND target together, then re-apply; driven axis: refuses the mouse, shows a full ring (depth 1) in the
source color centered on the source neutral. Its **pastille** (bottom-right, size `max(6, floor(size·0.45))`) is a drag
source `PAD_X = −1` / `PAD_Y = −2` (MD:48-53): dropped on a param knob it ADDS that axis (§3.4), on a slot setting or a
macro it drives it (RM §18.4).

### 2.7 Modals

**Settings** (UI:8570-8772; 440×620; [Close] at top outside the scroll; Esc closes it unless the filter field has
focus):

| section | control | state key | range / default |
|---|---|---|---|
| ROLL — HOW FAR | caption "How many params the P axis tracks"; `min` / `max` int sliders | `random_min` / `random_max` | 1..300, 3 / 8; min pushes max up, max ≥ min |
| | Bypass % | `random_bypass_percentage` | 0..100 %, 30 % |
| | Modulated % | `random_lfo_probability` | 0..100 %, 0 % |
| | ☐ Exclusive X / Y | `exclusive_xy` | true |
| | Range span (dual) | `range_min..range_max` | 0..1, 0..1 |
| | Base intensity | `randomize_intensity` | 0..1, 0.3 |
| | Base span (dual) | `randomize_min..randomize_max` | 0..1, 0..1 |
| | ☐ Also roll the generator's frequency | `ultra_random_settings.sound_frequency` | false |
| LFO RANDOMIZER | Slots (int) | `lfo_random_settings.slots` | 1..8, 8 |
| | ☐ Randomize Shape / Rate (free Hz) / Tempo Sync / Phase / Pad → LFO Rate (meta-links) | `.shape .rate .sync .phase .pad_meta` | on, on, off, off, off |
| ENGINE | Auto JSFX toggle | `auto_jsfx` | true (inserts the bridge on every visited track, deferred to mouse-up) |
| | caption "Tracks get the bridge AND the CP_Mod source bank automatically" | | (bank now created on first slot write, UI:8822-8825) |
| | [Show X / Y envelopes] | creates bridge if needed, `GetFXEnvelope(…,0/1,true)` | |
| | Slew | `link_slew` | 0..2 s, 0 → bridge slider 3 via `SetParamNormalized(slew/2)` (LE:2073-2079) |
| | ☐ Log link writes to the console | `debug_links` | |
| FILTERS | Param (text) | `param_filter` | substring filter, rescans the chain |

**FX browser fallback** (FXMUI, only if `CP_FXBrowser.lua` cannot be launched): modal "FX Browser" 760×520. Toolbar
`[Search FX…][Scan][Z-A/A-Z][Close]`; three columns 0.20/0.55/0.25: Categories (● marks selected) | Plugins (★/☆ toggle
favorite, click select, Ctrl+click multi-select) | Track FX Chain (`"n. name (bypassed)"` click opens window, [ON]/[off]
bypass, [X] delete; Sound Gen and protected FX hidden). Footer: "N plugins · N selected", ☐ Auto-open, ☐ Close after
add, [Cancel], [Add to Track] (adds selected or last clicked, `-1000-n` end insertion). "Random FX Insertion": FX count
1..10 (3), ☐ Favorites only, [Add Random] (§3.9).

### 2.8 Context menus (all native popup menus)

- **Param row right-click** (`openParamModMenu`, UI:2965-3211): header = param name (disabled). If sources: one line per
  source `"<name>   <depth text>"` (checked = active; click toggles bypass), "Remove source" ▸ (each, + "All
  sources"), "Modulate a depth" ▸, separator. Then: **Follow Pad X**, **Follow Pad Y**, **Follow Pad XY** (checked when
  no source and that exact assignment), **CP LFO** ▸ 1..8, **Global LFO** ▸ 1..8, **Macro** ▸ (macro names), **Note
  Envelope** ▸ ENV 1 (amp), ENV 2..4, **Sequencer** ▸ Mod A/B, **Note** ▸ 4 note sources; checked = already a source,
  click toggles (add / remove). Then **Param LFO (native)** ▸ Enable/Disable, Shape {Sine, Square, Saw L>R, Saw R>L,
  Triangle, Random}, Speed (Hz) {0.1, 0.25, 0.5, 1, 2, 4, 8}, Tempo sync {1/16→4, 1/8→2, 1/4→1, 1/2→0.5, 1 bar→0.25,
  2 bars→0.125}, Strength {10, 25, 50, 100 %} (REAPER's own per-param LFO, stacks on top). "Show the source rack".
  Capture arm entry ("Arm for capture" / "Disarm capture" / disabled "Arm capture — <why>").
  "Follow Pad …" = `followPadAxes`: release all sources, set selected, write X/Y assign directly (bypasses the
  exclusive auto-clear) (UI:2826-2845).
- **FX "…" menu** (`openFXMenu`, UI:4228-4312): name (disabled); ☐ Keep out of ROLL (detached); Show tracked only /
  Show every param; Graph ▸ Full/Compact/Hidden (house FX only); Track all; Track continuous only (step_count 0 or >10);
  Track none; Track at random; Random count ▸ 1..10 (`fx_random_max`, default 3); Roll X / Y here; Roll ranges here;
  Roll bases here; Open the plugin window. (No delete/reorder entry.)
- **Preset menu** (field click, UI:2068-2178): current name (disabled); **Init — start from an empty constellation…**
  (confirm box; §3.11); Save '<cur>' / Save…; Save as…; list of presets (checked = current; flat if ≤24, else "Load" ▸);
  Rename…; Delete… (confirm); Save grain set…; Load grain set ▸; Delete grain set ▸.
- **Snapshot menu** (UI:1998-2065): without preset: "A snapshot lives inside a preset" + "Save a new preset…". Else:
  "Snapshots of <preset>" (disabled); Capture as '<next>'; Capture as…; Overwrite '<cur>'; list (checked = current);
  Rename '<cur>'…; Delete '<cur>'; **Set A = now (set)**; **Set B = now (set)**; Clear A / B (morph captures).
- **Capture menu** (UI:716-738): "Capture to envelopes" (disabled); Pad X; Pad Y; Macros ▸ 8; "Keyboard notes → MIDI
  item"; Disarm all. Impossible entries are greyed with their reason, never hidden.
- **Macro right-click**: free macro → rename dialog; driven macro → its sources menu + "Rename…" (UI:586-608).
- **Pad right-click**: no menu; right-drag = zone, plain right-click = clear zone (§5.5).

### 2.9 Mouse gestures and modifiers (master table)

| target | gesture | effect | ref |
|---|---|---|---|
| pad | left press/drag | gesture start: `gesture_base = gesture_pos`, `captureBaseValues()`; if press within 0.10 (normalized) of the dot, keep a grab offset (dot doesn't jump) else dot jumps under cursor; stops walk/figures activity (mode unchanged); driven axes ignore the mouse (click shows a 4-s message) | UI:1200-1256 |
| pad | release | `gesture_active = false`, offsets 0, `saveTrackSelection` | UI:1257-1265 |
| pad | right-drag | rubber band; on release, if `|Δx| > 0.03 and |Δy| > 0.03` → `setZone`, else `clearZone`; then re-apply | UI:1153-1198 |
| param knob | drag | base (sensitivity 0.004/px; **Shift = fine ×0.1**, re-anchored when Shift toggles) | WID:3241ff (`sensitivity`, l.+18, +157, +185) |
| param knob | **Alt+drag** | depth (signed: crossing zero = invert); `range = |depth|/gesture_range` clamped 0..1, invert = depth<0 | UI:3762-3777, WID (Alt/Shift decided at release, 2026-10-10) |
| param knob | wheel | base ±0.02/notch (**Ctrl = ×0.25**) | WID wheel block |
| param knob | Alt+wheel | depth, extends lo/hi around the source neutral | WID wheel block |
| param knob | double-click (no modifier) | base := `scan_value` (value at chain scan) | UI:3519-3525, WID |
| param knob | Alt+click (no move) | remove ALL modulation sources of the param (only if it has one) | UI:3748-3750 |
| param knob | Shift+click | bypass / un-bypass all its sources | UI:3751-3753 |
| param knob | drop a source dot | add source (max 4 total, via sum bus); drop a pad pastille: ADD that axis (Y + X → XY) | UI:2645-2664, 2852-2867 |
| param row | right-click anywhere | modulation menu | UI:3905-3911 |
| FX header | click / drag | fold (on release) / change pool | §2.4.2 |
| ROLL chip | left / right click | arm / roll that axis now | UI:1756-1774 |
| macro knob | drag / right-click | value / rename (driven: base, Alt+drag depth, Shift+click bypass, Alt+click release) | UI:532-619 |
| any pastille | press-drag-release | dashed wire (2 px, dash 7/5) in the source color, 4-px dot at start, 8-px ring at cursor, label box follows; first target containing the point on release consumes it | MD:191-336 |
| seam | drag / double-click | resize / reset width | UI:9332-9392 |

Wording conflict: UI:3499-3503 says "ALT+GLISSER N'ECRIT PLUS LA PORTEE" (Alt+wheel only) — **outdated**; UI:3733-3737,
the tooltip UI:4031 and WID (comment "Alt et Shift se décident au relâchement (2026-10-10) … je préfère vraiment
Alt+drag") say **Alt+drag = depth**. Code wins: Alt+drag.

### 2.10 Keyboard

FX Constellation has **no shortcuts of its own**. The only key handled is **Esc**: closes Settings (UI:8581-8586);
otherwise closes the open link strip, else disarms Map (UI:9223-9235); else the toolkit's close-on-Esc applies. Keys the
window does not consume are passed back to REAPER when the window is hosted (CLAUDE.md H, l.303-322: "FX Constellation
n'a AUCUN raccourci à lui (un seul test de touche : Échap)"). `Keyboard.lua` is the on-screen piano, not shortcuts.

### 2.11 Track following, Lock, goto (UI:8929-9076)

- Follows `GetSelectedTrack(0,0)`; **auto-locks to the first track seen** after opening (`UI._auto_locked`), the Lock
  button unlocks/relocks manually. Lock only holds while the track is in the ACTIVE project tab (`ValidatePtr2(0,…)`);
  otherwise unlock and follow the new project's selection.
- Goto request: ExtState `CP_FXConstellation/goto = "<time_precise>|<track GUID>"`, consumed once, refused if older than
  5 s (sent by CP_Session's strip chip). Temporarily unlocks, switches, relocks on the requested track (UI:8914-8961).
- On a track change: flush the outgoing track's selection; cancel a pastille drag; keyboard panic; Record.TrackChanged;
  `track = new`; `_eng_adopt = true`; `clearZone()`; `scanTrackFX()`; `equip_pending = true` (bridge insertion
  deferred until the mouse is up and no Capture pass runs); `jsfx_automation_enabled = auto_jsfx and bridge present`;
  `jsfx_has_env`; `syncBridgeState`; `links_dirty` (UI:8957-9037).
- There is **no FX-to-FX navigation** besides folding and scrolling the list.

### 2.12 Mockups vs shipped (CD-A, CD-B, CD-C; RM §3.1, §11.2, §18.3)

| aspect | v5 mockup (CD-C, the refonte target) | earlier mockups (CD-A/B) | shipped (Nov) |
|---|---|---|---|
| overall | command bar / 3 zones (left 340, center 1fr, right 312, + 12-px "◀ REPLIER" strip) / info bar, ~1650×700, 2-px seams (1 px #0c0d0e + 1 px #ffffff12) | A: one 1140-px window, top row pad 298 · NAVIGATION 180 · MODE+RANDOMIZER 180 · PRESETS+SNAPSHOTS 180, bottom "FX SETTINGS" horizontal FX cards; B: dock 1700×300 of 7 reorderable collapsible sections / window 1100×720 | frame as v5; widths 14.5·bh / flex / 12.5·bh |
| command bar | "FX Constellation · Track 2 — 6 FX", Lock, Single\|Granular, Manual\|Walk\|Figures, 8 macro knobs (36 px, name under, violet grab dot on top edge), ?, S | — | title, track name, **preset & snapshot groups (VST-style)**, Record, Capture, Lock, the two segments, A→B; macros are NOT here |
| left zone | pad (range rect dashed inset, crosshair, anchor ghost ring, 3-dot trail, glow, cursor #8ff0d8), foot "0.96, 0.35 … ancre 0.50, 0.50 … Reset", ROLL (gradient #1a6f5d→#12564a, "7 axes armés") + UNDO + A/B, 8 axis cells with LEDs, "BASES" int field + two-handle slider, report line, preset card with 4×2 snapshot tiles | A: pad 298² with granular circles r = one cell | pad (crosshair, zone veil, anchor ring, glow dot, smooth target ring; **no trail, no dashed range rect** — the gesture_min/max frame was deliberately removed UI:1113-1130), foot, nav detail, ROLL + 8 chips (no UNDO / A/B buttons), **macros 2×4** (moved here 2026-10-04); snapshots moved to the bar |
| center | tabs Paramètres / Sound Gen; FX group bands; **two-column rows** `18 lock │ 30 knob │ name │ 66 value │ 54 X Y N │ 64 source chip`; lock chip (amber) per row | A/B: horizontal cards with checkbox, X/Y/N chips, range bar | tabs Parameters / Sound Gen / **Seq**; **ROLL/DETACHED pools**; two-column rows; **no per-row lock**; keyboard under the tabs |
| right | SOURCES rack (slots LFO/ENV/FOLLOW/S&H/STEPS, mini-curves, dest counts, "+ ajouter une source") | — | rack with Track/Global/Links tabs + Map; tile grid (RM §20.1) |
| info bar | counts, engine text, capsule "PAD ←" with X/Y round drop targets, Granular warning | — | same + per-axis dials with pastilles, Stop all engines, Release all links, capture status |
| colors | teal accent #13bd99 on tinted greys | A/B: default_dark #5999d9 / teal #7fbcb1, X blue #7099d9, Y red #d97070 | Constellation theme (CFG) = v5 palette; X/Y chips both accent |

RM §11.2 lists the remaining gaps at the time (rounded hand-drawn shapes, macro look, tile grid); (f) and (g) of §11.5
closed most of them (drawing grammar Card/Strip/Sunk/Chip in the toolkit, two columns, bands, pad outline, snapshot grid);
the snapshot grid later left with the macros' move (RM §18.3).

---

## 3. THE PARAMETER MODEL

### 3.1 Chain scan (`FXM.scanTrackFX`, FXM:234-459)

- Walk the track chain; `fx_data[visible_fx_id]` (0-based, consecutive among LISTED FX) with `actual_fx_id` = raw chain
  index. Not listed: names containing "FX Constellation Bridge" (remembered as `jsfx_automation_index`), "Sound
  Generator", "CP_Mod" (first "CP_Mod LFO" → `modlfo_index`), and **protected** FX by `fx_ident`/`fx_name`/display name
  substring: `reasamplomatic`, `cp_kit_choke`, `cp_kitsampler`, `cp kit sampler`, `fx constellation - sequencer`,
  `fx constellation - mod mix`, `fx constellation - notes` (CORE:393-419, 515-537).
- The Sound Generator gets an `fx_data` entry **after** the loop with `sg = true` (hidden from the list, but a
  modulation destination; params from `SoundGenerator.MOD_P`, `selected = false` always, no step probe) (FXM:340-428).
- FX entry: `name` = `extractFXName` (strip `"TYPE: "` prefix, remove `(...)`, trim, >25 chars → 22 + "...")
  (CORE:373-377), `full_name`, `enabled`, `actual_fx_id`, `guid`, `detached` (pool membership), `params`, opt. `sg`.
- Param filter (`shouldFilterParam`, FXM:22-39): exclude names containing "(meter)"; exclude any `filter_keywords`
  match; if `param_filter ~= ""` keep only names containing it (case-insensitive).
- Step detection (`detectParamSteps`, FXM:46-63): `TrackFX_GetParameterStepSizes`; toggle → 2; else `count =
  round((max−min)/step)+1`, kept if 2..10, else 0 (continuous).
- After the scan: `loadTrackSelection()`, `updateSelectedCount()` (also bumps `sel_gen`), `reconcileModSources()`,
  `links_dirty = true`.
- Change polling (`checkForFXChanges`, FXM:471-587): every 50 ms (`update_interval`): FX count change → rescan;
  every 0.25 s: name+param-count signature or pool string change → rescan; enabled flags each tick; selected params
  (≤128) re-read every tick; others round-robin 64 per tick (an untracked, unsourced param's base follows the plugin).

### 3.2 Per-param fields, keys, value spaces (FXM:296-320, CORE:336-358)

| field | meaning | space |
|---|---|---|
| `name` | REAPER param name | — |
| `current_value` | last read value (modulated readback) | norm |
| `base_value` | live baseline (pad rewrites it every applied frame in script mode) | norm |
| `scan_value` | value at scan time; double-click reset target | norm |
| `min_val`, `max_val` | `TrackFX_GetParamEx` raw range (SG shaped params: from generator table) | raw |
| `unit` | suffix in "(…)" only if in whitelist {Hz,kHz,s,ms,dB,st,ct,cents,%} and range ≠ 0..1 | — |
| `shape` | measured slider shape (`log`/`sqr`/`curve`/nil) | — |
| `selected` | **tracked** — gates pad, randomizers AND modulation (assigning a source auto-selects) | bool |
| `step_count` | 0 continuous, 2 toggle, 3..10 discrete | int |
| `fx_id`, `param_id`, `actual_fx_id` | ids | — |
| `key` | `"<trackGUID>_<fx full_name>||<param name>"`; cached `key_range/_invert/_x/_y` = key .. suffix | — |

State tables keyed by those keys (CORE:8-160):
- `param_ranges[key_range]` → range, default **1.0** (`FXM.getParamRange`, FXM:633-636).
- `param_invert[key_invert]` → bool, default false.
- `param_xy_assign[key_x]`, `[key_y]` → bool; **read as `~= false`, i.e. absent = true** (FXM:667-672).
- `param_base_values[key]` → the **anchor** base the pad offsets against (norm).
- `param_mod_source[key]` → first source slot (0/nil = pad), `param_mod_more` (≤3 more `{slot, depth}`),
  `param_mod_pol` (per-link "u"/"b"), `param_mod_off` (bypassed sources).
- Exclusive axis: **global** pref `exclusive_xy` (default true). `setParamXYAssign(axis, true)` clears the other axis
  when exclusive (FXM:674-686). Reads never enforce it.
- Source slot encoding (LE:117-131, 173-219): 0 pad; 1..8 local CP_Mod LFO; 101..108 global LFO (hidden "CP MOD" track,
  14-bit CC); 201..204 Sound Gen envelopes (ENV 1 = amp env); 211..218 macros; 221..222 sequencer Mod A/B; 231..234
  note sources (NOTE, VEL, RAND, OCT). Test order is the contract: note > seq > macro > env > global > lfo.
  Pastille-only pseudo-slots: −1 = pad X, −2 = pad Y (MD:48-53).

### 3.3 Tracking defaults (the subtle part)

- Scan sets `selected = exclusive_xy` and, if `exclusive_xy`, seeds axes by parity: X if `(fx_id + param) % 2 == 0`
  else Y (FXM:315, 326-334).
- `loadTrackSelection` then (FXM:1128-1377):
  - **track never saved** → deselects every param, `captureBaseValues()`, pad to center (0.5, 0.5) on arrival; parity
    axes remain;
  - **saved track** → `selected = selection[key] or false`; `base_value = base_values[key] or current_value` (and
    seeds `param_base_values` for tracked params); range = saved or 1.0; invert; **axes = saved pair, else BOTH true**;
    mod sources restored; on arrival (new track) restores `gesture_base_x/y`, `gesture_x/y`, target.
  - First load of an entry without `native = 1` → `links_reanchor` (audible value becomes the base; one-time migration).
  - ⚠ The default axis therefore depends on the track's history (AN §1.5); RM §11.4 notes "`xy_assign` true on both
    axes by default" made X/Y/N look lit on almost every row.
- `selectAllContinuousParams`: selects only step_count 0 or >10 (FXM:617-623).

### 3.4 How the pad moves a param

Definitions: `pos = (gesture_x, gesture_y)`, `anchor = (gesture_base_x, gesture_base_y)`, `B` = anchor base
(`param_base_values[key]`, seeded once from `base_value` if missing), `R` = range, `G` = `gesture_range` (default 1,
**no UI writer** — AN §1.4; part of PAD_FIELDS so presets carry it), `s = −1` if inverted else +1.

**Script path** (`GS.applyGestureToSelection`, GS:14-79; used when `links_active` is false, i.e. Granular, no bridge,
or Auto JSFX off):
```
ox = (gx − gbx)·2 ; oy = (gy − gby)·2                       -- ±1 for a full half-course from a centered anchor
(up, down) = calculateAsymmetricRange(B, R, G, gmin, gmax)  -- CORE:648-660
    maxr = R·G·0.5 ; up = min(maxr, gmax−B) ; down = min(maxr, B−gmin)
    if up < maxr:   down = min(down + (maxr−up), B−gmin)    -- headroom redistributed
    elif down < maxr: up = min(up + (maxr−down), gmax−B)
cX = (s·ox > 0) ? s·ox·up : s·ox·down   (only if X assigned; same for Y)
v  = B + cX (X only) | B + cY (Y only) | B + (cX + cY)/2 (XY) | B (none)
v  = clamp(v, gesture_min, gesture_max) ; if step_count>0: v = round(v/(1/(n−1)))·(1/(n−1))
write: param has a source → LE.setBaseline(v) (link baseline) else setParamNorm(v)
current_value = base_value = v   (param_base_values unchanged = anchor)
```
Without clamping this is `v = B + s·(pos − anchor)·R·G` (average of the two axes for XY).

**Native path** (`LE.syncLinks`, LE:2889-3343; `want_links = (pad_mode == 0)` and a bridge with ≥35 params): every
**selected** param with **no source** and at least one axis gets a plink:
- source = bridge param 3 `x_out` (X only), 4 `y_out` (Y only), 5 `xy_mix` (both) (LE:51, 3080-3093);
- `offset = −gbx | −gby | −(gbx+gby)/2`, `scale = R·G·s` (`modSpanEff`, LE:1037-1080), `baseline = B` (anchor);
- REAPER computes `value = baseline + (src + offset)·scale`, clamped to the param's own 0..1 (LE:3150-3204).
- Not available natively: asymmetric redistribution, `gesture_min/max`, step snapping (LE:25-34).
- Offsets are **sticky**: written when the link is cabled, on re-anchor or rebuild only (LE:2182-2187). Re-grabbing the
  pad in script mode re-anchors with `B := current value`, which is value-continuous, so both paths agree unclamped.
  ⚠ AN §1.10 (Aug) reported that display (`getLiveValue`, centered on the *current* anchor) and audio (old offset) can
  disagree after a re-grab under native links; whether Nov fixed it is unverified (?).
- A param with a source AND an axis: in native mode the pad becomes an extra input of its sum bus (ModMix) if room
  (4 inputs/bus, 16 buses/track), else the axis is "standing by" (chip standby state) (RM §21.7 "Le pad et des
  sources…", LE:2175-2225). In Granular the grid writes the baseline under the source.
- `captureBaseValues()` (FXM:688-733): re-anchor at gesture start, nav start, ROLL, presets: `gesture_base = pos`;
  for each selected param: if it has a source or one of its assigned axes is **driven by a source**
  (`pad_link_x/y`), keep `base_value` as anchor; else `base_value = current_value` and anchor = it. Skipped entirely
  when `links_active` (native mode: anchors live in link baselines).
- Editing the base from the knob: `updateParamBaseValue` → `base_value` and anchor := v; linked → `setBaseline`, else
  `setParamNorm`; save (FXM:735-757). Editing range/invert → `pushDepth` (rewrites the link scale) (FXM:638-665).
- Release of a link returns the param to its BASE (decided by Cedric 2026-08-22: "un modèle stable qui clique un peu"),
  RM §14.2 rule 6, LE:3214ff.

### 3.5 "Portée" (range) semantics

- `range ∈ [0,1]` = fraction of the param's normalized course covered when the pad goes from one edge to the other
  (with G = 1). From a centered anchor the param moves ±R/2 around its base.
- The knob draws the **real reachable window** `win_lo..win_hi = modWindow01(base, depth, center)` with `depth = R·G·s`
  and `center` = pad anchor component (pad) or the link's neutral (0.5 bipolar LFO, 0 envelope/macro, per-link polarity
  override): `lo = base + (0−c)·depth`, `hi = base + (1−c)·depth`, clamped 0..1 (UI:2360-2411, LE:3416-3424).
- `gesture_min/max` ("Range" slider in nav detail) clamp values in the **script** path only.
- Randomizer "Range span" draws new ranges uniformly in `[range_min, range_max]`.
- Alt+drag on a knob sets `range = |depth|/G` (clamped 0..1) and invert = sign (UI:3765-3777).

### 3.6 Per-track persistence (`flushTrackSelection`, FXM:846-972)

`track_selections[trackGUID] = { selection, ranges (≠1 only), xy_assign (only if not both true), invert_assign,
fx_random_max (by full_name), base_values (ANCHOR for tracked params), gesture_base_x/y, gesture_x/y, current_preset,
native = 1, norm = 2, sg_shaped, mod_sources, mod_more ("211:0.5000;103:-0.2500"), mod_pol ("211:u;103:b"), mod_off
("211;103"), macro_names, pad = capturePad(), morph_a, morph_b }`, inner keys `"<full_name>||<param name>"`.
Stored via ExtState `CP_FXConstellation/track_selections` (re-read on scan when no save is pending) and a .dat mirror;
`saveTrackSelection` only marks dirty, the flush builds once per frame when the mouse is up (FXM:806-845).

### 3.7 Pools ROLL / DETACHED (FXM:126-232, RM §18.1, UI:4621-4952)

- Membership = set of **FX GUIDs**, stored in the track's `P_EXT:CP_FXC_detached` as space-separated sorted GUIDs
  (only GUIDs still on the track are kept). Shared with the FX Browser (same key, `FXManager.POOL_EXT`).
- A DETACHED FX is touched by **none** of the 8 ROLL axes: no param tracking/bases/ranges/X-Y/invert/sources rolls, no
  random bypass, and it **keeps its chain position** in random order. Explicit gestures on it remain allowed.
- Detaching (`setFXDetached(fx_id, true)`) **releases what ROLL put there**: for every param, release its source link
  and set source 0, and untrack it (`selected = false`); values stay where they are (FXM:201-232). Same cleanup when
  another process (FX Browser) detaches it (FXM:507-527). `pool_gen` increments.
- Sound Gen cannot be detached (not listed).

### 3.8 FX chain management

| action | where | behavior |
|---|---|---|
| add to ROLL pool | [+] picker of ROLL column | `addFXToPool(name, false)`: insert right after the last ROLL FX (i.e. before detached ones), or at the first detached index, or at end; window not opened (`TrackFX_Show(…,2)`); undo "Add FX: <name>" (FXM:1902-1937) |
| add to DETACHED | [+] of DETACHED | insert at end, mark detached |
| add (browser) | Add FX… → CP_FXBrowser (standalone) / FXMUI modal | FXMUI: `addFXByName(name, auto_open, true)` at end (`-1000 − count`) (FXM:1861-1890) |
| add random | FXMUI "Add Random" | without replacement, instruments excluded, favorites-only option, each via `addFXToPool(…, false)` (FXM:1939-1979) |
| bypass | header Power icon; FXMUI ON/off; ROLL BY | `TrackFX_SetEnabled` |
| remove | FXMUI chain list [X] only | `TrackFX_Delete` |
| reorder | **only** via ROLL "OR" (random permutation) | §4 |
| open window | header Maximize / "…" | `TrackFX_Show` 3/2 toggle |
| fold | header click | `fx_collapsed[visible fx_id]` (session only, not persisted, keyed by visible index (?)) |
| show all / tracked | "…" | `fx_show_all[getFXKey(fx_id)]`, default true, persisted |

The bridge, CP_Mod bank, Sound Gen, sequencer, sum bus and notes FX are inserted "quietly" (no floating window) and are
never listed/rolled/moved by presets (CORE:539-554).

### 3.9 Macros (GS:1409-1481, UI:457-651, BR sliders 7-22)

- 8 macros; **values live only on the bridge** (`macro1..8` = sliders 7..14, param idx 6..13, raw 0..1); the slewed
  outputs `macroN_out` (sliders 15..22, idx 14..21) are the link sources for slots 211..218. Names are script state
  (`macro_names[n]`, default "MACRO n"), stored per track and in presets/snapshots.
- UI: 2 rows × 4 cells (`cell_w = floor(w/4)`, knob `ks = clamp(cell_w − 16, 18, 34)`, cell h `ks + 21`); name centered
  UNDER the knob (caption, muted, truncated); **pastille** (8 px, macro source color) overlapping the top-right corner;
  red armed dot top-left when Capture-armed; values re-read from the bridge at 10 Hz when no mouse button is down.
  Free macro: plain Knob 0..1 writes `setMacro`. Driven macro (a source links to it): ModKnob on the link baseline.
  Tooltip: "<name>  NN %\nRight-click: rename · drag the dot onto a knob to modulate it\nDrop a source dot here to drive
  this macro".

### 3.10 Init (PS:1825-1948, RM §18.3, UI:2112-2123)

Confirm box text: "Init clears this track's ROLL chain, releases every FX Constellation link, untracks all params and
resets the Sound Gen, the modulators, the macros and the pad. DETACHED FX stay where they are. Undo restores
everything." One undo block "FX Constellation: Init", track muted during. Steps: release all links and internal links;
clear all sources; untrack all, range 1.0, invert off; clear anchors; delete every non-kept, non-detached FX (keeps
bridge/bank/SG/seq/etc.); Sound Gen to its birth patch; sequencer reset; bank slots to birth (slot 1 on, sine, 1 Hz);
`lfo_meta = {}`; `applyPad(nil)` (pad defaults); morph cleared; pos/anchor/target 0.5; zone cleared; `pad_src_x/y = 0`;
macros 0, names cleared; rescan; `syncPadAxes`; preset name "" ; not a saved preset.

### 3.11 Presets / snapshots / morph (brief)

- Snapshot (`PS.saveSnapshot`, PS:534-612) = pos, anchor, fx_list, bypass states, and for each param that is tracked OR
  sourced: anchor base, range, x/y assign, invert, selected, mod_source/more/pol/off, depth mods; plus `pad =
  capturePad()`, macros + names, LFO state. Lives under `presets[current_loaded_preset].snapshots` (requires a preset).
- Granular set = `{grid_size, grains[{x, y, param_values[fx_id][param_id]}]}` under the preset (PS:53-112) — keyed by
  visible ids (fragile across chain edits (?)).
- `preset_modified` = chain-state comparison at 2 Hz against a baseline taken on arrival (PS:815-857); shown as the
  amber dot; never written into the name (RM §2.1 fix).
- Morph A/B (GS:1836-1885): capture = `{full_name → {enabled, params{name → current_value}}}` of selected params;
  morph = linear `a·(1−t) + b·t` per param present in both; writes value or link baseline; sets base and anchor.

---

## 4. THE RANDOMIZER ("ROLL")

### 4.1 Axes (UI:1693-1710) and defaults (CORE:303-313)

| chip | key | default armed | single-axis action (right-click) | in ROLL |
|---|---|---|---|---|
| P | `params` | **off** | `globalRandomSelect()` | 1st |
| B | `bases` | on | `randomizeAllBases()` | 5th |
| R | `ranges` | on | `globalRandomRanges()` | 6th |
| XY | `xy_assignments` | on | `globalRandomXYAssign()` | 3rd |
| N | `invert` | on | `globalRandomInvert()` | 4th |
| BY | `bypass` | on | `randomBypassFX()` | 7th (after batch close) |
| OR | `fx_order` | on | `randomizeFXOrder()` | 8th |
| MO | `mod_sources` | **off** | `globalRandomLFOAssign()` | 2nd (only if `random_lfo_probability > 0`) |
| — | `sound_frequency` | off (Settings checkbox) | — | 9th |

ROLL button → `FXManager.ultraRandom()` then `GS.updateJSFXFromGesture()` (UI:1732-1738). Order and batching
(FXM:1652-1725): `beginBatch` → P → MO → XY → N → B → R → `endBatch` (one selection rebuild; MUST close before the
chain-level rolls, which rescan) → BY → OR → SG frequency → `captureBaseValues()` (re-anchor). P runs first because
every other axis acts on tracked params. MO runs before XY so XY only distributes what stays on the pad.

Global exclusions for param-level rolls: FX whose `full_name` contains "Sound Generator" and **detached** FX. Chain-level
rolls also skip the bridge, "CP_Mod", protected FX and detached FX.

### 4.2 Algorithms (FXM line refs)

- **P — globalRandomSelect** (FXM:1606-1650): untrack every non-SG, non-detached FX; pool = ALL params of those FX
  (stepped ones included); `hi = max(1, min(random_max, #pool))`, `lo = max(1, min(random_min, hi))`;
  `count = random int [lo, hi]`; draw `count` params **without replacement**; `selected = true`; re-anchor; save.
- **MO — globalRandomLFOAssign** (FXM:1478-1548), p = `random_lfo_probability`: clears `lfo_meta`. If p > 0: ensure the
  track's LOCAL CP_Mod bank; `n = clamp(lfo_random_settings.slots, 1, 8)`; for i = 1..n `ModJSFX.randomizeSlot(i,
  cfg)`:
  `on = true`; shape `random(0,5)` if cfg.shape; rate log-uniform `exp(U(ln 0.05, ln 10))` Hz and sync 0 if cfg.rate;
  if rate AND sync: 50 % free random rate / 50 % `sync = random(1,6)`; sync only: `sync = random(0,6)`; phase `U(0,1)`
  if cfg.phase (M `CP_Engine/ModJSFX.lua` Aug version; Nov version not recovered (?)). If `cfg.pad_meta` and
  `rand < p` and the slot is free-running: add meta `{bank="local", slot=i, axis∈{x,y,xy} uniform, invert = rand<0.5,
  base_rate = slot.rate}`. Then for each selected param (non-SG, non-detached): `rand < p` → set FIRST source to a
  random slot 1..n; else if it has a source → release link, source 0 (back to the pad).
  Meta application (LE:2026-2069, script-side, only when the pad moved): `off = (g − g_base)·2` per axis (xy =
  average), negated if invert; `rate = clamp(base_rate·2^(2·off), 0.01, 20)` (±2 octaves at full course).
- **XY — randomizeXYAssign** per FX (FXM:1455-1469, 1550-1567): for each selected param WITHOUT a mod source:
  `exclusive_xy` → X if `rand < 0.5` else Y (exclusively); not exclusive → thirds: X only / Y only / both.
- **N — globalRandomInvert** (FXM:1569-1579): each selected param `invert = rand < 0.5` (sources not excluded).
- **B — randomizeAllBases** (FXM:1422-1452): `c = (randomize_min + randomize_max)/2`, `h = (max − min)/2`;
  `base = clamp(c + U(−1,1)·h·randomize_intensity, min, max)`; written to `base_value`, anchor, plugin (`setParamNorm`)
  and link baseline if linked; undo block "Randomize all bases"; re-anchor. Defaults → bases uniform in [0.35, 0.65].
  ⚠ Centered on the SPAN center, not on the current base (no random walk across rolls).
- **R — globalRandomRanges** (FXM:1589-1604): each selected param `range = U(range_min, range_max)` via `setParamRange`
  (→ `pushDepth`).
- **BY — randomBypassFX** (FXM:1813-1833): for each eligible raw FX `enabled = not (rand < random_bypass_percentage)`
  (default 0.3); undo "Random bypass FX"; rescan.
- **OR — randomizeFXOrder** (FXM:1727-1811): mobile slots = raw indices of eligible FX (not bridge/SG/CP_Mod/protected/
  detached); Fisher–Yates on their GUIDs; applied left to right: for slot k, find the GUID's current index `cur`, if
  `cur > dest` → `swapFX(dest, cur)` = `CopyToTrack(q→p, move)` then if `q − p > 1` `CopyToTrack(p+1→q, move)`; fixed
  FX never receive a move; track muted during; undo "Randomize FX order"; rescan.
- **Sound frequency** (FXM:1709-1722): if SG enabled, each ON oscillator `freq = exp(U(ln 20, ln 20000))`, push.
- Per-FX verbs (header icons / "…" menu): `randomizeXYAssign(fx)`, `randomizeRanges(fx)`, **`randomizeBaseValues(fx)`
  = `base = U(0,1)` for each selected param** (FXM:1399-1420 — ignores intensity and span, inconsistent with B (?)),
  "Track at random" `randomSelectParams(fx)`: untrack all of the FX, `count = random(1, min(fx_random_max[fx] or 3,
  #params))` without replacement (FXM:1379-1397).
- Granular "Randomize grains": §5.6. Seeds: plain `math.random` (no seed control); `GS.randomizeSelection` (seeded by
  time, asymmetric variation around bases) exists but has **no caller** (dead) (GS:81-106).
- Pre-refonte UI (M `6cf79c5` UI.lua:1176-1262, CD-A/B): ULTRA RANDOM, FX Order, Bypass + %, XY + exclusive checkbox,
  N, LFO + %, Ranges + span, Bases + Int + span, Parameters + min/max — 11 buttons and 6 sliders, now 1 button + 8 chips
  + Settings (RM §10.4).

---

## 5. THE XY PAD

### 5.1 State and `GestureSystem.PAD_FIELDS` (GS:1296-1303, CORE defaults)

`PAD_FIELDS` (exact order): `navigation_mode, random_walk_speed, random_walk_jitter, random_walk_jump,
random_walk_sync, figures_mode, figures_speed, figures_size, pad_zone_x0, pad_zone_x1, pad_zone_y0, pad_zone_y1,
gesture_range, gesture_min, gesture_max, link_slew, smooth_speed, max_gesture_speed, pad_src_x, pad_src_y, pad_mode,
granular_grid_size`.

| field | default | UI range |
|---|---|---|
| navigation_mode | 0 (Manual) | 0 Manual, 1 Walk, 2 Figures |
| random_walk_speed | 2.0 Hz | 0.1..10 (engine ≤20) |
| random_walk_jitter | 0.2 | 0..1 |
| random_walk_jump | false | bool |
| random_walk_sync | 0 | 0 Free, 1..5 = 1/16,1/8,1/4,1/2,1 bar |
| figures_mode | 0 | 0..5 Circle, Square, Triangle, Diamond, Z, Infinity (CORE:6) |
| figures_speed | 1.0 Hz | 0.01..10 |
| figures_size | 0.5 | 0.1..1 |
| pad_zone_* | 0,1,0,1 | 0..1 |
| gesture_range | 1.0 | no UI |
| gesture_min / max | 0 / 1 | "Range" dual slider |
| link_slew | 0 s | 0..2 (Settings "Slew") |
| smooth_speed | 0 | 0..1 |
| max_gesture_speed | 2.0 | 0.1..10 |
| pad_src_x / pad_src_y | 0 | source slot |
| pad_mode | 0 Single | 0/1 Granular |
| granular_grid_size | 3 | 2, 3, 4 |

`capturePad()` (GS:1323-1333) adds `nav_on` (activity), `lfo_meta`, and `grains` only when `pad_mode == 1`.
`applyPad(t)` (GS:1356-1379): missing field → DEFAULT (never the previous value); re-arms activity from `nav_on`;
restores metas/grains; invalidates the engine cache. Per track in `track_selections[guid].pad`, in every preset and
snapshot; a never-configured track gets Core defaults (`snapshotPadDefaults` at init). `padChanged()` compares the 22
fields each frame to trigger persistence. Session-only: `gesture_x/y`, `gesture_base_x/y`, `target_gesture_x/y`,
`random_walk_active`, `figures_active`, `gesture_active`, `pad_link_x/y` (fact published by `syncPadAxes`).

### 5.2 Manual + Smooth (GS:1805-1832, UI:1242-1256, 1346-1360)

`smooth_speed == 0`: drag writes `gesture_x/y` directly. `> 0`: drag writes `target_gesture_x/y`; each frame:
`d = target − pos`; if `|d| > 0.001`: limit `|d|` to `max_gesture_speed·dt`, then `pos += d·smooth_speed`; write bridge;
apply. A hollow target ring (r = dot_r − 1, muted α0.6) follows the mouse. Runs in Lua at frame rate (not in the bridge).

### 5.3 Random Walk (bridge @block BR:59-124; Lua fallback GS:420-444, 1612-1660, 1727-1786)

Per segment (when due):
```
p0 = clamp_zone(current) ; p3 = (zx0 + U·zw, zy0 + U·zh)
p1 = clamp_zone(p0 + 0.3·(p3−p0) + (U−0.5)·0.2·(zw, zh))
p2 = clamp_zone(p0 + 0.7·(p3−p0) + (U−0.5)·0.2·(zw, zh))
Free:   dur = max(0.05, 1/rate + U(−1,1)·(1/rate)·jitter)
Synced: qn ∈ {0.25,0.5,1,2,4}; dur = qn·60/tempo (120 if unknown); while playing, due on each new
        slot floor(beat_position/qn); stopped: due when t ≥ dur
Jump:   pos := p3 at segment start (teleport, no travel)
else:   u = min(1, t/dur); pos = (1−u)^3 p0 + 3(1−u)^2 u p1 + 3(1−u) u^2 p2 + u^3 p3
```
Also due when the zone changes. A rate change mid-segment (Free) rescales `t` and `dur` by `old/new` (no jump). Lua
fallback (no engine band): same geometry; when synced it uses `Master_GetTempo` intervals and ignores jitter for travel;
Jump while playing is grid-locked via `TimeMap2_timeToQN(GetPlayPosition())`.

### 5.4 Figures (BR:125-163; Lua GS:317-409)

Phase `φ = frac(Σ dt·rate)` (Lua: `figures_time·speed`), `a = 2πφ`, `h = size/2`, computed in the unit square (y up)
then inscribed in the zone `x = zx0 + clamp01(x)·zw` (centered, size relative to the zone):
- 0 Circle: `(0.5 + h·cos a, 0.5 + h·sin a)` (counter-clockwise from the right).
- 1 Square (k = 4·frac within quarter): bottom edge L→R `(0.5−h+k·size, 0.5−h)`, right edge up, top edge R→L, left edge
  down.
- 2 Triangle (breakpoints 0.33/0.66, k = 3·…): `(0.5, 0.5−h+k·h)`, then `(0.5−k·size, 0.5+h)`, then
  `(0.5−h+k·size, 0.5+h−k·h)` — ⚠ **as coded this path is discontinuous** (jumps (0.5,0.5)→(0.5,0.5+h) and
  (0.5−size,0.5+h)→(0.5−h,0.5+h)); identical in Lua and JSFX; probably a bug (?).
- 3 Diamond: left→top→right→bottom: `(0.5−h+k·h, 0.5+k·h)`, `(0.5+k·h, 0.5+h−k·h)`, `(0.5+h−k·h, 0.5−k·h)`,
  `(0.5−k·h, 0.5−h+k·h)`.
- 4 "Z" (bow-tie): top edge L→R `(0.5−h+k·size, 0.5+h)`, diagonal TR→BL `(0.5+h−k·size, 0.5+h−k·size)`, bottom edge
  L→R `(0.5−h+k·size, 0.5−h)`, diagonal BR→TL `(0.5+h−k·size, 0.5−h+k·size)`.
- 5 Infinity (lemniscate of Bernoulli): `sc = 0.4·size`, `x = 0.5 + sc·sin a/(1+cos²a)`,
  `y = 0.5 + sc·sin a·cos a/(1+cos²a)`.

### 5.5 Zone (GS:254-315, BR:55-58, 164-176, UI:1146-1198, RM §18.2)

Right-drag draws a rectangle (stored ordered `pad_zone_x0..y1`, clamped 0..1). "Has zone" = any edge beyond 0.0005 of
the full square. Effects: mouse positions are clamped to it (after the grab offset); walk targets are drawn inside it
and its random deviations scale with it; figures are inscribed in it; the bridge clamps the **output** of each axis to it
whoever writes the input (mouse, envelope, source); a source-driven axis is **inscribed** via its link (`x_pos = zx0 +
source·zw`: link `scale = zw`, `baseline = zx0`, `offset = 0`, LE:1590-1660). Setting a zone pulls the point inside and
restarts the Lua walk segment. Zone is a TRACK fact: cleared on track change and re-adopted from the bridge (sliders
32-35). Drawn as a veil (window_bg α0.55) outside + accent outline α0.7; rubber band in text color α0.85.

### 5.6 Granular (GS:108-252, UI:1132-1144, 1267-1281)

- Grid g×g (g ∈ {2,3,4}); grain (i, j) at `((i+0.5)/g, (j+0.5)/g)`; each grain stores, for every **selected** param of
  every FX, `v = gesture_min + U·(gesture_max − gesture_min)`. Changing grid size re-inits + randomizes; "Randomize
  grains" re-rolls values (keeps positions).
- Influence `w = max(0, 1 − dist/(1/g))` (radius = one cell; overlapping); per FX `value = Σ wᵢ·vᵢ / Σ wᵢ` (Σw summed
  over all influencing grains per FX).
- Writes param (or link baseline if sourced), sets `base_value` and anchor to it. Only when `pad_mode == 1` (guard
  GS:181). Native pad links are released in Granular (`want_links = false`); info bar warns.
- Drawn: grid lines (separator α0.35), each grain a circle r = cell/2 (muted α0.25 outline) + 3-px dot (text color).

### 5.7 The bridge JSFX — "FX Constellation Bridge" (BR, = GS:547-746)

v5, **35 sliders**, `@init` + `@block`, **no `@sample`** (audio passes untouched). File rewritten at every script start
(`writeBridgeFile`, GS:749-765); a stale instance (`GetNumParams < 35`) is deleted and re-inserted **at the same chain
position** with every slider value and every envelope chunk carried over (sliders are append-only) (GS:767-924).
Created at end of chain, hidden window. Band counters: `MACRO_BAND_COUNT 22`, `ENGINE_BAND_COUNT 31`, `ZONE_BAND_COUNT
35`, `ZONE_BASE 31`, `MACRO_IN_BASE 6`, `MACRO_OUT_BASE 14` (GS:524-531).

| slider | idx | name | range, default | meaning | written by |
|---|---|---|---|---|---|
| 1 | 0 | `x_pos` | 0..1 (.001), 0.5 | X input | Lua (raw), automation envelopes, native link from the X source (`syncPadAxes`) — **never by @block** |
| 2 | 1 | `y_pos` | idem | Y input | idem |
| 3 | 2 | `slew` | 0..2 s (.01), 0 | one-pole glide time of x/y/macro outputs | `applySlew` (normalized slew/2) |
| 4 | 3 | `x_out` | 0..1, hidden | X output = **link source for pad-X params** | @block only |
| 5 | 4 | `y_out` | idem | Y output | @block |
| 6 | 5 | `xy_mix` | idem | `(x_out + y_out)/2` (source for XY params) | @block |
| 7–14 | 6–13 | `macro1..8` | 0..1, 0 | macro values (also link destinations) | Lua / links / envelopes |
| 15–22 | 14–21 | `macro1..8_out` | hidden | slewed macro outputs (link sources, slots 211..218) | @block |
| 23 | 22 | `eng_mode` | 0..2 int | 0 off, 1 walk, 2 figures | `pushEngine` |
| 24 | 23 | `eng_mask` | 0..3 int | bit 1 = engine owns X, bit 2 = Y | `pushEngine` |
| 25 | 24 | `walk_rate` | 0..20 Hz, 0.5 | Free walk rate | `min(20, random_walk_speed)` |
| 26 | 25 | `walk_jit` | 0..1, 0 | jitter | |
| 27 | 26 | `walk_jump` | 0/1 | teleport | |
| 28 | 27 | `walk_sync` | 0..5 int | 0 free; 1..5 → 0.25/0.5/1/2/4 QN | |
| 29 | 28 | `fig_mode` | 0..5 int | figure | |
| 30 | 29 | `fig_rate` | 0..10 Hz, 0.2 | | `min(10, figures_speed)` |
| 31 | 30 | `fig_size` | 0..1, 0.8 | | |
| 32–35 | 31–34 | `zone_x0, zone_x1, zone_y0, zone_y1` | 0..1, 0/1/0/1 | zone | `pushEngine` (v5 only) |

@block per block:
```
zone ordered (min/max), zw, zh ; run = eng_mode>0.5 && eng_mask>0.5
if run: walk or figures → internal pe_wx/pe_wy (clamped to zone)   else: pe_w = x_pos/y_pos, reset segment timers
tx = (eng_mask&1 && run) ? pe_wx : x_pos ; ty = (eng_mask&2 && run) ? pe_wy : y_pos
tx, ty clamped to zone
slew > 0.001 ? (coef = 1 − exp(−samplesblock/(srate·slew)); out += (t − out)·coef for x, y and all 8 macros)
             : out = t
xy_mix = (x_out + y_out)/2
```
Arbitration (GS:475-505, 1230-1239): `eng_mask` = axes the engine REALLY owns = `(mode>0) · (!pad_link_x·1 + !pad_link_y·2)`;
a source-driven axis keeps its link on `x_pos` while the other axis walks. Engine values are written **raw**
(`TrackFX_SetParam`), only when changed (cache `ENG_LAST`, 13 values: idx 22..34; 9 on a v4) — the historical
normalized-divider bug broke Walk/Figures (GS:949-987).

### 5.8 Lua ↔ bridge loop (GS:1097-1273, 1483-1603, 1688-1834)

Per frame `updateGestureMotion`: `pushEngine()` → `updateAutomationFromJSFX()` → if `engineRunning()` (= bridge has the
engine band AND (Walk & `random_walk_active` OR Figures & `figures_active`)) Lua computes nothing; else Lua runs
jump/walk/figures/smooth and writes `x_pos/y_pos` (`updateJSFXFromGesture`: skipped when the engine runs; never writes a
driven axis; skips changes < 0.0005) and applies via the script path (no-op when `links_active`).
`updateAutomationFromJSFX`: skipped while dragging; reads `x_out/y_out` (params 3,4) when the engine runs, else the
driving source's value inscribed in the zone (`LE.padAxisValue`) for driven axes, else `x_pos/y_pos`; if moved ≥ 0.0005
the pad ADOPTS the position (dot + smooth target); applies via script only if not `links_active`.
`adoptEngine` (when connecting to a bridge this window didn't create — track change, rescan finding one, reuse): reads
the 9/13 engine sliders into state (mode → `navigation_mode` + activity; a stopped engine keeps the user's mode),
`gesture_x/y ← x_out/y_out`, target = pos, `jsfx_last ← x_pos/y_pos`, `readPadSources()`. A new instance is never
adopted; a merely MOVED instance (same FX GUID) is not re-adopted (GS:1179-1219).
`pad_src_x/y` persistence: project ExtState section `CP_FXConstellation`, key `pad_src:<trackGUID>`, value `"x,y"`
(fallback legacy key `pad_src`); visiting a track with 0,0 writes nothing (GS:1020-1075).

### 5.9 Pad axes as destinations and sources

- **Receive**: info-bar pills; `pad_src_x/y` = INTENTION; `pad_link_x/y` = FACT, published only by `LE.syncPadAxes`
  (LE:1670-1765), which links bridge param 0/1 to the source (FX param link or MIDI CC link for global slots), `offset
  0`, `scale = zone width`, `baseline = zone low`. Requires Auto-JSFX coupling; an unresolvable source keeps the
  intention but sets the fact false. Driven axis: pad refuses the mouse on that axis, crosshair line takes the source
  color (α0.9), dial shows the source.
- **Emit**: the two dial pastilles (PAD_X/PAD_Y). Dropped on a param knob → `followPadAxes` with that axis added
  (existing assignment kept only if the param had no source and was tracked); on a slot setting (RATE/PHASE/CURVE/SLEW)
  or a macro → internal native link (`LinkEngine.linkInternal`) (RM §18.4). Pad X color = theme accent, pad Y = accent
  lightened `0.8·a + 0.2` (MD:117-128).

---

## 6. VISUAL LANGUAGE

### 6.1 Palette (CFG = mockup v5 tokens, CD-C)

| role | token | hex / rgba |
|---|---|---|
| title bar / window bg | `title_bar` / `window_bg` | #1b1d1e / #1f2223 |
| surface (cards, rows) / surface2 (FX band) | | #212527 / #26292b |
| popup / command-info bar zone | `popup_bg` | #232628 |
| sunken (pad, fields) | `frame_bg` | #16191a |
| button / hover / active | | #2c3032 / #33393b / #26292b |
| header | | #282c2e |
| text / bright / muted / dim | `text` / `title_text` / `text_mute` / `text_disabled` | #b9bfc0 / #e6ebeb / #818989 / #5b6364 |
| accent (pad, ON, tracked) | `accent` / hover / active / dim / `on_accent` | #13bd99 / #2fd9b3 / #0f9d80 / #12463a / #06231d |
| amber = "turned over": invert, modified, bypass, negative depth | `bypass`, `value_negative`; UI `AMBER` | #d9994a; UI:355 uses (0.85,0.64,0.29) ≈ #d9a34a |
| danger / record | `danger` | #e05a5a |
| borders | `border` | **black α0.376** (borders dig a groove, never light outlines) |
| separator / seams | `separator`; `seam_shadow` / `seam_light` | white α0.06; black α0.52 / white α0.07 |

Greys are **tinted toward the accent (cyan)**, not neutral. Fonts: Tahoma title 15 / h1 14 / h2 12 / body 11 /
caption 9; Consolas 11 for values. Metrics: `button_height 22`, `chip_h 18`, `item_spacing 5`, `window_padding 8`,
`rounding 4 / small 3 / large 6`, `widget_style "flat"`.

Source colors (MD:63-115), same color everywhere (rack pastille, knob ring, wire, row tint, crosshair):
local LFO 1..8 = #8a94dd indigo, #d97fa8 rose, #7fc4d9 cyan, #c9d97f lime, #d9a37f amber, #8bd3a3 green, #e08b7a brick,
#baa3d9 violet; global = same ×0.68 (darker); macros = same ×0.82 + 0.16 (lighter); ENV 1..4 = #e8bd9c, #e9d392,
#e0aa8c, #f2c9b4; SEQ A/B = #63cebd, #7ab1e0; NOTE/VEL/RAND/OCT = #e88ece, #f4809d, #ca98f4, #a78dec; unknown = grey 0.6.
v5 mockup intent: `--m1` LFO #8a94dd, `--m2` FOLLOW #d97fa8, `--m3` S&H #7fc4d9, `--m4` STEPS #c9d97f, `--m5` ENV
#d9a37f, `--m6` macros #b78ad9.

### 6.2 Shapes

- **Knob (ModKnob)** (WID:177-178, 3241ff): 270° sweep from −135° to +135° (0 = up, clockwise; gap at the bottom).
  Layers: dark disk (`frame_bg`) + track ring (border color, thickness `tw = max(2, floor(r·0.1))`, outer radius `r−3`);
  **base arc** from min to base in accent (hover/active variants); **modulation ring** just inside (radius
  `r−3−tw−1`, thickness tw) spanning `win_lo..win_hi` in the source color α0.55 (grey α0.3 when bypassed; accent or
  `value_negative` amber if no source color and depth < 0); extra sources = thin 1-px arcs further in (2-px steps,
  α0.85, α0.25 if bypassed, doubled when highlighted); highlighted knob = blinking ring at r+2; **live dot** on the ring
  (moves at screen rate for a single bank LFO); pointer. Row knob 32 px; macro 18–34; dial 16.
- **Chips**: rounded (radius small 3), ON = accent fill + `on_accent` text; standby = accent letter on neutral; invert
  = amber fill. Hand-drawn shapes go through the toolkit grammar Card / Strip / Sunk / Chip (RM §11.5 f).
- **Pad**: sunken square; crosshair 1 px separator α0.55 (driven axis: source color α0.9); dot r = `max(4,
  floor(pad·0.025))` accent disk + 1-px text-colored outline + soft glow (r·4, α0.35); anchor = hollow accent ring
  α0.40 r+2 (hidden when within 0.01 of the dot); zone veil; grains. v5 also wanted a dashed inset range rectangle,
  a 3-point fading trail, a radial glow `#13bd9944→#13bd9910→0` (72 px) and cursor `#8ff0d8` with glows 10/22 px
  (CD-C) — trail and dashed rect not shipped.
- **Pills** (pad receivers): fully rounded capsules, sunk fill α0.55 (hover α0.95), 1-px frame in the source color
  (α0.8 lit / 0.35 free / 1 while dragging).
- **Pastilles**: filled circle in the source color (α0.85, 1 on hover), dark outline α0.55, glow r·2.2 α0.45 on hover/
  drag; wire dashed.
- **Seams**: 2-px relief (dark line + light line), never a colored rule. Zones are flat; three depths (zone / card /
  sunk). Labels never letter-spaced (gfx limitation, RM §9, §11.4).
- **Cards**: tracked row tint accent 10 %, source-focus tint 26 %; snapshot/preset modified dot amber 3 px.

---

## 7. Discrepancies, dead code, uncertainties (for the rebuild)

1. UI header comment (UI:10-24) and HELP_TEXT (UI:742-768) are outdated: macros left the bar; X/Y also EMIT now
   (pastilles, RM §18.4 reversal of §3.4 — do not "restore" §3.4); the engine text is not clickable.
2. Settings "Log link writes" message says "toggle the engine in the info bar" — that toggle no longer exists.
3. `gesture_range` has no writer since the refonte (AN §1.4) but multiplies every depth (pad AND sources, LE:1037-1049).
4. `x_curve`, `y_curve`, `layout_mode`, `section_width_percent`, `Core.figures_modes` names: declared/saved, unused.
5. `GS.randomizeSelection` dead; `addRandomFX` only reachable through the fallback browser.
6. Per-FX "Roll bases" = U(0,1) vs global B = intensity around span center — inconsistent (?) intended.
7. Triangle figure discontinuous (§5.4).
8. Native-link anchor vs display after re-grab (AN §1.10) — status in Nov unverified (?).
9. Axis default depends on track history (§3.3); `selected = exclusive_xy` at scan is immediately undone by
   `loadTrackSelection` for unsaved tracks (FXM:1191-1213).
10. Granular sets keyed by visible `fx_id`/`param_id` (break on chain edits) (?).
11. `fx_collapsed` keyed by visible index, session-only (?).
12. Alt+drag vs Alt+wheel comment conflict (§2.9) — Alt+drag is current.
13. ModJSFX.randomizeSlot recovered only from the Aug mirror; Nov CP_Mod LFO has 8 shapes
    {Sine, Triangle, Saw Up, Saw Down, Square, Random, S&H, Steps} and 11 syncs {Free … 32 bars}
    (J `CP_Mod LFO.jsfx` sliders 2/4) — the Aug roll used shape 0..5 and sync 1..6 (?).
