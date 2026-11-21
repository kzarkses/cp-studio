# CP Studio — la roadmap complète

Tout ce qu'il faut faire pour reconstruire la suite en natif, de la première
note jouée jusqu'à la dernière application. Écrite le 2026-11-21, après la perte
de l'ancien dépôt (CP_Scripts, Lua + C++).

Chaque étape dit **quand elle est finie**, par un critère qu'on peut mesurer ou
entendre. Une étape n'est pas finie parce que son code compile.

---

## 0. Les exigences

Posées par Cedric le 2026-11-21. Tout le reste en découle.

1. **Dessiner ce qu'on veut.** Une forme, une courbe, un widget nouveau ne doit
   jamais demander de toucher au moteur de dessin.
2. **Une fluidité exemplaire, au taux de rafraîchissement de l'écran.**
3. **Plus de Lua, tout en natif.** Une seule langue (Rust), un seul binaire
   pour le code de la suite.
4. **Des fenêtres ancrables.** C'est essentiel : toute fenêtre doit pouvoir
   vivre dans un docker de REAPER, comme une fenêtre de REAPER.
5. **Une lecture audio exacte à 100 %.** Ça vaut pour la lecture simple,
   l'étirement, la vitesse de lecture, et les sélections minuscules : une
   sélection de quelques échantillons doit se lire.

Ce que ces mots veulent dire, en critères :

| exigence | critère |
|---|---|
| fluidité | aucune image ratée pendant un geste de 10 s (glisser, défiler, zoomer), au rafraîchissement de l'écran de Cedric. Compté par la suite elle-même, pas estimé. Au moins 60 Hz sur la machine la plus faible visée (un PC de 2005), en ne repeignant que ce qui change. |
| latence de l'écran | entre un mouvement de souris et l'image qui le montre : une image au plus, plus la présentation. |
| lecture exacte | à vitesse 1 sans étirement, la sortie EST le fichier, au bit près. |
| départ et fin | à l'échantillon exact, dans le bloc (déjà vrai). |
| sélection minuscule | une sélection de N échantillons joue exactement N échantillons de matière, jusqu'à N = 1. Aucun fondu imposé par le moteur ne la recouvre. |
| vitesse | une position exacte, sans dérive mesurable au bout de 10 minutes, avec une interpolation choisie (rapide ou haute qualité). |
| étirement | la qualité de l'étireur de REAPER (élastique), et la durée exacte demandée, à l'échantillon. |
| latence du son | entre un clic et le son : un bloc audio, plus une image au plus. |
| ancrage | chaque fenêtre s'ancre dans n'importe quel docker, garde sa place au redémarrage et dans les screensets. Le clavier et la souris y marchent comme en flottant. |

---

## 1. Où on en est

| fait | quand | preuve |
|---|---|---|
| ✅ Rust installé, dépôt neuf, public, poussé après chaque commit | 2026-11-21 | github.com/kzarkses/cp-studio |
| ✅ La petite DLL fixe et le rechargement à chaud du module | 2026-11-21 | retouche recompilée en 0,6 s, rechargée en 0,9 s ; le vrai chargeur testé dans un faux REAPER |
| ✅ Le cœur du moteur audio : voix, colonnes, vivier, horloge | 2026-11-21 | identique au bit à l'ancien C++ (10 scénarios, 300 graines de fuzz) |
| ✅ La première application est choisie : le navigateur de fichiers | 2026-11-21 | décision de Cedric |
| ✅ A1 : le moteur branché dans REAPER (aperçu matériel permanent, crochet, décodage par REAPER, pics) | 2026-11-21 | `cargo xtask reaper-test` dans le vrai REAPER : deux WAV relus au bit près, aperçu tiré une fois par bloc, aucun compteur de perte, 0,18 % du fil audio pour 3 voix |
| ✅ B1 (en partie) : fenêtres flottantes ou ancrées, clavier des fenêtres ancrées (crochet « accelerator » + dernier clic), double-clic | 2026-11-21 | auto-test : le navigateur s'ancre dans un docker et en revient ; reste : screensets, DPI par écran testé |
| ✅ B2 (base) : `cp_gfx`, rastériseur à couverture exacte, chemins, Bézier, traits, dégradés, transparence, découpage au pixel | 2026-11-21 | vérités géométriques (disque = πr², rectangle fractionnaire = son aire), planches PNG |
| ✅ B3 (base) : texte par GDI, cache de glyphes, mesure, « … » | 2026-11-21 | captures du vrai REAPER |
| ✅ B4 (base) : icônes Lucide en vecteur (SVG avec arcs) | 2026-11-21 | test : chaque icône se dessine dans son carré |
| ✅ B5 (base) : horloge calée sur l'écran (DwmFlush), images sautées comptées | 2026-11-21 | vrai REAPER : 120 Hz mesurés, 0 image en retard ; une image = 2,1 ms ; reste : ne repeindre que ce qui change |
| ✅ B7 (base) : mesures à l'écran (temps d'image, cadence, images sautées) | 2026-11-21 | action « Performance overlay » |
| ✅ A2 (base) : écoute en flux (jouable dès la première seconde décodée), pics à plusieurs résolutions | 2026-11-21 | tests ; limite 1,5 Go par fichier (23 min stéréo 48 kHz) ; reste : le vrai flux disque pour les fichiers de plusieurs heures |
| ✅ A3 (premier critère) : une section de N échantillons rend N échantillons, N de 1 à 4096 | 2026-11-21 | test `cp_audio` |
| ✅ Le navigateur V1 : arbre des raccourcis, en-têtes collés empilés, écoute au relâchement, forme d'onde à tout zoom (64 px par échantillon), section, insertion et glisser-déposer avec section, recherche (noms + MediaDB), favoris, clavier, ancrable | 2026-11-21 | tests avec le vrai moteur et les vraies polices ; vrai REAPER (insertion exacte, fenêtre, ancrage) |
| ▶ **En cours : le navigateur V2** (collections, hasard, calage au tempo, remplacement à chaud, effets), puis le retour de Cedric | | |

