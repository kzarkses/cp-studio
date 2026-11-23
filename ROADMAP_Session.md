# CP Session — la feuille de route

Le lanceur de clips de CP Studio, refait en Rust. Commencé le 2026-11-23 au
soir, à la demande de Cedric : « Session, implémenté complètement ».

La spécification est l'ancienne Session (Lua, perdue le 2026-11-20). Il en
reste :
- `CP_Session.lua` du 25 octobre, dans l'historique de Claude ;
- `ROADMAP_Chantiers.md`, intact ;
- les analyses de `Recherche/` ;
- `Loop.lua` et `Cells.lua`, mais seulement dans leur version d'août.

Un inventaire complet en a été tiré le 2026-11-23. Ce qui suit en garde les
règles. Les numéros de ligne renvoient à cet ancien fichier.

## Ce que Cedric a tranché (2026-11-23)

- **Pas de plafond au nombre de colonnes.** Le moteur passe à 128 ports. Le
  port 0 reste à l'écoute du navigateur, et la colonne k joue sur le port
  k + 1.
- **L'overdub fait partie de la Session.**
- **Les colonnes-groupes restent de côté.**
- **Un clip MIDI est gardé dans le projet, sans item caché.** L'éditeur
  l'ouvre.

## L'architecture

| pièce | rôle |
|---|---|
| `cp_engine::lanes` | Le clip MIDI de chaque port, joué dans le fil audio, daté à l'échantillon. Il ne dépend d'aucun battement du fil principal. |
| l'extension du chargeur (`ExtApi`) | Les lanes, l'arrêt daté d'une voix, l'annulation d'un lancement et l'écriture d'une prise en WAV, **sans changer l'ABI**. Un chargeur plus ancien dit seulement que la fonction manque. |
| `cp_session` | Le modèle, pur et testé : colonnes, cases, scènes, lancement quantifié, prises, overdub, suites de lecture (Motion), enregistrement dans le projet. |
| `cp_sessionview` | La vue : grille, en-têtes, scènes, ligne d'arrêt, barre d'outils, tranche de mixage. Elle traduit chaque geste en une `Act`. |
| `cp_module/src/session.rs` | L'application dans REAPER : pistes, état du projet, moteur, éditeur, arrangement. |

**Le temps.** Le moteur ne connaît que l'échantillon. La Session tient une
ligne de temps qui lie les frames du moteur aux noires (`Timeline`). Elle a
deux modes :
- **Free** : la Session a sa propre horloge, qui part au premier lancement et
  joue même transport arrêté ;
- **Follow** : elle suit le transport de REAPER, et donc la clock externe.

Chaque lancement est daté à une frame du moteur, puis posté tout de suite. La
frontière tombe à l'échantillon, même si le fil principal s'arrête.

## Ce qu'elle fait — l'état

✅ fait (avec ses essais), ☐ à faire. Construite dans la nuit du 23 au
24 novembre ; l'auto-test dans le vrai REAPER passe (voir plus bas).

**Le moteur** (dans le chargeur, réinstallé cette nuit-là)
- ✅ Les lanes MIDI : un clip par port, daté à l'échantillon, dans le fil
  audio. Dix minutes sans une note décalée, des blocs de 97 frames.
- ✅ Un autre clip remplace le premier au frame exact. Un arrêt daté. Un
  tempo qui change sans saut. Une édition sans relance.
- ✅ L'annulation d'un lancement qui attend.
- ✅ Les notes probables, tirées à chaque passe.
- ✅ L'arrêt daté d'une voix. La prise écrite en WAV.
- ✅ 128 ports : pas de plafond au nombre de colonnes (127 colonnes).

**Le modèle** (`cp_session`, 28 essais)
- ✅ Les colonnes et les cases :
  - une colonne = une piste montrée dans la console ;
  - une piste disparue qui porte des clips reste, « tenue » (son menu leur
    donne une piste) ;
  - les scènes : au moins 8, et une rangée vide en bas.
- ✅ Le lancement :
  - Q, et sa tolérance ;
  - le début de la case ou le verrou de phase ;
  - recliquer annule un lancement qui attend, ou arrête à la frontière ;
  - un arrêt n'a pas de tolérance.
