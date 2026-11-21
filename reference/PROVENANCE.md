# La reference : l'ancien moteur C++

Ce dossier contient le coeur de l'ancien moteur (CP_Native, efface le
2026-11-20), tel qu'il a pu etre reconstitue. Il ne sert qu'aux tests :
`cp_oracle` le compile en DLL et compare sa sortie a celle de `cp_engine`,
echantillon par echantillon. Il n'est jamais livre.

## D'ou viennent ces fichiers

Des transcriptions des sessions de travail avec Claude (copiees sur
`E:\CP_Scripts_recovery_20261120\claude_transcripts`). Ces sessions contenaient
leur contenu complet, lu ou ecrit. Chaque fichier a ete confronte aux empreintes
git des arbres que `winfr` a pu rendre du disque efface.

C'est un INSTANTANE COHERENT du 2026-10-25 au matin (vers 10 h 45). Il est donc
anterieur aux lots du socle de l'apres-midi : comptage des pertes, ports a
generation, notes du moteur, etc.

| fichier | etat |
|---|---|
| `cp_types.h` | identique a un commit (empreinte git) |
| `cp_ring.h` | identique a un commit |
| `cp_pool.h`, `cp_pool.cpp` | identiques a un commit |
| `cp_engine.h` | identique a un commit |
| `cp_curve.h` | identique a un commit |
| `cp_extclock.h`, `cp_extclock.cpp` | identiques a un commit |
| `cp_engine.cpp` | complet ; lu a 10 h 44, modifie plus tard dans la journee |
| `cp_voice.h`, `cp_voice.cpp` | complets ; lus a 10 h 44, modifies plus tard |
| `cp_lanes.h` | complet ; lu a 10 h 45, modifie plus tard |
| `cp_lanes.cpp` | recousu de 4 lectures partielles concordantes, sans trou |
| `cp_health.h` | complet ; ecrit a 11 h 30, modifie plus tard |

« Modifie plus tard » : les arbres git rendus portent une autre empreinte pour
ce nom. Ce sont les lots du socle (`project-audit-socle` dans la memoire de
Claude) : ils ajoutent des compteurs et des gardes, pas des changements de son.

### Le reste de CP_Native (hote, dessin, tests)

Meme origine. Ils serviront a A1 (le branchement dans REAPER) et a B2 (le
dessin). Aucun n'est prouve identique a un commit ; ceux qui finissent en
`.partiel` sont TROUES : chaque ligne manquante y est marquee
`/*@@ LIGNE MANQUANTE @@*/`.

| dossier | complets | troues |
|---|---|---|
| `src/host` | `cp_source.h` (identique a un commit), `cp_source.cpp`, `cp_playclock.h`, `cp_sane.h` (identique), `cp_key.h` (identique), `cp_win.h`, `cp_list.h`, `cp_log.cpp` | `cp_main.cpp` (40 lignes sur 3 199), `cp_win.cpp` (800 sur 4 099), `cp_list.cpp` (135 sur 314) |
| `src/gfx` | `cp_blit.cpp`, `cp_raster.cpp`, `cp_raster2.cpp`, `cp_text.cpp` | `cp_surface.h` |
| `src/test` | — | `harness.cpp`, `gfx_test.cpp` (leurs versions du 22 aout sont entieres sur GitHub, voir ci-dessous) |
| `src/video` | `cp_vsink.cpp` (recousu) | — |
| `src/tab`, `tools/gen_bp_weights.py` | le portage C++ de Basic Pitch ecrit le 2026-11-20 pour la tablature (verifie contre onnxruntime a 2e-5) ; `cp_bp_weights.h` se regenere avec `gen_bp_weights.py` depuis `nmp.onnx` | — |

L'etat de CP_Native au 22 aout (harnais compris) est entier dans le miroir
GitHub : `E:\CP_Scripts_recovery_20261120\github_2026-08-22.git`. Les scripts
de reconstitution sont dans `E:\CP_Scripts_recovery_20261120\tools`
(`extract_ref.py` refait ce dossier depuis les transcriptions).

## Le binaire final

Le binaire final de l'ancien moteur (ABI 11) existe encore, installe dans
REAPER : `UserPlugins\reaper_cpclip.dll`. Une copie est dans
`E:\CP_Scripts_recovery_20261120\reaper_installed`.

## L'oracle

`oracle/oracle.cpp` expose l'`Engine` C++ en fonctions C, sans rien y changer.
`cp_oracle` le compile avec MSVC (`/O2`, `/fp:precise`, SSE2, comme l'ancien
binaire), dans un dossier temporaire hors du depot.