**Le but immédiat est le jalon 1** (voir plus bas) : un navigateur de fichiers
complet et utilisable au quotidien. Il fait tourner ensemble les deux moteurs,
le toolkit et l'ancrage, pour que Cedric puisse s'en servir pour de vrai et
faire un vrai retour.

---

## 2. La méthode

- **L'ancien moteur est la référence** tant qu'on le porte. Chaque morceau porté
  est comparé à l'ancien C++ (l'oracle), sur les mêmes entrées. Quand on
  s'écarte exprès de l'ancien (phase A3), l'écart a son propre test, et les
  scénarios qui n'ont pas changé restent comparés.
- **On mesure avant de conclure.** La fluidité se compte (images ratées), la
  charge audio se compte (part du fil audio), les pertes se comptent (commandes
  perdues, rendus refusés).
- **Les tests tournent hors de REAPER** : le moteur audio, le dessin (images
  comparées au pixel), le chargeur (faux REAPER). Ce qu'ils ne prouvent pas,
  l'écran et l'oreille, c'est Cedric qui le juge.
- **Le rechargement à chaud** sert à tout ce qui est interface : une retouche se
  voit en une seconde, sans fermer REAPER.
- **Les mémoires de Claude sur l'ancienne suite** sont la spécification de ce
  que faisait chaque outil, et la liste des pièges de REAPER déjà payés.

---

## Phase A — Le moteur audio

### A1. Le brancher dans REAPER : le premier son

- **La source de chaque colonne.** Une `PCM_source` maison (une classe C++ de
  REAPER), écrite en Rust dans `cp_sys` en reproduisant à la main la table
  virtuelle de MSVC. Un petit programme C++ compilé contre le SDK vérifie que
  la disposition est la bonne.
- **Un aperçu de piste permanent par colonne** (`PlayTrackPreview2Ex`) : jamais
  arrêté, il rend du silence quand rien ne joue. On ne crée ni ne détruit rien
  au lancement d'un son.
- **L'horloge :** le crochet audio (`Audio_RegHardwareHook`) fait avancer
  l'horloge au passage « post », après le rendu du bloc.
- **La lecture des fichiers :** un fil de travail décode avec la `PCM_source`
  de REAPER (tous les formats que REAPER lit) vers le vivier.
- **Où vit le moteur** (décision recommandée) : dans la petite DLL fixe. Il ne
  se recharge donc pas, et une retouche de l'interface ne coupe jamais le son.
  Changer le moteur demande un redémarrage de REAPER, comme avant, mais c'est
  rare, et il est testé hors de REAPER.
- **Fini quand :** une action de test lance une boucle et Cedric l'entend, sans
  clic. Les compteurs de lecture (trop ou pas assez de blocs lus) restent à zéro.
