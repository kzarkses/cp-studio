# CP Studio — ce qu'un agent doit savoir avant de toucher a quoi que ce soit

CP Studio est la suite de Cedric pour REAPER, reecrite en Rust, entierement en
natif : une extension REAPER, sans Lua. Elle remplace CP_Scripts (Lua + C++),
dont le depot a ete efface le 2026-11-20.

**Par ou commencer : `ROADMAP_CPStudio.md`** (les exigences de Cedric, ce qui
est fait, tout ce qui reste, dans l'ordre, avec un critere de fin par etape).

## 1. D'ou on vient

- Le materiel de l'ancien depot est sur `E:\CP_Scripts_recovery_20261120`
  (lire `LISEZMOI.md` la-bas). Trois copies : E:, `D:\CP_Scripts_sauvegarde`,
  Google Drive de Cedric (`Mon Drive\CP_Scripts_sauvegarde`).
- L'ancien moteur C++ (audio + dessin) sert de REFERENCE au portage : on le
  compile a cote du code Rust, on donne les memes entrees aux deux, et ils
  doivent rendre la meme sortie (son echantillon par echantillon, pixels).
- Ce qui a ete decide et pourquoi : Rust pour que le compilateur attrape les
  erreurs de memoire et de fils AVANT qu'elles fassent tomber REAPER chez
  Cedric ; tout natif parce que la seule raison de l'ancienne separation C++ /
  Lua (une extension ne se recharge pas) est levee par le rechargement a chaud.

## 2. Comment c'est range

| crate | role |
|---|---|
| `cp_abi` | le contrat binaire entre le chargeur et le module (la SEULE frontiere binaire) |
| `cp_sys` | la seule porte vers REAPER et Windows, et les primitives temps reel (`rt`) : tout le code `unsafe` est ici |
| `cp_engine` | le moteur audio (voix, ports, vivier, horloge), en Rust sur, sans REAPER |
| `cp_audio` | le moteur DANS REAPER : apercu permanent par port, crochet de bloc, decodage des fichiers par REAPER sur un fil de travail, pics de forme d'onde |
| `cp_oracle` | l'ancien moteur C++ compile en DLL, pour comparer `cp_engine` a lui, et le programme C++ qui verifie nos tables virtuelles contre le SDK (tests seulement) |
| `cp_gfx` | le moteur de dessin : rasteriseur a couverture exacte, chemins, texte (cache de glyphes), PNG, sans Windows |
| `cp_ui` | le toolkit : theme, evenements, icones Lucide (SVG), champ de saisie, defilement |
| `cp_browser` | le navigateur de fichiers (jalon 1), sans Windows : il se teste avec le vrai moteur et se dessine en PNG |
| `cp_hot` | le rechargement a chaud : surveiller, copier, charger, passer l'etat |
| `cp_loader` | `reaper_cp_studio.dll`, la petite DLL FIXE que REAPER charge au demarrage |
| `cp_module` | `cp_module.dll`, tout le code de CP Studio, recharge a chaud |
| `cp_fixture` | un module minuscule pour tester le rechargement |
| `xtask` | `cargo xtask check / module / install` |

Le chargeur tient TOUT ce que REAPER et Windows connaissent (battement,
actions, classe de fenetre, fenetres) et le transmet au module courant. Les
fenetres survivent donc a un rechargement, et le nouveau code les repeint.

Le moteur audio vit dans le chargeur (`cp_audio`) : une retouche du module ne
coupe jamais le son. Le module le pilote par `HostApi::audio` (ABI 2) avec des
entiers a generation (clips, voix) : 0 n'est jamais valide, un handle perime est
refuse. Changer le moteur = reinstaller le chargeur, REAPER ferme.

## 3. Les regles non negociables

1. **`unsafe` n'existe que dans `cp_sys`.** Les autres crates le refusent a la
   compilation. Seules exceptions : les deux points d'entree exportes, generes
   par `export_reaper_plugin!` et `export_module!`, et `cp_oracle`, qui pilote
   une DLL C++ et n'est jamais livre. Chaque bloc `unsafe` porte un commentaire
   `SAFETY:` (clippy le verifie).
2. **Aucune panique ne traverse une frontiere `extern "C"`.** Elle ferait
   avorter REAPER avec le projet ouvert. Tout point d'entree passe par
   `cp_sys::guard`. Un module qui panique est mis de cote jusqu'a la
   construction suivante ; REAPER continue.
3. **Le module ne confie jamais a REAPER ou a Windows un pointeur vers son
   propre code** (son code disparait au rechargement) et ne lance jamais un fil
   qui survive a `shutdown`.
4. **Jamais de verrou tenu pendant un appel vers le module** : il rappelle
   l'hote. Et jamais d'attente sur un chemin reentrant : `try_lock`, et `BUSY`
   si c'est occupe.
5. **L'ABI est un entier.** Le monter = recompiler les deux cotes et reinstaller
   le chargeur REAPER ferme. Un module d'un autre ABI est refuse et l'ancien
   reste en place.
6. **La cible est un PC de 2005** : pas de `target-cpu=native`, rien au-dela de
   SSE2. Le runtime C est lie dans la DLL (`crt-static`).
7. **Le module se recompile en moins de 20 s** (`cargo xtask module` le mesure ;
   0,6 s le 2026-11-21). Au-dela, on redecoupe.
8. **Aucune dependance externe pour l'instant.** En ajouter une est une
   decision (temps de compilation, chaine d'approvisionnement), pas un reflexe.

## 4. Travailler

- `cargo xtask check` avant chaque commit : formatage, clippy sans
  avertissement, tous les tests.
- `cargo xtask module` : reconstruit le module. REAPER, s'il tourne, le
  recharge seul en environ 1 s (sondage 250 ms + stabilite 300 ms + 30 ms
  d'echange). La construction cassee est refusee, l'ancien module reste.
- `cargo xtask install` : seulement quand le CHARGEUR change. REAPER doit etre
  ferme (la commande le dit s'il ne l'est pas). Ecrit aussi
  `UserPlugins\reaper_cp_studio.module`, qui pointe sur le module de
  developpement (`target\debug\cp_module.dll`).
- Dans REAPER : actions `CP Studio: Show status`, `CP Studio: Reload module
  now`. Journal : `%APPDATA%\REAPER\cp_studio.log` (la session precedente dans
  `cp_studio.prev.log`).
- Retirer CP Studio de REAPER : supprimer `UserPlugins\reaper_cp_studio.dll`
  (REAPER ferme).

## 5. Ce que les tests prouvent, et ce qu'ils ne prouvent pas

- `cp_hot/tests/reload.rs` : chargement, rechargement avec passage de l'etat,
  panique rattrapee, construction cassee refusee, dechargement, menage.
- `cp_sys/tests/fake_reaper.rs` : le VRAI `reaper_cp_studio.dll` charge par un
  faux REAPER avec les structures exactes du SDK : version refusee,
  enregistrements, actions, rechargement par le battement, fermeture.
- `cp_sys/tests/fake_reaper.rs` joue aussi le FIL AUDIO de REAPER : crochet
  « pre », chaque apercu tire sa source par la table virtuelle, crochet
  « post ». La boucle de test s'y repete exactement, aucun compteur de perte ne
  bouge.
- `cp_oracle/tests/pcm_abi.rs` : un programme C++ compile contre le SDK appelle
  notre `PCM_source` par ses methodes C++, nous tend une source C++ que nous
  appelons par notre table, et donne la disposition des structures
  (`PCM_source_transfer_t`, `preview_register_t`, `audio_hook_register_t`).
  Le SDK est cherche dans `$REAPER_SDK`, sinon `../reaper-sdk/sdk`.
- `cp_audio` : a vitesse 1 la sortie est la matiere au bit pres, une section
  joue exactement sa matiere, le decodage rend chaque echantillon a sa place,
  les pics encadrent le vrai min/max a tout zoom.
- `cp_oracle/tests/differential.rs` : `cp_engine` contre l'ancien moteur C++,
  sur les memes commandes. Sorties IDENTIQUES AU BIT, memes dates de bloc, memes
  etats publies. Des scenarios par comportement, plus 300 graines de fuzz. Il a
  ete verifie qu'il mord : un ordre de multiplication change (1 ULP) et un
  fondu de 4 ms au lieu de 5 sont tous deux attrapes.
- `cargo xtask reaper-test` : le VRAI REAPER, lance depuis un dossier vide
  avec `CP_STUDIO_SELFTEST=quit`, en silence (gain 0). Il relit deux WAV par
  REAPER au bit pres, fait tourner le moteur (compteurs de pertes), pose une
  section dans le projet vide, ouvre le navigateur, mesure la cadence de
  l'ecran et les images en retard, l'ancre puis le rend flottant, photographie
  ses fenetres dans `%TEMP%\cp_selftest`. REAPER doit etre ferme. A lancer
  apres chaque changement du chargeur. (REAPER ne se ferme pas seul a la fin :
  une autre extension avorte a la sortie, meme sans la notre ; xtask l'arrete.)
- `cp_browser` : ses tests ecrivent des PNG (`%TEMP%\cp_browser_*.png`) :
  les REGARDER apres un changement de dessin.
- Ils ne prouvent ni l'oreille ni le geste reel (glisser-deposer, clavier dans
  un docker). C'est Cedric qui ecoute et qui manipule.

## 5 bis. Le portage du moteur audio

- La reference est `reference/cp_native` (voir `reference/PROVENANCE.md` : un
  instantane du 2026-10-25 au matin, ce qui est prouve et ce qui ne l'est pas).
- **Le Rust suit le C++ operation par operation**, memes types (f32 / f64),
  meme ordre. Ne jamais « simplifier » une expression du rendu sans relancer
  l'oracle : deux ordres d'operations ne donnent pas les memes bits.
- Deux ecarts VOULUS, sans effet sur les cas normaux : chaque port possede ses
  voix (deux ports peuvent etre rendus en meme temps sans rien partager), et un
  clip voyage dans la commande qui le lance (`Arc`) : une voix ne perd jamais sa
  matiere en plein son, et la memoire n'est jamais liberee dans le fil audio.
  Les lectures sont bornees au tampon (l'ancien C++ lisait hors du tampon sur
  une position negative).
- Pas encore porte : les lanes MIDI (`cp_lanes.cpp` est incomplet, l'oracle
  utilise un bouchon) et la clock externe.
- Le cote REAPER (`cp_audio`, A1) : un apercu MATERIEL (`PlayPreviewEx`, sortie
  1/2) par port, jamais arrete. Pas un apercu de piste : il passerait par le
  rendu anticipe de REAPER, en avance et par gros blocs, et l'horloge du
  crochet ne serait plus celle du bloc tire.

## 6. Travailler avec Cedric

- En francais, phrases courtes, questions en gras. « Discutons » = un echange
  ecrit avec UNE recommandation, jamais un questionnaire a choix multiples.
- Il teste tout lui-meme : ne jamais lui dire de tester, ne jamais classer
  « non teste » comme un risque.
- Autonomie complete donnee = aller au bout sans s'arreter pour valider.
- Textes de l'interface en anglais ; commentaires du code en francais sans
  accents.
- Commits atomiques, en francais sans accents, `git commit -F <fichier>`, avec
  la ligne `Co-Authored-By`. Jamais `git stash`.

## 7. Ce qui a coute un depot entier

- **Ne jamais lancer un outil tiers avec un dossier de valeur comme dossier
  courant** (MuseScore 4 `-o x.mscx` a efface le contenu de son dossier
  courant). Lancer depuis un dossier vide du scratchpad. Cargo et git, eux,
  tournent dans le depot.
- **Jamais d'effacement recursif.** Le code efface des fichiers NOMMES.
- **Pousser apres chaque commit** sur `origin`
  (https://github.com/kzarkses/cp-studio, PUBLIC, choix de Cedric) : le 20
  novembre, trois mois de travail n'existaient que dans le `.git` local. Rien
  de prive dans le depot : ni jeton, ni mot de passe, ni donnee personnelle.
