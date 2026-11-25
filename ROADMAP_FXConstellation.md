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
- **L'état d'une piste vit dans la piste** (`P_EXT:CPS_FXC`). L'annulation de
  REAPER le ramène avec le reste, et le module le relit quand il change sans
  lui. Au-delà de 48 Ko, il va dans le projet (ExtState `CPS_FXC`, sans
  annulation). Plus de fichier pour toute la machine : il voyageait mal
  entre les projets. (D'abord rangé dans le projet ; la relecture a montré
  qu'une annulation ne l'atteignait pas.)
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

## L'état

✅ fait, ☐ à faire.

**Le modèle** (`cp_fxc`, 21 tests)
- ✅ Les sources (l'espace d'adresses repris), les cartes des JSFX.
- ✅ Le plan des liens : pad seul (sortie du bridge selon les pastilles X / Y,
  ancre en offset), source seule (neutre de la source ou du lien), deux
  sources ou plus et le pad qui roule avec une source (un bus du Mod Mix, un
  bus tenu le reste), axe du pad mené par une source (inscrit dans la zone).
- ✅ L'adoption : le plan d'un modèle, relu comme des liens, refait le même
  modèle (testé).
- ✅ ROLL (les huit axes), le granulaire, les figures (le triangle ne saute
  plus, dans le bridge aussi), les presets et snapshots en texte.

**La vue** (`cp_fxcview`, planches `%TEMP%\cp_fxc_*.png`)
- ✅ Barre de commande, pad, navigation, ROLL, macros, parametres en pools
  ROLL / DETACHED, rack Track / Global / Links, Map, éditeur de slot (LFO,
  S&H, STEP), onglets Sound Gen et Seq, clavier, barre d'état.
- ✅ Chaque contrôle est là où il est dessiné (testé).

**Dans REAPER** (`cp_module/src/fxc*.rs`)
- ✅ La piste suivie, Lock, l'état par piste dans le projet, l'adoption.
- ✅ La passe des liens (seulement ce qui diffère ; nos liens plus voulus
  relâchés, ceux de l'utilisateur jamais touchés).
- ✅ Les effets internes posés au premier besoin, retirés quand plus rien ne
  les demande, les liens recalés par GUID ; un retrait refusé si quelque
  chose mourrait avec (enveloppe, réglage modulé, lien qu'il mène).
- ✅ La banque globale : la piste cachée « CP MOD », son envoi MIDI seul.
- ✅ Le moteur du pad poussé au bridge.
- ✅ ROLL, presets (état entier des effets : `vst_chunk` / `clap_chunk`),
  snapshots, Init, A → B, Sound Gen (garé dans la piste quand il
  s'éteint), séquenceur (patterns par requête), clavier.

**Construit ensuite, la même nuit**
- ✅ Moduler un réglage : les boutons du Sound Gen (le PITCH module son
  `pm`), ceux du séquenceur (les réglages continus), RATE / PHASE / CURVE /
  SLEW du slot choisi, les macros. Un réglage d'un effet interne est un
  paramètre comme un autre (GUID de l'effet, indice) ; une source ne mène
  jamais ses propres réglages ; presets et snapshots les gardent par genre
  d'effet.
- ✅ Des liens sur des liens : une source module la profondeur d'un lien,
  et ainsi de suite ; les 8 voies de profondeur du Mod Mix, chaque
  modulateur avant ce qu'il module ; « Modulate a depth » dans le menu des
  sources, réglés dans l'onglet Links.
- ✅ ROLL, le rappel d'un preset et Init : un pas d'annulation chacun, avec
  l'état de la piste et le Mod Mix posé ou retiré (sinon Ctrl+Z annulait le
  retrait du bus au lieu de ROLL).
- ✅ La capture : Pad X, Pad Y et les macros armés s'écrivent en points
  d'enveloppe sur les curseurs du bridge pendant que REAPER joue (la
  source, jamais ce qu'elle mène).
- ✅ Une relecture indépendante du moteur des liens (12 défauts, corrigés) :
  l'annulation, Init qui écrivait dans le mauvais effet, les liens oubliés
  par le modèle, l'adoption des réglages internes et des voies de
  profondeur, une base changée hors de FXC reprise, trois gestes qui
  faisaient sauter une valeur à 0,5, deux instances du même effet dans un
  snapshot, un vieux bridge qui pouvait être doublé, un bouton MIDI CC 16
  pris pour la banque globale.
- ✅ Un paramètre sur lequel REAPER refuse un lien (ReaComp : AudIn,
  Multichannel Mode) devient « non modulable » : plus suivi, plus tiré par
  ROLL.
- ✅ Une seconde relecture indépendante, de la vue et de la capture (12
  défauts, corrigés) :
  - la capture : un saut en avant n'efface plus le passage sauté, une
    valeur tenue reste plate, le passage se raccorde à l'enveloppe d'origine
    à ses deux bords, les enveloppes sont triées à chaque battement ;
  - les gestes : la couture des colonnes à toute échelle d'écran, la perte
    de la souris qui termine le geste (rien ne reste tenu), un clic droit
    pendant un geste ignoré, le mode Map qui part de la bonne profondeur,
    la peinture des pas qui remplit les pas sautés ;
  - le module : la cible du pad lissé qui restait, un geste sur un effet
    interne qui relisait toute la chaîne, l'état perdu quand on changeait
    d'onglet de projet (il attend désormais que sa piste revienne), le
    clavier qui accroche la note en Continuous, HOLD seulement en
    Triggered + Manual, REMOVE du séquenceur qui demande confirmation quand
    il a plusieurs patterns.
- ✅ Le pad lissé (SMOOTH), l'éditeur d'ENV 2..4 dans le rack, les menus des
  sources de note (étendue, glissé) et des voies du séquenceur, le bouton
  HOLD, un en-tête d'effet tiré d'un pool à l'autre, l'onglet
  « Constellation » du sélecteur de vues de CP Transport, les réglages de
  ROLL au complet.

### Le 2026-11-25 : la nouvelle vue, d'après les retours de Cedric

Ses retours du matin : le pad lent (« sensation 30 fps »), un effet à 2086
paramètres MIDI, pas de barre de défilement, « la grille qui bouge » avec
le point, Walk lent, une interface à repenser (icônes, marges, bordures),
et « ajouter une banque » avant de pouvoir utiliser un LFO. Une maquette
validée (artifact « FX Constellation v2 »), puis :

- ✅ **L'image.** Dessiner coûtait 26,5 ms par image complète (le
  rastériseur tramait toute la surface de chaque carte arrondie, les arcs
  des boutons étaient des traits de 64 segments) : 7 ms. Et une image ne
  repeint que ce qui a changé (la vue d'avant comparée élément par
  élément) : 0,8 ms quand seul le pad bouge, mesuré dans REAPER chez
  Cedric.
- ✅ **Le pad à l'écran.** Walk, Figures et un axe mené sont lus sur le
  bridge à chaque image de l'écran (le moteur reste dans le fil audio : la
  modulation est juste, fenêtre fermée comprise) ; le pad lissé avance au
  pas de l'image ; les boutons qu'il mène suivent au même geste ; un
  sillage montre la marche.
- ✅ **Les mots cachés** de l'ancienne version (lus dans sa configuration :
  `MIDI, CC, midi, Program…`) : un paramètre dont le nom en contient un
  n'est pas montré, jamais s'il est suivi. Le bouton entonnoir les montre ou
  modifie la liste. Une recherche par nom.
