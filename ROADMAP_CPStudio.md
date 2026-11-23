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
| ✅ B1 (en partie) : fenêtres flottantes ou ancrées, clavier des fenêtres ancrées (crochet « accelerator » + dernier clic), double-clic ; ouvrir, ancrer et fermer se font hors du message de la fenêtre et jamais au démarrage de REAPER | 2026-11-21 | auto-test : le navigateur s'ancre dans un docker et en revient, les touches redirigées arrivent, un rechargement à chaud garde la fenêtre et la sélection ; reste : screensets, DPI par écran testé, le glisser de l'onglet entre dockers à la souris |
| ✅ B2 (base) : `cp_gfx`, rastériseur à couverture exacte, chemins, Bézier, traits, dégradés, transparence, découpage au pixel | 2026-11-21 | vérités géométriques (disque = πr², rectangle fractionnaire = son aire), planches PNG |
| ✅ B3 (base) : texte par GDI, cache de glyphes, mesure, « … » | 2026-11-21 | captures du vrai REAPER |
| ✅ B4 (base) : icônes Lucide en vecteur (SVG avec arcs) | 2026-11-21 | test : chaque icône se dessine dans son carré |
| ✅ B5 : horloge calée sur le vrai rafraîchissement de l'écran (`D3DKMTWaitForVerticalBlankEvent` ; `DwmFlush` s'arrêtait sur un écran immobile) ; ne repeindre que ce qui change (la forme d'onde seule pendant l'écoute) et n'envoyer à l'écran que les rangées changées ; une garde laisse toujours au moins la moitié du fil principal à REAPER | 2026-11-21 | vrai REAPER, ancré en 870 × 1298 : 0,4 ms par image d'animation, 1,8 ms par redessin complet, battement de REAPER 42 ms au pire (le gel signalé par Cedric : 32 s) ; reste : le geste de 10 s compté à la main, la latence souris → écran |
| ✅ B6 (base) : souris (capture, double-clic, molette verticale et horizontale, curseurs), caractères par `WM_CHAR` (AZERTY), menus contextuels natifs, presse-papiers, glisser-déposer Windows vers REAPER, infobulles natives (CP Transport) | 2026-11-21, infobulles le 2026-11-22 | tests ; vrai REAPER pour le clavier ; glisser depuis l'Explorateur vers le navigateur et Guitar Tab, infobulles du navigateur (2026-11-23) |
| ✅ B7 (base) : mesures à l'écran (temps d'image, cadence, images sautées) | 2026-11-21 | action « Performance overlay » |
| ✅ A2 (base) : écoute en flux (jouable dès la première seconde décodée), pics à plusieurs résolutions | 2026-11-21 | tests ; limite 1,5 Go par fichier (23 min stéréo 48 kHz) ; reste : le vrai flux disque pour les fichiers de plusieurs heures |
| ✅ A3 (premier critère) : une section de N échantillons rend N échantillons, N de 1 à 4096 | 2026-11-21 | test `cp_audio` |
| ✅ Le navigateur V1 : arbre des raccourcis, en-têtes collés empilés, écoute au relâchement, forme d'onde à tout zoom (64 px par échantillon), section, insertion et glisser-déposer avec section, recherche (noms + MediaDB), favoris, clavier, ancrable | 2026-11-21 | tests avec le vrai moteur et les vraies polices ; vrai REAPER (insertion exacte, fenêtre, ancrage) |
| ✅ Le navigateur V2 : collections colorées 1 à 7, hasard (R), calage au tempo (×0,5 / 1 / 2, par la hauteur), remplacement à chaud de la source (Q, Échap), vue « samples », effets comme racine virtuelle | 2026-11-21 | vrai REAPER : 1 615 effets listés, tempo du projet lu, pic de la miniature = pic du fichier ; reste : rangées de hauteur variable dans la vue « samples », l'étirement (A5) pour caler sans changer la hauteur |
| ✅ Premier retour de Cedric : « stable et fonctionnel, barebone mais c'est là », sauf un gel de REAPER en mode ancré | 2026-11-21 | gel trouvé et corrigé à la racine (voir B5), l'auto-test mesure désormais ce cas |
| ✅ Les places natives (ABI 6) : une vue de CP Studio à la place d'une vue de REAPER (l'arrangement, le transport), tenue et rendue par la DLL fixe | 2026-11-22 | vrai REAPER : CP Transport posé sur le transport natif masqué (2560 × 36 en 0,0) et lui cède la place quand Cédric le rallume ; le navigateur à la place de l'arrangement (2557 × 1310), clavier, rechargement à chaud, Arrange / Browser |
| ✅ CP Transport : transport, compteur, tempo (glisser, molette, saisie), signature, Tap, métronome, vitesse, icônes d'actions (ajouter par la liste des actions de REAPER, retirer, déplacer), sélecteur de vues avec la place de chaque vue | 2026-11-22 | 6 tests et planches PNG ; démarre seul avec REAPER |
| ✅ CP Guitar Tab en Rust : l'item choisi devient une piste MIDI (pour comparer à l'oreille) et une partition MusicXML (portée et tablature, rythme calé sur le tempo du projet, marteaux et tirés), ouverte dans MuseScore ; réglages (seuils, note la plus courte, grille, accordage) ; icône dans CP Transport | 2026-11-22 | réseau à 3,8e-5 d'onnxruntime ; vrai REAPER : un riff de 8 noires retrouvé note pour note en 0,5 s |
| ✅ Le mode sombre de CP Studio, la part sûre : barres de titre (DWM) et menus (mode d'application de Windows), jamais le contenu des boîtes ; éteint par défaut, action « CP Studio: Dark title bars and menus » | 2026-11-22 | vrai REAPER : la boîte de fichiers « Load Script » ouverte 5 s mode sombre allumé, sombre et sans plantage (l'extension « Reaper Dark Mode » y faisait planter SHELL32 depuis la mise à jour de Windows du 21) |
| ✅ La nuit du 22 au 23 novembre (autonomie complète) : le moteur audio complété (A2 lecture en flux, A4, A5 cuit et temps réel mesuré, A7 avec la phase, A8 audio et MIDI, A9, A10, fondu de boucle), Guitar Tab complété (triolets, deux voix, armure, techniques de jeu, noms d'accords, rythme sous la tab, chiffrages qui changent, reprises), l'éditeur (bends qui suivent leur note, accords, arpèges, euclides, saisie pas à pas), les onglets des dockers cachés dès le démarrage | 2026-11-23 | `cargo xtask reaper-test` PASS dans le vrai REAPER après chaque morceau ; détails dans chaque section ci-dessous |
| ▶ **Maintenant : le jalon 1 à l'usage.** Cedric s'en sert au quotidien ; on corrige et on polit à partir de ses retours. Ensuite, le choix de l'application suivante | | |

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
- ✅ **La lecture en flux, faite le 2026-11-23** (`cp_sys::paged`,
  `cp_audio::stream`) : au-delà de 256 Mo décodés (11 min en stéréo à
  48 kHz), un fichier est coupé en pages d'une seconde ; un fil de flux charge
  celles qui sont devant chaque tête (une par voix, dans son sens) et lâche
  celles qui sont loin ; le début reste toujours. Le fil audio ne libère ni
  n'attend rien (une page lâchée n'est libérée que quand aucune lecture ne la
  tient). Plus de limite de longueur. Deux heures prêtes en 0,4 ms, 10 Mo
  en mémoire au lieu de 2,8 Go ; aucun bloc sans sa page en temps réel avec
  des sauts au hasard. Vrai REAPER, 12 min de WAV : la bonne seconde entendue
  après deux sauts, 10 Mo, pics complets. L'oracle reste identique au bit.