- **Pour reprendre :**
  - **La référence :** `reference/cp_native/src/host/cp_source.h` et
    `cp_source.cpp`. C'est l'ancienne `PortSource` : `GetSamples` remplit le
    bloc depuis `render_port` et convertit float → ReaSample, plus le MIDI
    daté.
  - **Le SDK** (`C:\Users\Cedric\dev\reaper-sdk\sdk`) :
    - `reaper_plugin.h` donne `class PCM_source` (l'ordre des méthodes
      virtuelles fait la table), `PCM_source_transfer_t`, `preview_register_t`
      et `audio_hook_register_t` ;
    - `reaper_plugin_functions.h` donne `PlayTrackPreview2Ex`,
      `StopTrackPreview2`, `Audio_RegHardwareHook`, `PCM_Source_CreateFromFile`
      et `GetPlayPosition2`.
  - **Les pièges déjà payés :**
    - la position de lecture qui va avec l'horloge est `GetPlayPosition2`
      (bloc en traitement), pas `GetPlayPosition` (ce qu'on entend, en retard de
      la latence de sortie) ;
    - un aperçu vers le master rend « succès » et ne joue rien ;
    - une piste qui porte un instrument avale l'audio qu'on y verse.
  - **Le test :** étendre le faux REAPER (`cp_sys/tests/fake_reaper.rs`) pour
    qu'il tire les blocs d'un aperçu comme REAPER le fait.

### A2. Le vivier complet

- Les fichiers de toute taille. Le plafond actuel (64 s à 96 kHz) vaut pour les
  boucles. Pour écouter un long fichier (navigateur), il faut une **lecture en
  flux** : un fil lit le disque en avance, la voix lit un anneau.
- Le budget mémoire, le retrait et le ramassage, comptés.
- **Les pics pour l'affichage** des formes d'onde, calculés sur un fil de
  travail, à plusieurs résolutions (le zoom ne recalcule rien).
- **Fini quand :** un fichier de 2 h s'écoute sans attente ni coupure, et sa
  forme d'onde se dessine à tout zoom sans calcul visible.

### A3. L'exactitude à 100 %

C'est l'exigence 5. Le moteur s'y écarte exprès de l'ancien C++.

- **Les fondus se plient à la région.** Aujourd'hui, un saut de tête croise sur
  5 ms et un lancement sur une voix qui sonne impose 5 ms de fondu : sur une
  sélection de 2 ms, ces fondus mangent tout. Désormais, un fondu ne dépasse
  jamais la moitié de la région.
- **Des régions jusqu'à 1 échantillon**, en lecture simple et en boucle. Une
  boucle minuscule fait un bourdonnement, et c'est son vrai son. Le moteur n'y
  ajoute aucun clic.
- **Une seule correspondance temps ↔ échantillon**, écrite une fois et testée :
  - temps du projet → échantillon du fichier, avec le décalage de prise, le taux
    de prise et le taux d'échantillonnage du fichier ;
  - et dans l'autre sens.

  Une sélection faite dans REAPER ou dans l'éditeur tombe sur le bon échantillon.
- **Les tests :**
  - à vitesse 1 sans étirement, la sortie est le fichier au bit près ;
  - une région de N échantillons rend N échantillons, pour N de 1 à 4096 ;
  - aucune dérive de position au bout de 10 minutes à un taux quelconque ;
  - le départ et la fin tombent à l'échantillon, même dans un bloc de 1.
- **Fini quand :** les tests ci-dessus passent, et Cedric entend une sélection
  minuscule faite à l'écran.

### A4. La vitesse et la hauteur

- Deux interpolations :
  - **Hermite** (actuelle), rapide, pour le jeu en direct ;
  - **sinc polyphasé**, haute qualité, quand la machine le permet et pour tout
    rendu.
- **La lecture à l'envers** : le sampler savait jouer en REVERSE.
- **La hauteur sans changer la durée**, par l'étirement (A5).
- **Fini quand :** à vitesse 0,5, 1,5 et 2, la différence avec un rendu de
  référence (sinc long, hors temps réel) est sous un seuil mesuré. Et la
  lecture à l'envers est exacte à l'échantillon.

### A5. L'étirement

L'ancien moteur avait mesuré l'étireur de REAPER : 2,3 % du fil audio par voix,
et 85 à 139 ms d'amorçage. D'où deux chemins.

