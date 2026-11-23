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

✅ fait, ▶ en cours, ☐ à faire.

**Le moteur**
- ✅ Les lanes MIDI, l'arrêt daté, l'annulation, la prise en WAV.

**Le modèle**
- ▶ Colonnes et cases :
  - une colonne = une piste montrée dans la console ;
  - une case = un clip MIDI ou audio ;
  - les scènes : 8, puis une rangée vide toujours en bas.
- ▶ Le lancement :
  - Q (Off / Beat / Bar / 2 / 4 mesures) ;
  - une tolérance de 1/8 de Q, au plus 0,25 noire ;
  - une case part de son début (l'accolade), ou en verrou de phase (option par case) ;
  - recliquer une case qui joue l'arrête à la frontière ;
  - recliquer une case qui attend annule son lancement.
- ▶ Arrêter une colonne (à la frontière), tout arrêter, la panique.
- ▶ Lancer une scène :
  - chaque colonne qui a un clip le lance, les autres s'arrêtent ;
  - avec Ctrl, les autres colonnes ne s'arrêtent pas ;
  - une case qui joue déjà n'est PAS arrêtée : l'ancien code le faisait, c'était un défaut.
- ☐ Capturer une scène : les clips qui jouent sont copiés dans la première rangée vide sous la sélection.
- ▶ La prise MIDI :
  - Rec : N mesures fixe à la fois l'attente et la durée (voulu) ;
  - la prise part à la frontière de N mesures, tout de suite si rien ne joue ;
  - elle s'arrête seule, puis tourne.
- ▶ La prise audio : pareil, l'entrée de la piste, un WAV dans le dossier d'enregistrement du projet.
- ☐ L'overdub : passe par passe, les notes jouées rejoignent la boucle au tour suivant.
- ▶ Les cases audio :
  - deux longueurs : la région (ce qu'on entend) et la passe (au bout de combien elle recommence) ;
  - Loop : la matière boucle sur la région ;
  - One-shot : la région rejoue à chaque passe ;
  - le tempo de la source (déclaré > lu dans le fichier > lu dans le nom > deviné) ;
  - le taux ajusté à 2 % près ;
  - l'étirement cuit.
- ☐ Motion, une par colonne : Stay, One shot, March & wrap, March & stay, March & stop, Random, Exclusive random. Datée à la fin de la passe.
- ▶ L'enregistrement dans le projet (ProjExtState `CP_Session2`) et la reprise de l'ancien format (`CP_Session/grid` + `CP_Loop/dest<n>`).

**La vue**
- ☐ La grille :
  - en-têtes (nom, couleur, armement) ;
  - cases (nom, couleur, état, barre de passe) ;
  - les triangles des scènes, la ligne d'arrêt, la colonne fantôme (une piste neuve) ;
  - le défilement par colonnes entières.
- ☐ La barre d'outils : Clock, Q, Rec, overdub, tout arrêter, panique, Arrange Rec, Back to Arrangement.
- ☐ Les gestes :
  - la bande gauche d'une case lance, arrête ou enregistre ;
  - un clic sur le contenu ouvre l'éditeur ;
  - Alt+clic efface ;
  - glisser déplace (une case pleine échange) ; Ctrl copie ;
  - le clic droit ouvre le menu de la case ;
  - le double-clic sur un en-tête renomme.
- ☐ Le clavier : flèches, PgUp/PgDn, Entrée, Suppr, Maj+Entrée, Ctrl+Entrée, Ctrl+Maj+I, Ctrl+Z (la dernière case effacée).
- ☐ La tranche de mixage : entrée, sortie MIDI, écoute, fader, pan, M, S, mode d'automation, vumètre ; puis effets et envois.
- ☐ Des fichiers déposés dans une case (navigateur, Explorateur).

**L'application**
- ☐ L'action, l'icône dans CP Transport, l'onglet du sélecteur, les trois places.
- ☐ Ouvrir une case MIDI dans l'éditeur, et écrire ce qu'il change.
- ☐ Vers l'arrangement :
  - glisser une case dans l'arrangement ;
  - Arrange Rec ;
  - taire l'arrangement (lanes fixes), Back to Arrangement.
- ☐ L'auto-test dans le vrai REAPER : une lane joue un instrument, une prise MIDI, une scène.

## Ce qu'on refuse (repris de l'ancienne Session)

- Le modèle de patterns de FL, un tempo ou une signature par scène, les modes de lancement Gate et Repeat, la saisie de notes au clavier de l'ordinateur.
- Un undo général de la grille. Ctrl+Z rend seulement la dernière case effacée.
- Écraser une rangée pleine en capturant une scène.
- Taire les items de l'arrangement un par un : les lanes fixes le font.