- ✅ **Relu à part le 2026-11-23** (une relecture indépendante du code) :
  aucun défaut de mémoire ; des défauts dans le choix des pages, corrigés et
  gardés par des essais au rythme de REAPER (à l'envers : 13 615 pages posées
  en 2 s → 13 ; deux voix d'un même clip ; une boucle au milieu d'un long
  fichier ; une source plus courte qu'annoncée ; deux départs coup sur coup).
- Reste : un flux qui cuit (l'élastique) ne se fait pas sur un clip en flux
  (refusé, en le disant ; le navigateur se cale alors par la vitesse).
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
- ✅ **Fait le 2026-11-23** (`cp_engine::sinc`, `Param::Interp` / `Reverse`) :
  sinc à fenêtre de Kaiser (16 passages par zéro, le noyau s'élargit au-delà
  de la vitesse 1) ; un 18 kHz joué à 2 : −113 dB (Hermite : 0 dB, tout se
  repliait) ; un 12 kHz à 0,75 : erreur −109 dB (Hermite −21 dB) ; à
  l'envers, N échantillons rendus au bit près, N = 1 compris ; dix minutes
  à 1,37 : 1,7e-5 échantillon d'écart ; 1,6 % d'un cœur par voix stéréo.
  Hermite reste le défaut (l'oracle compare toujours au bit près). Le
  navigateur écoute en sinc dès que sa vitesse change.

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
- ✅ **Cuit, fait le 2026-11-23** (`cp_sys::pitch`, `cp_audio::stretch`, ABI 8) :
  l'étireur de REAPER lui-même (`IReaperPitchShift`, élastique 3.3.3 Pro chez
  Cedric), appelé par sa table virtuelle vérifiée contre le vrai compilateur.
  Durée exacte à l'échantillon ; le retard de l'étireur est mesuré, puis
  retiré. Vrai REAPER : 2 s à 0,75 → 128 000 frames exactement, attaques à
  0,33 ms de leur place en médiane (3,3 ms au pire, le lissage d'élastique) ;
  +3 demi-tons → 522,5 Hz pour 523,3. Le navigateur cale au tempo SANS
  changer la hauteur (vitesse le temps de la cuisson, puis la version étirée
  au même endroit).
- ✅ **En temps réel, fait le 2026-11-23** (`cp_engine::shift`, `Param::Tempo` /
  `Shift`) : une voix emprunte un étireur à son premier tempo ou à sa
  première hauteur, et le rend en finissant. Le fil audio n'attend jamais
  (`try_lock`) et ne libère jamais rien : le vivier du fil principal (16 au
  plus) reste le dernier détenteur. Vrai REAPER : tempo 0,8 et +4 demi-tons
  sur une voix qui joue dans une piste, sans aucun rendu refusé.
- ✅ **Mesuré dans le vrai REAPER le 2026-11-23** (un second moteur rendu
  hors ligne, le vrai élastique) : la première attaque à -0,77 ms de sa
  place, les autres à 0,77 ms en médiane, 2,06 ms au pire ; le même départ
  daté dans 0,3 s donne exactement les mêmes écarts : l'amorçage ne décale
  rien, la compensation prévue n'est pas nécessaire. +4 demi-tons : 554,3 Hz
  pour 554,4.
- ✅ **Relu à part le 2026-11-23** : aucun défaut de mémoire ; corrigés : un
  étireur gardé par une voix annulée ou coupée net (au-delà de 16, le tempo
  d'une voix neuve était ignoré), le passage par tempo 1 en jouant (un saut
  et une réserve périmée), le retournement en jouant, une vitesse infinie.
- ✅ Un saut ou un relancement d'une voix étirée croise désormais 5 ms de
  l'ancienne sortie de l'étireur vers la nouvelle (essai : un pas de 0,463
  sans, 0,0053 avec, la pente naturelle du sinus).
- Reste : l'entendre (Cedric) ; les enveloppes d'une voix étirée avancent par blocs
  de 512 et la fin d'ADSR se cale sur la matière lue, pas entendue ; un
  retournement en jouant repart de la position lue (en avance de la latence
  de l'étireur) ; seize voix étirées lancées sur le même temps s'amorcent
  dans le même bloc.

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
- ⚠ **Constaté le 2026-11-23** : la fin de `cp_lanes.cpp` (au-delà de la ligne
  1249 : la fin de `run_gate` et tout `Lanes::tick`, la boucle qui fait
  tourner les lanes) n'est ni dans les copies, ni dans l'historique de
  fichiers de Claude, ni dans les transcriptions (cherché). Le port ne peut
  donc pas être comparé à l'ancien : les lanes seront à repenser avec la
  Session, avec Cedric.

### A7. La clock MIDI externe

- Son code de référence a été retrouvé en entier et prouvé identique
  (`cp_extclock`). On le porte, puis on le compare à l'ancien.
- **Fini quand :** l'oracle passe, et CP Studio se cale sur une machine externe.
- ✅ **Le calcul, fait le 2026-11-23** (`cp_engine::extclock`) : identique au bit
  à l'ancien, sur des scénarios (tempo bruité, 124 → 128, impulsions perdues,
  140 → 70, Start / Stop / Continue / SPP, doublons, changement de taux) et
  300 graines au hasard.
- ✅ **Le branchement, fait le 2026-11-23** (ABI 10) : l'entrée MIDI lue dans le
  crochet audio (`cp_sys::midi_in`, tables vérifiées contre le compilateur) ;
  CP Transport, clic droit, « External MIDI clock: <entrée> » : le tempo du
  projet suit le maître au centième, REAPER part à sa première impulsion et
  s'arrête à son Stop. Vrai REAPER : la vraie entrée MIDI 0 lue dans le
  crochet sans incident.
- ✅ **La phase, faite le 2026-11-23** : une sonde dans le vrai REAPER a
  montré que changer le TEMPO en jouant fait sauter la position (-0,2 noire,
  70 ms d'arrêt pour +1 %) alors que la VITESSE de lecture change sans
  heurt. Le tempo ne change donc plus qu'à l'arrêt ; en jouant, une PLL sur
  la vitesse (rapport des tempos + correction en 2 s, bornée à 2 %), calée
  sur la mesure du maître, latence de sortie comptée, retard du maître
  réglable (« Clock delay », -20 à +20 ms). Simulée : 0,65 ms d'écart après
  5 s, 0,63 ms pendant une rampe 128 → 132 BPM.
- ✅ **Relu à part le 2026-11-23**, corrigé : un module rechargé perdait le
  verrou (la clock remise à zéro) ; un changement de tempo du maître
  relâchait le verrou (la vitesse allait et venait) ; le tempo n'était
  reposé qu'au changement du maître ; un projet très loin du maître jouait à
  une vitesse de 3 (son tempo est maintenant posé une fois) ; les sauts se
  répétaient sans fin (trois au plus, jamais en enregistrant, seuil d'un
  quart de noire) ; la base prend le tempo là où l'on joue ; la vitesse
  d'avant rendue après un rechargement ou quand la clock est coupée.
- Reste : l'essai avec une vraie machine (et le réglage du retard à
  l'oreille) ; la vitesse d'avant n'est pas rendue si REAPER se ferme en
  jouant synchronisé, ni au projet d'avant si l'on change d'onglet en
  jouant.

### A8. L'enregistrement

- **La prise audio dans une case** : capture native, latence compensée, rejouée
  dès la fin de la passe.
- **La prise MIDI dans une lane.**
- **Fini quand :** une prise tombe à sa place à l'échantillon, mesurée contre une
  boucle de retour.
- ✅ **La prise audio, faite le 2026-11-23** (`cp_audio::record`) : l'entrée de
  la carte copiée dans un anneau par le crochet, chaque bloc daté ; une prise
  commence et finit à des temps du moteur, la latence d'entrée + de sortie
  retirée (elle tombe là où elle a été jouée). Vrai REAPER : une
  demi-seconde → 24 000 frames exactement, latence 147 + 171 retirée.
- ✅ **La prise MIDI, faite le 2026-11-23** (`cp_audio::midirec`) : les
  messages courts des entrées datés à l'échantillon dans le crochet, la
  latence de sortie retirée, une entrée ou toutes, les notes tenues relâchées
  à la fin. Vrai REAPER : une prise de toutes les entrées finie à l'heure,
  rien de perdu.
- ✅ **Dans l'ABI 12** (2026-11-23) : les prises audio et MIDI, le temps du
  moteur et le lancement daté sont accessibles aux applications (la Session
  n'aura pas à attendre une réinstallation du chargeur).
- Reste : la mesure contre une boucle de retour (un câble) ; le branchement
  dans une application (la Session).

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
- ✅ **La session longue, faite le 2026-11-23** : huit heures de son tirées comme
  REAPER, avec lancements, arrêts, réglages et clips chargés puis libérés au
  hasard : aucune perte comptée, aucune fuite, aucune dérive (0,73 % d'un
  cœur par voix). Une minute très chargée (40 voix) tourne dans la suite des
  tests. Elle a trouvé un vrai défaut (un faux départ de l'horloge au
  démarrage, corrigé).

### A10. Les sorties

- **Colonne → piste.** Le master n'est pas une destination d'aperçu valable :
  REAPER répond « succès » et ne joue rien. Il faut une piste relais.
- **Les niveaux de chaque colonne** sont publiés pour les vumètres.
- **Fini quand :** chaque colonne sort dans sa piste, et ses niveaux
  s'affichent sans retard visible.
- ✅ **Fait le 2026-11-23** (ABI 9) : un port joue DANS une piste
  (`PlayTrackPreview2Ex`) ; une piste effacée en pleine lecture le renvoie à
  la sortie matérielle sans que REAPER tombe ; les crêtes de chaque port sont
  publiées à chaque bloc. Vrai REAPER : crête de la piste = crête du port.
- En plus : le **fondu enchaîné au point de boucle** (`Param::LoopXfade`) : une
  boucle qui ne se raccorde pas ne claque plus (saut de 0,836 → 0,066).
- ✅ **Relu à part le 2026-11-23** : les aperçus de piste étaient lancés et
  arrêtés dans « l'onglet devant » ; changer d'onglet pendant qu'une colonne
  jouait dans une piste laissait le registre dans la liste du premier onglet
  (lu après libération à la fermeture). Ils retiennent leur projet, et une
  barrière attend la fin d'une lecture avant de libérer. Mesuré :
  `StopTrackPreview2` rend 1 sur une piste vivante, 0 sur une piste effacée.
- Reste (relecture) : une piste neuve à l'adresse exacte d'une piste effacée
  passerait pour elle (comparer son GUID) ; le nombre d'entrées de la carte
  n'est relu qu'au début d'une prise (un autre périphérique avec moins
  d'entrées).

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
- **La poignée des vues ancrées** (2026-11-23) : points, icône et nom à gauche
  de la barre ; tirée, elle passe le glisser à REAPER (un appui posté à la
  poignée native du docker, son `WDLTabCtrl`, mesuré) ; montrée quand REAPER
  cache ses onglets (réglage auto / always / never dans le menu de la vue).
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

### Guitar Tab v2 — des pistes de guitare (en cours, 2026-11-22)

La V1 (une prise, sa partition et sa tab) est construite et passe l'auto-test.
Le premier usage de Cedric a montré qu'il faut un vrai système de pistes :
enregistrer, effacer, recommencer, **sans jamais retourner ranger
l'arrangement** (aucune « dette » d'organisation). Maquette v2 :
https://claude.ai/artifact/3Gmk5y4SXg7x7qJ1sPdhiD.

Ce que Cedric a tranché le 2026-11-22 :
- **Une piste Guitar Tab = un dossier de deux pistes REAPER** : la guitare
  (audio) et sa tab (MIDI, avec le synthé de Guitar Tab).
- **Des pistes hautes** avec des couches alignées sur les mêmes mesures :
  audio, MIDI, partition, tab. Par défaut : audio, partition et tab ; le MIDI est
  caché.
- **Réenregistrer au même endroit suit les réglages de REAPER** : des prises
  empilées ou des lanes, selon ce que REAPER fait déjà.
- **Guitar Tab ne montre que ses propres pistes.** Une piste existante y entre
  par un clic droit.
- **Un double-clic sur le MIDI ouvre l'éditeur MIDI** (celui de REAPER, puis le
  nôtre quand il reviendra, voir plus bas).
- **Le son de la tab** : un JSFX maison posé sur les NOUVELLES pistes tab, avec
  quatre sons, Guitar, Piano, Sine et Bass.
- **L'accordeur dans CP Transport, réglable** : soit à la place du compteur
  quand le transport est à l'arrêt, soit dans un emplacement à lui (au centre,
  où il y a de la place). Il lit l'entrée de la carte son dans le crochet audio
  du chargeur.

Ce que Cedric a tranché le 2026-11-22 après-midi (construit le même jour) :
- **La transcription se fait quand il la demande** : la baguette d'une prise,
  l'action, ou un bouton de transcription automatique (coupé par défaut).
- **Une fois transcrit, le MIDI fait foi.** Il s'éloigne de l'audio, et c'est
  voulu : une note effacée dans le MIDI disparaît de la tab et ne revient pas
  (ni fantôme, ni décodage) ; rien ne se retranscrit tout seul.
- **Les prises se déplacent comme dans l'arrangement** : on voit l'item (son
  onde, son MIDI, sa tab) bouger, son début se cale par le magnétisme de
  REAPER, Ctrl copie.
- **Des fichiers audio se déposent dans Guitar Tab** depuis le navigateur.

**Complété dans la nuit du 22 au 23 novembre** (demande de Cedric : « complète
Guitar Tab pour le rendre complet ») :
- **les triolets** : chaque temps choisit entre la grille droite et trois
  croches par noire, selon les attaques jouées ; le 3 et son crochet, en
  MusicXML aussi ;
- **deux voix** : une basse tenue sous une mélodie n'est plus raccourcie (hampes
  en bas) ; des accords qui résonnent restent une voix ;
- **l'armure** : la tonalité devinée sur toute la guitare, les notes nommées
  dedans (Si bémol en Fa), les altérations jusqu'à la barre ;
- **les techniques de jeu** : le vibrato détecté dans la hauteur fine ; la
  palette pose P.M., note morte, harmonique, let ring, vibrato ; dessinés,
  gardés, écrits en MusicXML ;
- **les noms d'accords** au-dessus de la portée, à chaque changement ;
- **le rythme sous la tablature** quand la partition est cachée (comme Guitar
  Pro), dans la fenêtre et le PDF.
- **les chiffrages qui changent** (2026-11-23) : les mesures suivent celles
  de REAPER (un 3/4, un passage en 6/8, une mesure partielle) à l'écran, dans
  la partition, le PDF et le MusicXML ; les ligatures par temps de la mesure
  (noire pointée en 6/8) ; le tempo n'y change rien (tout est en noires) ;
- **les reprises** (2026-11-23) : un passage rejoué tel quel s'écrit une fois
  entre barres de reprise, « ×3 » au-dessus, dans le PDF et le MusicXML
  (menu de la piste : « Repeats in exports ») ;
- **les changements de tempo** (2026-11-23) : écrits là où ils tombent (une
  noire et sa valeur au-dessus de la portée, `<metronome>` en MusicXML) ;
  une mesure où le tempo change ne se reprend pas.
- **relu à part** (2026-11-23) : des notes qui sortaient de la page avec
  plusieurs reprises dans un système, une note perdue par la compaction
  avec des triolets, un triolet coupé par une barre de 7/8, des
  mesures-copeaux, l'ordre du MusicXML : corrigés, chacun avec son essai.
- **le jeu libre** (2026-11-23, pour une prise ad lib) : un bouton par
  guitare ; aucune ligne verticale, la règle en secondes, chaque note à sa
  date, la corde allumée le temps de la note, le magnétisme sur les attaques
  de l'audio, un accord = des notes à 30 ms ; le PDF sans barre (à la ligne
  dans un silence), le MusicXML une mesure par phrase, chiffrage caché.
  Reste : si un tempo est posé plus tard, le MIDI suit les noires et l'audio
  non (pas encore traité).