- ✅ Les scènes. Ctrl garde les autres colonnes. Une case qui joue déjà n'est
  plus arrêtée.
- ✅ La capture d'une scène : rien ne se relance, rien n'est écrasé.
- ✅ La prise MIDI et la prise audio :
  - Rec : N mesures ;
  - la colonne s'arrête au début de la prise, à la même frontière ;
  - le clip tourne en phase à la fin de la prise ;
  - recliquer annule la prise ou la finit (arrondie à la mesure).
- ✅ L'overdub : une prise par passe, fondue dans la boucle.
- ✅ Les cases audio :
  - la région et la passe ;
  - Loop, ou one-shot à chaque passe ;
  - le tempo de la source, avec sa raison ;
  - le taux ajusté à 2 % près ;
  - Stretch : la version cuite, et la hauteur qui change le temps d'une
    passe en attendant ;
  - sur une piste à instrument, une piste de son sœur.
- ✅ Motion : les 7 suites de lecture, datées à la fin de la passe.
- ✅ Free et Follow :
  - un tempo qui change re-ancre sans saut ;
  - un saut du transport recale ce qui joue et revise ce qui attend.
- ✅ Arrange Rec : un journal de ce qui joue, écrit à l'arrêt.
- ✅ L'enregistrement dans le projet (`CP_Session2`) et la reprise de
  l'ancienne Session.

**La vue** (`cp_sessionview`, planches PNG et gestes)
- ✅ La grille, la barre d'outils, les tranches de mixage, la colonne
  fantôme, le défilement.
- ✅ Les gestes et le clavier de l'ancienne Session ; Espace reste à REAPER.
- ✅ Tout se nomme au survol : aucune phrase à l'écran.

**L'application** (`cp_module/src/session.rs`)
- ✅ L'action, l'icône dans CP Transport, l'onglet du sélecteur, les trois
  places.
- ✅ Les menus :
  - de la case : couleur, longueur, signature, tempo, lecture, lancement ;
  - de la colonne : suites de lecture, cacher, effacer, piste neuve ;
  - de Q et de Rec ;
  - de la tranche : entrée, sortie MIDI, automation, effets.
- ✅ Une case MIDI s'ouvre dans l'éditeur : ses notes reviennent à la case
  et s'entendent tout de suite. La tête suit la lane.
- ✅ Des fichiers déposés :
  - de l'Explorateur, ou du navigateur avec son passage ;
  - sur une case, puis les cases vides en dessous ;
  - sur « + », une piste neuve.
- ✅ Une case glissée dans l'arrangement : un item sur la piste sous la
  souris, à l'instant sous la souris (calé).
- ✅ L'auto-test dans le vrai REAPER :
  - un clip MIDI joué par sa lane fait sonner ReaSynth dans sa piste ;
  - une case audio sonne dans la sienne ;
  - la panique arrête tout ;
  - Arrange Rec écrit un item de 12 notes, ce qui a joué.

**Reste**
- ☐ Taire l'arrangement sous une colonne qui joue (l'ancienne passait par
  les lanes fixes, jamais mesurées), et le vrai Back to Arrangement (pour
  l'instant : tout s'arrête).
- ☐ L'automation d'un clip (les courbes de l'ancien AUTO) ; ouvrir une case
  audio dans l'éditeur (sa région).
- ☐ Le clavier virtuel de REAPER n'entre pas dans une prise : elle lit les
  entrées matérielles.
- ☐ L'arrangement sous Arrange Rec : les items s'ajoutent par-dessus ceux
  qui y sont.
- ☐ L'oreille de Cedric : les prises, l'overdub, Follow avec une clock
  externe.

## Ce qu'on refuse (repris de l'ancienne Session)

- Le modèle de patterns de FL, un tempo ou une signature par scène, les modes de lancement Gate et Repeat, la saisie de notes au clavier de l'ordinateur.
- Un undo général de la grille. Ctrl+Z rend seulement la dernière case effacée.
- Écraser une rangée pleine en capturant une scène.
- Taire les items de l'arrangement un par un : les lanes fixes le font.
