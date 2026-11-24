# FX Constellation — la feuille de route (CP Studio, Rust)

FX Constellation refait en natif dans CP Studio. Demandé le 2026-11-24 au
soir. Cedric : « Oui, FX Constellation » puis « Tu peux tout faire,
séquenceur et sound gen dans la nuit directement. Prends le temps qu'il
faut ».

L'ancienne version (Lua, ~36 000 lignes) a été effacée le 2026-11-20. Ce qui
en reste a été relu la nuit du 24 au 25 et décrit dans
`reference/fx_constellation/` :

| fichier | ce qu'il décrit |
|---|---|
| `spec_window.md` | la fenêtre, les paramètres suivis, ROLL, le pad, le bridge |
| `spec_modulation.md` | les banques de sources, les liens natifs, le bus Mod Mix, ModGraph, le rack |
| `spec_seq_sg.md` | le séquenceur et le Sound Gen (JSFX et panneaux) |
| `spec_state.md` | ce qui est gardé, les presets et snapshots, les règles de l'ancien CLAUDE.md |

Les sources de ces relevés : le dossier survivant (modules de FXManagerUI à
UI, la feuille de route d'octobre), le miroir GitHub du 22 août, l'historique
des fichiers de Claude, et les JSFX installés dans REAPER.

## Ce qui est décidé (cette nuit, en autonomie)

- **Les mêmes JSFX, tels quels.** Bridge, `CP_Mod LFO`, `CP_Mod MIDI`, Mod
  Mix, Notes, Sequencer, Sound Generator : les copies installées (25 octobre
  et 9 novembre) sont dans `crates/cp_module/src/fxc_jsfx/`. Le module les
  écrit dans `Effects` seulement s'ils manquent ou diffèrent. Ils n'étaient
  sauvegardés nulle part ailleurs.
- **Les mêmes liens, les mêmes clés.** Les numéros de sources (1..8, 101..108,
  201..204, 211..218, 221..222, 231..234, -1/-2), les liens natifs
  (`param.N.plink.*`), la piste cachée « CP MOD », `FXC_ROUTES`,
  `CP_FXC_detached` : les projets de Cedric les ont sur disque.
- **Une piste sans état adopte ses liens.** Les fichiers de l'ancienne
  version (`selections.dat`, les presets de novembre) sont perdus : les liens
  natifs des projets sont le seul endroit qui dit quels paramètres suivent le
  pad, avec quelle portée et quelle source. À la première visite d'une piste,
  CP Studio les relit et en refait son modèle. Il ne les relâche pas.
- **L'état d'une piste vit dans le projet** (`CPS_FXC`, clé = GUID de la
  piste). Plus de fichier pour toute la machine : il voyageait mal entre les
  projets.
- **Un paramètre se nomme par le GUID de son effet** et son indice. L'ancienne
  clé par nom confondait deux instances du même effet.
- **Le modèle est la vérité pendant la session**, le lien l'est à
  l'adoption. Une passe calcule les liens voulus (`cp_fxc::links::plan`, pur
  et testé) ; le module n'écrit que les écarts.
- **Pas de raccourci à lui** : comme avant, seul Échap.

## L'architecture

| pièce | rôle |
|---|---|
| `cp_fxc` | le modèle, pur : les sources, les parametres suivis, le pad, ROLL, le plan des liens, l'état gardé, les presets |
| `cp_fxcview` | la vue : barre de commande, pad, ROLL, macros, paramètres, rack des sources, Sound Gen, séquenceur, barre d'état |
| `cp_sys::fxc` | REAPER : les paramètres d'un effet (normalisé, brut, affiché), les réglages nommés (les liens), les GUID, les données de piste et de projet, les presets |
| `cp_module/src/fxc*.rs` | l'application : la fenêtre, la piste suivie, la passe des liens, les effets internes, les menus |

## Le plan de la nuit

☐ à faire, ✅ fait.

1. ☐ Le modèle (`cp_fxc`) : sources, paramètres, pad, plan des liens (pad,
   source seule, bus, axes menés), adoption, état gardé, ROLL, presets.
2. ☐ L'accès à REAPER (`cp_sys::fxc`) et les JSFX installés.
3. ☐ La fenêtre : pad (Manual / Walk / Figures / Granular, zone), ROLL et ses
   8 axes, macros, liste des paramètres (pools ROLL / DETACHED), barre de
   commande (presets, snapshots, Lock), barre d'état.
4. ☐ La modulation : rack Track / Global, tuiles, éditeur de slot (LFO, S&H,
   STEP), glisser une pastille sur un bouton, anneaux, Alt+glisser,
   plusieurs sources par le bus, Links, Map.
5. ☐ Le Sound Gen et le séquenceur : leurs onglets, le clavier.
6. ☐ L'auto-test dans le vrai REAPER (`cargo xtask reaper-test`).

## Les faits

- La mémoire partagée des JSFX (`gmem`) n'est pas offerte aux extensions (le
  SDK 7.78 ne la déclare pas). CP Studio lit et écrit les JSFX par leurs
  curseurs.
- Sur un curseur courbe (`:log`, `:sqr`), `TrackFX_GetParam/SetParam` parlent
  le domaine de la courbe : passer par le normalisé.
- `GetSetMediaTrackInfo_String` ne reçoit pas la taille du tampon : les
  valeurs de piste restent courtes, l'état long va dans le projet.