- **des notes choisies ensemble** (2026-11-23, demande de Cedric) : le clic
  droit glissé dans la partition ou la tab choisit des notes (ailleurs, des
  prises) ; Ctrl+clic, Maj+clic ; un glisser gauche les déplace ensemble
  (chacune dans son item ; vers le haut ou le bas dans la tab, elles
  changent de corde) ; Suppr les efface.
- **la palette de l'éditeur** sur ces notes (crate `cp_palette`, commune) :
  accords, arpèges, euclides, temps, vélocité ; les bends, glisses et
  liaisons de la tab ; un groupe Techniques. Une note née ou changée de
  hauteur prend un doigté autour des notes restées en place.
Reste : l'oreille et l'œil de Cedric sur ses vraies prises (les seuils
viennent de tests synthétiques).

### L'éditeur de clips revient (demande de Cedric, 2026-11-22)

**Ce qu'on avait.** CP_Editor (Lua) était complet, très fonctionnel, et Cedric
le jugeait excellent. Il n'existe plus dans la suite : il faut le réintégrer en
Rust. Il avait :
- le piano roll et la forme d'onde, audio et MIDI ;
- la couche de commandes partagée avec le Looper :
  - le modèle `Roll` (sélections, transposition, quantification avec force,
    humanize, legato, reverse, arpège, euclidien, gammes, copier-coller) ;
  - `RollUI`, un seul clavier et un seul menu pour les deux hôtes ;