- ✅ Une barre de défilement sur chaque zone qui déborde.
- ✅ La grille du pad est fixe (la « grille qui bougeait » était la croix
  qui suivait le point) ; des repères sur les bords.
- ✅ Les huit LFO toujours là : le premier clic ou glisser pose la banque.
  L'éditeur d'un LFO : la forme en grand, ses formes en icônes, ce qu'il
  mène et de combien.
- ✅ L'aspect : des cartes (marge, bordure fine, ombre), des icônes à la
  place des boutons à texte, des choix segmentés ; tout aux couleurs du
  thème de REAPER.
- ✅ Une troisième relecture indépendante, de cette journée (10 défauts,
  corrigés) : la recherche qui ne se repeignait pas en tapant, la lueur du
  pad qui débordait de l'image partielle, des images qui tournaient sans fin
  (et fenêtre cachée), Ctrl+X/V/Z de la recherche qui partaient à REAPER,
  le pad lissé plus rapide sur un écran rapide, le curseur de défilement
  allumé par n'importe quel geste, une teinte comptée deux fois à 125-150 %,
  un axe mené contre la souris, une cible du pad lissé hors de la zone, la
  recherche sans champ qui gardait le clavier.

### Le 2026-11-25 au soir : le plantage, le preset exact, le master

Cedric a fait une séquence qu'il aimait ; en cherchant l'action du
transport, « les modules ont planté ». Puis : « Je recall, j'ai exactement
le son que je suis en train d'entendre » ; « tout a été mis sur le master ».

- ✅ **Le plantage** n'était pas une panique : la liste d'actions de REAPER
  demande la coche de chaque action, le module répondait -1 pour une action
  sans coche, et le chargeur lit -1 comme une panique (FAULT). Rien au
  journal. Corrigé des deux côtés (`NO_TOGGLE`, 3d4ad98) ; un test le garde.
