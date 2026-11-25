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
  les instruments. Jamais un effet du cœur (FX Constellation, ses sources,
  le synthé de Guitar Tab : `cp_fxlist::is_internal`, un test le compare
  aux JSFX que le module pose), et « Replace the chain » les garde
  (2026-11-25, demande de Cedric).
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
- ✅ La boîte « Add FX to Track N » de REAPER (demande de Cedric, le même
  soir) : le bouton « Add » d'une fenêtre de chaîne, un emplacement libre de
  la liste d'effets d'une piste, le clic droit « Add FX »… Tous ouvrent
  cette boîte. Un crochet WinEvent du chargeur la reconnaît à son titre
  quand elle se montre, la ferme comme « Cancel » et ouvre la nôtre pour la
  même piste. Vérifié dans le vrai REAPER : l'auto-test clique le vrai
  bouton « Add ». Une chaîne
  d'entrée, de monitoring ou de prise reste à REAPER.
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
- Un emplacement libre de la liste d'effets du TCP : même raison. Il passe
  par la même boîte « Add FX to » que le bouton « Add », que l'auto-test
  clique.
- Un glisser hors de la fenêtre vers une piste ou la Session.

**Reste**
- ☐ Les effets d'une prise, d'entrée, de monitoring (leur boîte « Add FX »
  reste celle de REAPER).
- ✅ Le système ne se montre dans aucune liste à nous (règle de Cedric,
  2026-11-25 : « dans toute liste d'effets que l'on affiche nous-même, on
  retire toujours ce qui est nécessaire aux systèmes ») : le catalogue, la
  chaîne du navigateur, les tranches de la Session. `cp_fxlist::is_internal`
  dit lesquels ; `fxb::shown_fx` rend les effets montrés et leur vraie place,
  et chaque geste (ouvrir, contourner, supprimer, glisser, insérer à une
  place) traduit sa rangée. Vérifié dans `reaper-test` pour le navigateur
  (le bridge caché, contourner la première rangée touche le bon effet) ; la
  Session n'a pas d'auto-test de ses effets.
- ✅ La post-insertion (2026-11-25) : menu « After insert (FX
  Constellation) » du navigateur et du dé. Après un ajout, FX Constellation
  suit des paramètres de l'effet (aucun, tous, les continus, N au hasard ;
  jamais un mot caché ni Bypass/Wet/Delta), puis tire axe X/Y, portée,
  base, inversion comme ROLL ; « Insert bypassed ». Sur la piste qu'il
  montre, en place ; sur une autre, dans l'état de la piste.
- ☐ Les presets, l'aperçu d'un instrument.

## Les faits mesurés (2026-11-24)

- `TrackFX_CopyToTrack` sur la même piste compte la place d'arrivée **avant**
  de retirer l'effet. Avec deux effets, 0 vers 1 ne bouge rien ; 0 vers 2
  le met à la fin.
- `Main_OnCommand` lancé par un plugin ne passe pas par les crochets
  d'actions (`hookcommand2`). Un raccourci, un menu ou
  `KBD_OnMainActionEx`, si.
- La boîte « Add FX to » s'appelle `Add FX to Track 7 "Nom"`. Le crochet
  d'activation (CBT, `HCBT_ACTIVATE`) ne la voit pas ; un crochet WinEvent
  (`EVENT_OBJECT_SHOW`, `EVENT_OBJECT_NAMECHANGE`), si.
- L'ident d'`EnumInstalledFX` est le chemin complet d'un VST (2 et 3) et
  l'identifiant d'un CLAP.
- Les dossiers de REAPER (`reaper-fxfolders.ini`) :
  - `Type` vaut 3 pour un VST (son chemin), 2 pour un JS (son chemin sous
    `Effects`), 1000 pour une chaîne (son nom) ;
  - le même fichier peut y être deux fois ;
  - `[developer]` peut aussi s'y trouver, à côté de `reaper-fxtags.ini`.