- les raccourcis comme une donnée (`Keymap`), avec les six contextes MIDI de la
  configuration de Cedric reproduits fidèlement ;
- les enveloppes ADSR ou dessinées (ENV), et l'établi d'un son (recette CPC1) ;
- la probabilité par note (décidée), le curseur local à une case.

**Où sont les spécifications.**
- `ROADMAP_Editeur.md` (624 lignes), dans l'ancien dossier
  `REAPER\Scripts\CP_Scripts` :
  - §1 : ce que l'éditeur savait faire ;
  - §2 : la configuration souris de Cedric, contexte par contexte ;
  - §3 à §8 : la suite prévue.
- Les sources Lua (`CP_Editor/`, `CP_Engine/Roll.lua`, `RollUI.lua`,
  `Keymap.lua`) sont dans `E:\CP_Scripts_recovery_20261120\github_2026-08-22.git`.
  Ce dépôt est en lecture seule : on le lit pour porter, on ne le relance pas.
- La copie la plus récente de `CP_Editor.lua` (9 620 lignes, 2026-10-25) est une
  sauvegarde de l'historique de Claude, dans le même dossier de récupération :
  `claude_file_history\90a0a624-…\c7d828512c4ee59e@v5`. `Roll` et `RollUI`
  n'existent qu'à la date du 2026-08-22.