- ✅ **Le preset exact** (version 2) : la chaîne de la piste telle que REAPER
  la range (`GetTrackStateChunk`, le bloc `<FXCHAIN`) : chaque effet, son
  état entier, les données des JSFX (les 16 patterns du séquenceur sont dans
  son `@serialize`), l'ordre, les liens ; plus l'état `CPS_FXC` et les clés
  de la piste. Le rappel remet ce bloc (les effets prennent des identifiants
  neufs, recalés dans l'état), puis relit la piste comme à l'ouverture d'un
  projet. La version 1 (chaque effet refait par son nom, l'ordre perdu, les
  patterns perdus) se lit encore.
- ✅ **Le master** : FX Constellation suit la piste choisie, et la maître
  quand elle seule l'est (un clic sur sa tranche). Un point ambre le dit
  dans le champ de la piste ; « Move everything to a new track » (menu du
  preset) met chaîne et état sur une piste neuve et vide l'autre.
- ✅ **La post-insertion** (le navigateur) : après un ajout, FX Constellation
  suit des paramètres de l'effet (aucun, tous, les continus, N au hasard ;
  jamais un mot caché), puis tire axe, portée, base, inversion comme ROLL ;
  l'effet peut arriver contourné. Menu « After insert » du navigateur et du
  dé. Le dé ne tire jamais le cœur, et « remplacer la chaîne » le garde.

- ✅ **Le glisser commun** (`cp_module/src/carry.rs`, demande de Cedric : « un
  dnd fluide entre chaque module ») : une vue source pose ce qu'elle porte et
  le tient à jour à chaque mouvement ; la vue CP sous la souris (la fenêtre
  la plus profonde sous le point, où qu'elle soit dockée) allume la place du
  lâcher ; au lâcher, c'est elle qui reçoit. Premier cas : des effets du
  navigateur vers FX Constellation — l'onglet des paramètres se montre, la
  colonne ROLL ou DETACHED s'allume, un trait marque « avant cet effet » ;
  dans ROLL la post-insertion s'applique, dans DETACHED ils arrivent
  détachés. Ailleurs, le navigateur garde son geste d'avant. À ajouter : un
  effet d'une chaîne, les sources de modulation, les clips, d'autres vues
  d'arrivée.

**Vérifié dans le vrai REAPER** (`cargo xtask reaper-test`, phase Fxc,
2026-11-25) :
- suivre un paramètre pose le bridge et le lien du pad ; le pad écrit sa
  position ;
- une source tirée pose la banque et allume le slot ;
- deux sources passent par un bus du Mod Mix posé en tête, et le lien du
  pad suit son bridge ;
- un lien sur un lien : la voie de profondeur lit la banque et module
  l'entrée du bus ;
- une source sur une macro ; une macro ne se mène pas elle-même ;
- ROLL ; trois ROLL de l'ordre de la chaîne : chaque lien suit son effet ;
- un preset enregistré puis rappelé rend les mêmes paramètres (réglages
  internes compris) ;
- le Sound Gen posé puis retiré (son réglage gardé dans la piste) ; le
  séquenceur en tête puis retiré ;
- l'état de la piste relu ;
- une annulation de ROLL ramène le modèle d'avant ;
- une base changée hors de FX Constellation est reprise, pas écrasée ;
- Init relâche chaque lien à nous, celui d'une macro aussi ;
- la capture : lecture, le pad bouge, les points d'enveloppe écrits ;
- la fenêtre photographiée (`browserfxc.png` dans `cp_selftest`), et le
  temps d'une image : 0,81 ms quand seul le pad bouge, 7 ms en entier.

**Pas vérifié dans le vrai REAPER** : les gestes à la vraie souris
(l'auto-test appelle les actions de la vue), Walk à l'œil, la recherche au
clavier, le son.

**Reste**
- ☐ Les réglages de la banque globale (sur CP MOD) menés par une source.
- ☐ Les routes rapides (banque → Sound Gen tous les 32 échantillons), charger
  un sample dans le Sound Gen : ils passent par la mémoire partagée des JSFX
  (`gmem`), qu'une extension n'a pas (les 16 patterns, eux, voyagent dans le
  preset exact). Sans route rapide, une source atteint le Sound Gen par un lien natif
  (une valeur par bloc).
- ☐ La capture du clavier en item MIDI ; les vues des effets maison.
- ☐ La recherche garde le clavier après un Alt+Tab sans clic (le module ne
  voit pas encore la revendication du clavier du chargeur).
- ✅ Passés dans `reaper-test` le 25 au soir : le preset exact (même chaîne,
  même ordre, identifiants neufs, état qui suit), le cœur gardé par le dé,
  la post-insertion, le système caché dans la chaîne du navigateur, un effet
  détaché marqué et gardé, ReaEQ lâché dans DETACHED par le glisser commun.
  Pas couverts : « Move everything to a new track », le survol à la vraie
  souris.

## Les faits

- La mémoire partagée des JSFX (`gmem`) n'est pas offerte aux extensions (le
  SDK 7.78 ne la déclare pas). CP Studio lit et écrit les JSFX par leurs
  curseurs.
- Sur un curseur courbe (`:log`, `:sqr`), `TrackFX_GetParam/SetParam` parlent
  le domaine de la courbe : passer par le normalisé.
- `GetSetMediaTrackInfo_String` ne reçoit pas la taille du tampon : les
  valeurs de piste restent courtes, l'état long va dans le projet.