- **Cuit (le défaut)** : la région étirée est rendue une fois dans un fichier
  par REAPER (élastique, la même qualité qu'un item), mise en cache, puis jouée
  à vitesse 1. La lecture ne coûte rien et la durée est exacte. Tant que la
  version cuite manque, le son joue en changeant de hauteur. Ça ne dure qu'une
  passe.
- **En temps réel** (`IReaperPitchShift`), pour ce qui change pendant qu'on
  joue : un tempo qu'on tourne, une hauteur qu'on règle. L'amorçage est
  compensé par une avance.
- **Fini quand :** une boucle à 90 BPM jouée à 120 BPM dure exactement sa passe
  et sonne comme le même item étiré par REAPER. Et une hauteur qu'on tourne en
  jouant suit sans clic.

### A6. Les boucles MIDI (les lanes)

- D'abord **reconstituer `cp_lanes.cpp` en entier**. Il n'a été retrouvé qu'en
  morceaux. On recoud en rejouant les modifications des transcriptions, et ma
  mémoire comble le reste.
- Puis le porter, et le comparer à l'ancien.
- Ce que les lanes font :
  - boucles de notes datées à l'échantillon ;
  - accolade de boucle ;
  - automation des clips ;
  - case audio jouée par la lane ;
  - capture de l'entrée MIDI ;
  - sortie MIDI par colonne.
- **Fini quand :** l'oracle passe sur les lanes, et une boucle MIDI tourne dix
  minutes sans une note décalée.

### A7. La clock MIDI externe

- Son code de référence a été retrouvé en entier et prouvé identique
  (`cp_extclock`). On le porte, puis on le compare à l'ancien.
- **Fini quand :** l'oracle passe, et CP Studio se cale sur une machine externe.

### A8. L'enregistrement

- **La prise audio dans une case** : capture native, latence compensée, rejouée
  dès la fin de la passe.
- **La prise MIDI dans une lane.**
- **Fini quand :** une prise tombe à sa place à l'échantillon, mesurée contre une
  boucle de retour.

### A9. La robustesse

C'est le travail du socle d'octobre, à refaire dans le nouveau moteur.

- **Chaque perte est comptée et lisible :** commande perdue, rendu refusé, bloc
  lu en trop ou en moins.
- **Chaque frontière vérifie ce qu'elle reçoit.**
- **Une session longue** : huit heures de lecture, sans fuite ni dérive.
- **Le budget du fil audio**, mesuré par voix sur la machine la plus faible
  visée.
- **Fini quand :** la session longue passe, et la charge audio est connue et
  affichée.

### A10. Les sorties

- **Colonne → piste.** Le master n'est pas une destination d'aperçu valable :
  REAPER répond « succès » et ne joue rien. Il faut une piste relais.
- **Les niveaux de chaque colonne** sont publiés pour les vumètres.
- **Fini quand :** chaque colonne sort dans sa piste, et ses niveaux
  s'affichent sans retard visible.

---

## Phase B — Le moteur graphique

### B1. Les fenêtres natives et l'ancrage

- **L'ancrage :** `DockWindowAddEx`, `DockWindowActivate`, `DockIsChildOfDock`.
  La place est mémorisée par REAPER. Les screensets sont gérés.
- **Le menu « Dock / Undock »** à l'appui du clic droit sur la barre de titre :
  Windows ouvre le sien à l'appui, et l'ancien menu restait caché derrière.
- **Le clavier dans une fenêtre ancrée** : les « cinq portes » de l'ancienne
  suite.
  - REAPER reprend le focus 1 ms après un clic : on redirige les touches.
  - Une vue hébergée rend à REAPER les touches qu'elle n'utilise pas.
  - Le dernier clic l'emporte sur le focus.
- **Le double-clic** : la classe de fenêtre doit le demander (`CS_DBLCLKS`).
- **Un écran ou plusieurs**, et la mise à l'échelle par écran (DPI).
- **Une fenêtre cachée ne dessine plus**, mais sa logique continue : la Session
  relance ses cases sous un onglet caché.
- **Fini quand :** chaque fenêtre s'ancre partout, garde sa place au
  redémarrage, et le clavier et le double-clic y marchent comme en flottant.

### B2. Le dessin

- **Le portage du rastériseur de l'ancien C++.** Les sources sont retrouvées en
  entier : rectangles, rectangles arrondis à couverture exacte, disque, anneau,
  trait épais, polyligne, copie d'image. Il est comparé à l'ancien au pixel
  près.
- **Les formes à ajouter :**
  - courbes de Bézier ;
  - polygones remplis ;
  - dégradés linéaires et radiaux ;
  - ombres et halos ;
  - transformations (déplacer, agrandir) ;
  - calques hors écran (les « cuissons »).
- **La pile de découpes et la transparence** (alpha prémultiplié).
- **Les tests :** des vérités géométriques, pas des comptes d'appels. Par
  exemple, la somme des couvertures d'un disque doit valoir πr².
- **Fini quand :** chaque forme de l'ancien moteur sort identique au pixel. Et un
  widget nouveau se dessine avec les seules formes du moteur.

### B3. Le texte

- **Un atlas de glyphes** (l'ancien `cp_text.cpp` est retrouvé). Le rendu des
  lettres se fait par GDI ou par DirectWrite (décision à prendre à la mesure de
  la netteté).
- Les accents, la mesure, l'ellipse (« … »), le texte sur plusieurs lignes, et
  un champ de saisie : curseur, sélection, copier-coller.
- **Fini quand :** le texte est net à toute taille et à toute mise à l'échelle,
  et la saisie marche en AZERTY.

### B4. Les images et les icônes

- Le décodage PNG et JPG par Windows (WIC, sans dépendance).
- Les icônes Lucide, qui ont survécu : tracées en vecteur par le moteur, ou
  pré-cuites à chaque taille.
- **Fini quand :** toute icône est nette à toute taille.

### B5. La fluidité au taux de l'écran

- **Ne repeindre que ce qui change** (l'« endommagement »). Sans ça, 120 Hz
  coûtait la machine dans l'ancienne suite.
- **Un fil de présentation calé sur l'écran.** Il présente l'image au moment où
  l'écran la prend. Les animations se calculent sur l'heure de présentation, pas
  au nombre d'images.
- **Le chemin de présentation** (décision à la mesure) : celui de l'ancienne
  suite, une copie GDI calée sur `DwmFlush` et éprouvée. Ou le modèle « flip »
  de DXGI, plus régulier et compatible avec les écrans à taux variable.
- **Fini quand :** le compteur d'images ratées reste à zéro pendant les gestes
  de 10 s, au rafraîchissement de l'écran de Cedric. Et la latence souris →
  écran est mesurée à une image au plus.

### B6. L'entrée

- **La souris :** capture, double-clic, molette verticale et horizontale, pavé
  tactile précis, curseurs.
- **Le clavier :** les caractères par `WM_CHAR` (AZERTY), les raccourcis sans
  voler ceux de REAPER.
- **Le glisser-déposer :** depuis l'Explorateur, depuis REAPER, et entre nos
  fenêtres.
- **Les menus contextuels, les infobulles, les boîtes de fichiers.**
- **Fini quand :** chaque geste de l'ancienne suite marche. Et aucun clic ne se
  perd : l'ancienne suite en perdait sur les fenêtres ancrées.

### B7. Le moniteur de performance

- Une fenêtre qui affiche :
  - le temps de chaque image ;
  - les images ratées ;
  - la surface repeinte ;
  - la charge du fil audio ;
  - les pertes comptées.

  Construite dès le début, parce qu'on ne règle pas ce qu'on ne voit pas.

### B8. Si un jour il le faut : le dessin par la carte graphique

- Si le dessin par le processeur n'atteint pas l'exigence 2 sur un grand écran
  à haut taux, on ajoute un chemin Direct2D ou Direct3D. Le chemin processeur
  reste la référence et la solution de repli.

---

## Phase C — Le toolkit

- **C1. La mise en page :**
  - la grammaire zones / barres / coutures de l'ancienne suite ;
  - les panneaux redimensionnables ;
  - le défilement net, sans glisse ;
  - les en-têtes épinglées.
- **C2. Les widgets :**
  - bouton, bascule, bouton rotatif, fader, curseur, champ numérique à glisser ;
  - champ texte, liste déroulante, menu, onglets ;
  - liste et arbre virtualisés, tableau, barre de défilement, infobulle ;
  - vumètre ;
  - forme d'onde, règle et grille avec magnétisme ;
  - base de piano roll, éditeur d'enveloppe, pad XY.
- **C3. Le thème :** un fichier relu en direct (couleurs, tailles, polices), et
  des accents par application.
- **C4. Les raccourcis :** décrits comme des données, rangés par action (pas par
  touche), et intégrés à la liste d'actions de REAPER.
- **C5. Annuler / refaire :** l'undo de REAPER pour ce qui touche le projet, un
  undo interne pour l'état de l'interface.
- **C6. La persistance :** réglages par application, état par projet,
  disposition des fenêtres.
- **Fini quand :** on reconstruit une fenêtre de l'ancienne suite sans toucher
  au moteur de dessin.

---

## Phase D — Les fondations REAPER

Elles sont transverses : on les construit au fur et à mesure des besoins, et ici
tout est listé.

- **D1. L'API de REAPER dans `cp_sys` :** pistes, items, prises, sources,
  enveloppes, effets, MIDI, marqueurs, carte de tempo, transport. Avec les
  pièges connus :
  - un `ReaProject*` désigne l'onglet, pas le projet ;
  - `GetPlayPosition` et `GetPlayPosition2` ne disent pas la même chose ;
  - les paramètres des effets Cockos sont normalisés entre 0 et 1 ;
  - les formes de curseur des JSFX ont leurs propres règles.
- **D2. Les notifications :** changement de projet, d'onglet, de transport, de
  sélection.
- **D3. Les fils de travail :** décodage, pics, analyse, cuissons. Aucun appel
  interdit à REAPER hors du fil principal.
- **D4. Le MIDI matériel :** entrée et sortie.
- **D5. Les effets maison :**
  - **Ils restent en JSFX**, parce qu'une extension ne peut pas se placer dans
    une chaîne d'effets. Leurs sources sont rapatriées dans le dépôt (copie sur
    E:, `reaper_installed\Effects`).
  - **Leur vue sur mesure peut s'incruster dans la fenêtre d'effet** grâce à
    `reaper_plugin_fx_embed.h`.
- **D6. Les petits scripts Lua indépendants** (Various, Media Properties
  Toolbar…) restent tels quels, hors de CP Studio.

---

## Phase E — Les applications

**La première est le navigateur de fichiers** (décidé par Cedric le
2026-11-21). Il met à l'épreuve, à lui seul, toutes les exigences :
- l'écoute en flux ;
- la sélection minuscule ;
- la forme d'onde à tout zoom ;
- les longues listes fluides ;
- l'ancrage.

L'ordre des suivantes reste à décider.

### Jalon 1 — le navigateur de fichiers, utilisable au quotidien

**Ce qu'il faut pour y arriver** : A1, A2, A3 ; B1 à B7 ; de C, ce que le
navigateur utilise ; de D, ce que le navigateur appelle.

**Ce qu'il fait (V1).** C'est l'ancien navigateur (mémoire
`project-media-explorer`), plus ce que le natif permet enfin.

- **L'arborescence :**
  - les dossiers et les raccourcis de l'Explorateur de REAPER, lus dans
    `reaper.ini`, section `[reaper_explorer]` ;
  - le dépliage en ligne ;
  - les en-têtes de dossier qui restent collés en haut, empilés. Aucun DAW ne
    le fait.
- **L'écoute instantanée à la sélection**, par notre moteur, en flux :
  - arrêt, boucle, volume d'écoute ;
  - écoute au relâchement du clic : un glisser reste muet dès le premier pixel ;
  - sortie par une piste relais : le master n'est pas une destination d'aperçu
    valable.
- **La forme d'onde du fichier choisi, à tout zoom :**
  - la sélection d'une section, même minuscule ;
  - l'écoute ne joue que la section, à l'échantillon près.
- **Le glisser :**
  - vers REAPER, par un VRAI glisser-déposer Windows : REAPER pose l'item là où
    on lâche, avec la section choisie ;
  - aussi vers d'autres logiciels. C'était impossible depuis Lua.
- **La recherche** dans les noms et dans les bases MediaDB de REAPER ; les
  favoris.
- **Le clavier complet.** Ancrable partout.

**V2, juste après.** Ce que l'ancien navigateur avait aussi :
- les collections colorées (1 à 7) ;
- le hasard (R) ;
- le calage au tempo : tempo lu dans le nom, `GetTempoMatchPlayRate`, ×0,5 / 1
  / 2. En changeant la hauteur d'abord, puis en étirant quand A5 arrive ;
- le remplacement à chaud de la source de l'item choisi (Q, Échap pour revenir) ;
- la vue « samples », une forme d'onde par ligne ;
- les effets comme racine virtuelle (insertion d'un effet).

**Fini quand :** Cedric s'en sert comme navigateur au quotidien, ancré dans
REAPER, et en fait un vrai retour.

| application | ce qu'elle fait | où est sa spécification |
|---|---|---|
| Navigateur de fichiers (Media Explorer) | parcourir, écouter, glisser dans REAPER, à la FL Studio | mémoire `project-media-explorer`, `reference-reaper-browser-scripts` |
| Éditeur de clips | audio et MIDI, piano roll, enveloppes, établi d'un son | mémoires `project-sampler`, `project-editor-sound-workbench`, `project-midi-command-layer` |
| Session | lanceur de clips, prises, vers l'arrangeur, sortie MIDI par colonne | mémoires `project-session-*`, `ROADMAP_Chantiers.md` (ancien dépôt) |
| Sampler | kits de pads, un effet par kit | mémoires `project-kit-sampler-jsfx`, `project-sampler` |
| Transport | le transport à la place de celui de REAPER, clock externe | mémoires `project-transport-custom`, `project-ext-clock` |
| FX Constellation | moteur de modulation, pad, graphe, liens | mémoires `project-fx-constellation-refonte`, `project-moteur-modulation-plan`, `ROADMAP_FXConstellation.md` |
| Séquenceur / Sound Gen | séquenceur à la Pigments, synthé JSFX | mémoires `project-sequenceur`, `project-soundgen-modulation`, `ROADMAP_Sequenceur.md` |
| Vidéo | caméra, stop motion, calage par le son | mémoire `project-video`, `ROADMAP_Video.md` |
| Guitar Tab | audio → tablature dans MuseScore | ce dossier n'existe pas encore ; le portage de Basic Pitch est dans la transcription du 2026-11-20 |
| ChordLab, PaintSynth, barres d'outils… | les plus petits outils | mémoires et anciennes ROADMAP |

Pour chaque application, une ROADMAP à elle (`ROADMAP_<Nom>.md`) sera écrite au
moment de la commencer.

---

## Phase F — La livraison

- **F1. L'installation sur une machine neuve :** une commande, ou ReaPack.
- **F2. Les versions,** et la reprise des données des anciens projets (formats
  de session, réglages) quand une application revient.
- **F3. Le test sur la machine la plus faible visée :** les budgets mesurés, pas
  supposés.
- **F4. La documentation pour l'utilisateur.**

---

## L'ordre proposé

1. **A1**, le premier son. Le moteur est prêt, il ne manque que le branchement.
2. **B1, B2, B5, B7** : une fenêtre ancrable, le dessin, la fluidité et sa
   mesure.
3. **B3, B4, B6** : le texte, les images, l'entrée.
4. **A2, A3** : le vivier complet et l'exactitude à 100 %.
5. **C** : le toolkit, construit pendant la première application et pour elle.
6. **Le navigateur de fichiers** : V1, puis V2. C'est la fin du jalon 1.
7. **A4, A5** : la vitesse et l'étirement, quand l'éditeur arrive.
8. **A6, A7, A8** : lanes, clock, enregistrement, quand la Session arrive.
9. **A9, A10, F** : robustesse, sorties, livraison, tout au long.

---

## Les décisions ouvertes

| décision | ma recommandation |
|---|---|
| où vit le moteur audio | dans la petite DLL fixe (une retouche d'interface ne coupe jamais le son) |
| comment parler aux classes C++ de REAPER (`PCM_source`, `IReaperPitchShift`) | nos propres tables virtuelles dans `cp_sys`, vérifiées par un programme C++ ; reaper-rs en repli |
| l'étirement | cuit par défaut, en temps réel pour ce qui change en jouant |
| le chemin de présentation | celui de l'ancienne suite d'abord (GDI + `DwmFlush`), DXGI si la mesure le demande |
| le rendu du texte | à trancher à la mesure de la netteté (GDI ou DirectWrite) |
| ~~la première application~~ | **tranché** : le navigateur de fichiers (Cedric, 2026-11-21) |
| l'application suivante | à décider après le retour de Cedric sur le jalon 1 |

## Ce que CP Studio ne fera pas

- **Pas de Lua.** Les petits scripts indépendants existants restent tels quels.
- **Les effets restent en JSFX.** Une extension ne peut pas se placer dans une
  chaîne d'effets. Un plugin CLAP serait la voie si un jour il le faut.
- **Pas de macOS ni de Linux pour l'instant.** `cp_engine` ne dépend de rien de
  Windows, donc la porte reste ouverte.