- Perdus : `CP_Editor/Modules/AutoLane.lua`, le `Keymaps/editor.lua`
  d'octobre, `CP_Engine/Env.lua`.
- `Recherche/INVENTAIRE_CP_Editor.md` et `CONFRONTATION_CP_Editor.md` ont
  survécu (2026-08-01 ; leurs numéros de ligne visent l'ancien fichier).
- Les mémoires `project-midi-command-layer`, `project-keymap`,
  `project-editor-sound-workbench`.

**Le piège pour Guitar Tab.** L'ancien `Roll` écrivait toute note neuve sur le
canal 1 : dupliquer, coller, accords, glue, subdiviser, arpéger remettaient le
canal à 1. Dans Guitar Tab le canal est la corde. Le port garde le canal de
chaque note, et un test le tient.

**La maquette (2026-11-22).** Deux vues du même clip, sans phrase à l'écran :
- « Fit », par défaut : la vue cadre les notes à l'ouverture, avec 2 rangs de
  marge ; le reste du clavier se replie en deux bandes fines qu'un clic rouvre ;
- « Notes only » : seuls les rangs joués restent, chacun nommé.
- Couleur par corde, par hauteur ou par vélocité.
- La souris et la molette suivent la table REAPER de Cedric.
- Tout ce qui s'explique vient au survol.

**Le chemin.**
1. Porter le modèle `Roll` dans une crate Rust pure (`cp_roll`), sans unité,
   avec ses tests. Les pièges trouvés en 2026-07 deviennent des tests.
2. La vue piano roll sur `cp_ui`, puis le clavier et la souris pris de la
   configuration de Cedric.
3. Le brancher partout où un item MIDI s'ouvre :
   - le double-clic dans Guitar Tab (une note y garde sa corde : canal = corde) ;
   - les items de l'arrangement ;
   - plus tard, la Session.
4. La partie audio de l'éditeur vient avec A4 et A5 (vitesse, hauteur,
   étirement).

