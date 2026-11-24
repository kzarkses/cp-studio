# CP FX Browser — la feuille de route

Le navigateur d'effets de CP Studio, à la place de celui de REAPER.
Demandé et construit le 2026-11-24 au soir. Cedric : « Oui, construis-le, et
pars du dossier Favorites de REAPER ».

La liste de REAPER fait « Windows 98 ». L'ancien navigateur (Lua, perdu le
2026-11-20) avait des onglets faits à la main, un dé, et nourrissait FX
Constellation. Sa dernière copie est le miroir GitHub du 22 août
(`FX Constellation/CP_FXBrowser.lua`), avec la maquette de sa refonte
(`Claude Design/design_handoff_fx_browser_redesign`).

## Ce que Cedric a tranché (2026-11-24)

- **Le remplacer.** Pas un outil à côté : il prend la place de la liste de
  REAPER.
- **Les favoris partent du dossier « Favorites » de REAPER.** Ce que CP
  Studio y ajoute ou en retire est gardé à part. REAPER et CP Studio restent
  d'accord sur le reste.
- Ses onglets d'août (kHss, cool, test) et ses récents sont repris, une fois.
  Le fichier a été écrit à la main le 2026-11-24 depuis la copie d'août.

## L'architecture

| pièce | rôle |
|---|---|
| `cp_fxlist` | Le catalogue, pur et testé. La liste de REAPER (`EnumInstalledFX`) et ses fichiers : dossiers, catégories, développeurs, listes VST, CLAP et JS. Puis la recherche, les sources, les onglets, le dé et le fichier de CP Studio. |
| `cp_fxview` | La vue : recherche, pastilles, barre de côté, liste, chaîne. Elle traduit chaque geste en une `Act`. Planches PNG dans `%TEMP%\cp_fxbrowser_*.png`. |
| `cp_sys::fxb` | REAPER : la chaîne d'une piste (master comprise), ce qui est sous la souris, une piste neuve pour un instrument, une action lancée comme un raccourci. |
| `cp_module/src/fxb.rs` | L'application : la fenêtre (trois places), les menus, le remplacement de REAPER. |
| l'extension 2 du chargeur | `steal_action` : une action native passe d'abord par le module. `steal_fx_button` : le clic sur le bouton FX d'une piste passe d'abord par le module. |

**Les fichiers.**
- Ce que CP Studio garde : `<ressources>/CP_Studio/fx_browser.txt`. Une ligne
  par chose, les effets nommés comme REAPER les nomme.
- Les noms d'avant (« kHs Delay (Kilohearts) ») se retrouvent eux aussi.

## L'état

✅ fait, ☐ à faire.

**Le catalogue**
- ✅ La liste de REAPER, reliée à ses fichiers. Un fichier s'écrit de
  plusieurs façons, tout se compare par `norm_file`. Le même nom en VST2 et
  en VST3 garde ses deux fichiers. Un nom montré avec ou sans son
  développeur se retrouve.
- ✅ Les 18 dossiers de Cedric, ses catégories et ses développeurs. Sur ses
  fichiers : 2455 entrées hors REAPER, 1616 effets dans REAPER, en 40 ms.
- ✅ Les favoris : 141 trouvés sur les 149 du dossier « Favorites ». Les 8
  autres ne sont plus installés (Valhalla, kHs Clipper, MDelay) : REAPER
  garde leurs entrées dans ses fichiers.
- ✅ Les chaînes `.RfxChain` de `FXChains`.

**La vue**
- ✅ La recherche : tous les mots, `-mot` exclut. Elle lit le nom, le
  développeur, le type et les catégories.
- ✅ Les pastilles : Tous, Favoris, Récents, Plus utilisés, Instruments,
  puis les onglets. Tab et Maj+Tab passent de l'une à l'autre.
- ✅ Les onglets :
  - faits à la main, en y glissant des effets ;
  - ou « à règle » : une recherche dans une liste, tenue à jour toute
    seule.
- ✅ La barre de côté : les dossiers, catégories, développeurs et formats de
  REAPER.
- ✅ La liste :
  - sélection avec Clic, Ctrl, Maj et les flèches ;
  - Entrée ou double-clic ajoutent ;
  - on glisse vers la chaîne, un onglet, une piste, une colonne de la
    Session, ou sous les pistes (une piste neuve).
- ✅ La chaîne de la piste choisie :
  - déplacer, et copier avec Ctrl ;
  - contourner, supprimer, ouvrir ;
  - chaque geste est un pas d'annulation.
- ✅ Le dé : N effets différents de la liste visible. Jamais un instrument,
  sauf dans une liste d'instruments (un seul). « Replace the chain » garde
  les instruments.
- ✅ Étroite, la chaîne passe sous la liste. Plus étroite, la barre de côté
  se cache.

**Remplacer REAPER** (le réglage « Replace REAPER's FX browser », oui par
défaut)
- ✅ « View: Show FX browser window » ouvre et ferme celui-ci.
- ✅ « Insert virtual instrument on new track » (Ctrl+Y) l'ouvre sur les
  instruments. Le prochain ajout crée la piste, armée et à l'écoute.
- ✅ Le bouton FX d'une piste **sans effet** l'ouvre pour elle. Avec des
  effets, la chaîne de REAPER s'ouvre comme avant.
- ✅ Le « + » des effets d'une tranche de la Session l'ouvre pour sa piste.
- ✅ Le module rend tout à REAPER en se déchargeant.

**Vérifié dans le vrai REAPER** (`cargo xtask reaper-test`, phase
FxBrowser) :
- ajouter, ajouter en tête ;
- déplacer, contourner, copier ;
- le dé, la recherche ;
- l'extension 2 ;
- l'action native lancée comme un raccourci, qui passe par CP Studio, ferme
  puis rouvre la fenêtre.

**Pas vérifié dans le vrai REAPER**
- Le clic sur le bouton FX d'une piste : l'auto-test n'a pas de vraie
  souris. Le nom « tcp.fx » / « mcp.fx » vient de la documentation de
  `GetThingFromPoint`.
- Un glisser hors de la fenêtre vers une piste ou la Session.

**Reste**
- ☐ Le bouton « Add » de la fenêtre de chaîne de REAPER, son double-clic,
  son clic droit. À mesurer d'abord.
- ☐ Le double-clic sur une case vide des effets de la console
  (`mcp.fxlist` ?).
- ☐ Les effets d'une prise, d'entrée, de monitoring.
- ☐ Ce que FX Constellation recevait après une insertion (paramètres,
  portées, pools ROLL / détaché). Il attend que FX Constellation soit refait
  en Rust.
- ☐ Les presets, l'aperçu d'un instrument.

## Les faits mesurés (2026-11-24)

- `TrackFX_CopyToTrack` sur la même piste compte la place d'arrivée **avant**
  de retirer l'effet. Avec deux effets, 0 vers 1 ne bouge rien ; 0 vers 2
  le met à la fin.
- `Main_OnCommand` lancé par un plugin ne passe pas par les crochets
  d'actions (`hookcommand2`). Un raccourci, un menu ou
  `KBD_OnMainActionEx`, si.
- L'ident d'`EnumInstalledFX` est le chemin complet d'un VST (2 et 3) et
  l'identifiant d'un CLAP.
- Les dossiers de REAPER (`reaper-fxfolders.ini`) :
  - `Type` vaut 3 pour un VST (son chemin), 2 pour un JS (son chemin sous
    `Effects`), 1000 pour une chaîne (son nom) ;
  - le même fichier peut y être deux fois ;
  - `[developer]` peut aussi s'y trouver, à côté de `reaper-fxtags.ini`.