**Construit le 2026-11-22 (v1).** Cedric : « l'éditeur est vraiment un
éditeur à part de Guitar Tab ; ça fait du MIDI ou du clip ».
- `cp_roll` : le modèle, testé (le canal tenu).
- `cp_editview` : la vue de la maquette, piano roll et clip audio, testée en
  PNG et par les gestes.
- `cp_module/src/editor.rs` : l'app, avec son action, son onglet et son
  icône dans CP Transport, et trois places. Elle suit la sélection, écrit en
  un pas d'annulation, relit un item modifié ailleurs, fait entendre une note
  par sa piste. Le double-clic sur le MIDI de Guitar Tab l'ouvre.
- L'auto-test du vrai REAPER passe : transposition écrite, canaux gardés,
  annulation.
- Premier retour de Cedric (2026-11-22 après-midi), corrigé le même jour :
  - glisser une note l'envoyait en C9 : la vue suivait les notes pendant le
    geste. La vue verticale est désormais une caméra : tout le clavier défile,
    la hauteur des rangs ne change que sur demande, le cadrage se fait à
    l'ouverture et sur demande (son pavé « - »), jamais pendant un geste ;
  - **ses raccourcis de l'éditeur MIDI de REAPER, tels quels** : les défauts
    de REAPER (lus dans sa section 32060) puis son `reaper-kb.ini` ;
  - la table souris de REAPER complétée (clic droit = menu, Ctrl glisser sur
    la grille = copie, Maj+Win / Ctrl+Win, accords) ;
  - l'écoute des notes : un bouton.
- Deuxième retour (2026-11-22, 15 h), traité le même jour, pour l'éditeur et
  Guitar Tab ensemble :
  - **des saccades** : mesurées sur des bancs (tests `banc_*`). Le cadre
    pointillé d'une prise pas transcrite était tracé sur toute sa longueur
    (13 ms, 45 ms de près) ; les rangs de l'éditeur avaient des bords entre
    deux pixels (6 ms sur 7) ; chaque image de lecture redessinait toute la
    fenêtre. Désormais une image complète coûte 1,4 ms (éditeur) et 4 à 8 ms
    (Guitar Tab), une image de lecture 0,006 ms : la tête de lecture, le
    curseur et la ligne de la souris sont des traits posés sur la scène
    gardée (`cp_gfx::layers`, `cp_module/src/frame.rs`) ;
  - la relecture de REAPER : la sélection à chaque battement (un tiers de
    seconde avant), le reste quand le compteur du projet bouge, rien
    pendant qu'un bouton de souris est tenu ;
  - **l'audio et son MIDI forment un groupe** (celui qu'on bouge ou rogne
    entraîne l'autre) ; les notes appartiennent à leur item ;
  - comme REAPER : bords qui se rognent, poignées de fondu, ses touches au
    survol de la fenêtre principale (Q, D, S, Z, A, E), ses curseurs de
    souris (avec ses fichiers `Cursors`), la ligne à la souris et ses
    réglages, les couleurs de son thème, des traits d'un pixel ; le clic
    sur une note pose le curseur au clic.
- Nuit du 22 au 23 novembre :
  - les bends (et la pédale legato) suivent leur note quand elle bouge, se
    copie, se coupe ou s'efface, comme « Move CC events with notes », et se
    dessinent en courbe sur la note ;
  - le menu des notes porte les générateurs de l'ancien `Roll` : accords
    (dont la triade diatonique), arpèges (montant, descendant, aller-retour,
    hasard, deux octaves), rythmes euclidiens ; le canal (la corde) gardé.
- Le 23 novembre (maquette validée par Cedric) :
  - **la palette du clic droit** remplace le menu d'une soixantaine de
    lignes : neuf groupes d'icônes, le survol d'un groupe ouvre ses icônes,
    le survol d'une icône montre son résultat en pointillés, une icône qui ne
    ferait rien est éteinte, Maj+clic garde la palette, la molette règle un
    rythme euclidien, le dernier groupe revient ;
  - **les bends se modifient** : posés par la palette (½, 1, 1½ ton,
    aller-retour, vibrato, glisses d'approche), puis deux poignées sur le
    trait (le sommet : de combien et quand ; le pied : quand il part),
    Alt+clic les enlève ; relu dans le vrai REAPER.
- **la timeline de l'item seul** (2026-11-23, demande de Cedric) : un bouton
  de la barre ; les mesures comptées depuis le début de l'item, la grille,
  le magnétisme et la quantification aussi ; la vue bornée à l'item.
- Reste :
  - les couches CC et pitchbend (l'ancien éditeur n'en avait pas, décidé) ;
  - la probabilité (cases de la Session, à venir) ;
  - les enveloppes ADSR/ENV et l'établi d'un son ;
  - les transients du clip ;
  - ~~le pas à pas~~ : fait le 2026-11-23 (menu des notes, « Step input from
    MIDI keyboard » : chaque note jouée se pose au curseur, longue d'un pas de
    grille, le curseur avance ; des notes ensemble font un accord).

**Fini quand :** Cedric ouvre ses items MIDI dans notre éditeur plutôt que dans
celui de REAPER.

| application | ce qu'elle fait | où est sa spécification |
|---|---|---|
| Navigateur de fichiers (Media Explorer) | parcourir, écouter, glisser dans REAPER, à la FL Studio | mémoire `project-media-explorer`, `reference-reaper-browser-scripts` |
| Éditeur de clips | audio et MIDI, piano roll, enveloppes, établi d'un son | **v1 construite le 2026-11-22** (section ci-dessus) ; ancien `ROADMAP_Editeur.md`, mémoires `project-midi-command-layer`, `project-keymap`, `project-editor-sound-workbench` |
| Session | lanceur de clips, prises, vers l'arrangeur, sortie MIDI par colonne | mémoires `project-session-*`, `ROADMAP_Chantiers.md` (ancien dépôt) |
| Sampler | kits de pads, un effet par kit | mémoires `project-kit-sampler-jsfx`, `project-sampler` |
| Transport | le transport à la place de celui de REAPER, clock externe | **construit le 2026-11-22** (`cp_transport`) ; reste la clock externe : mémoire `project-ext-clock` |
| FX Constellation | moteur de modulation, pad, graphe, liens | mémoires `project-fx-constellation-refonte`, `project-moteur-modulation-plan`, `ROADMAP_FXConstellation.md` |
| Séquenceur / Sound Gen | séquenceur à la Pigments, synthé JSFX | mémoires `project-sequenceur`, `project-soundgen-modulation`, `ROADMAP_Sequenceur.md` |
| Vidéo | caméra, stop motion, calage par le son | mémoire `project-video`, `ROADMAP_Video.md` |
| Guitar Tab | guitare enregistrée → MIDI, partition et tab éditables dans REAPER | **V1 construite le 2026-11-22** (`cp_tab`, `cp_tabview`) ; V2 en cours (section ci-dessus) ; bends, triolets, deux voix, armure, techniques, chiffrages et tempos qui changent, reprises : faits (2026-11-22/23) |
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
| le chemin de présentation | GDI (`SetDIBitsToDevice` des seules rangées changées), calé sur `D3DKMTWaitForVerticalBlankEvent` ; `DwmFlush` écarté (il bloque sur un écran immobile) ; DXGI si la mesure le demande |
| le rendu du texte | GDI pour l'instant (`GGO_GRAY8_BITMAP`), net dans les captures ; DirectWrite si les retours le demandent (idéogrammes, emoji) |
| ~~la première application~~ | **tranché** : le navigateur de fichiers (Cedric, 2026-11-21) |
| l'application suivante | Guitar Tab v2 est en cours ; l'éditeur de clips est demandé (2026-11-22), son ordre reste à fixer avec Cedric |

## Ce que CP Studio ne fera pas

- **Pas de Lua.** Les petits scripts indépendants existants restent tels quels.
- **Les effets restent en JSFX.** Une extension ne peut pas se placer dans une
  chaîne d'effets. Un plugin CLAP serait la voie si un jour il le faut.
- **Pas de macOS ni de Linux pour l'instant.** `cp_engine` ne dépend de rien de
  Windows, donc la porte reste ouverte.
