# CP Studio — ce qu'un agent doit savoir avant de toucher a quoi que ce soit

CP Studio est la suite de Cedric pour REAPER, reecrite en Rust, entierement en
natif : une extension REAPER, sans Lua. Elle remplace CP_Scripts (Lua + C++),
dont le depot a ete efface le 2026-11-20.

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
| `cp_sys` | la seule porte vers REAPER et Windows : tout le code `unsafe` est ici |
| `cp_hot` | le rechargement a chaud : surveiller, copier, charger, passer l'etat |
| `cp_loader` | `reaper_cp_studio.dll`, la petite DLL FIXE que REAPER charge au demarrage |
| `cp_module` | `cp_module.dll`, tout le code de CP Studio, recharge a chaud |
| `cp_fixture` | un module minuscule pour tester le rechargement |
| `xtask` | `cargo xtask check / module / install` |

Le chargeur tient TOUT ce que REAPER et Windows connaissent (battement,
actions, classe de fenetre, fenetres) et le transmet au module courant. Les
fenetres survivent donc a un rechargement, et le nouveau code les repeint.

## 3. Les regles non negociables

1. **`unsafe` n'existe que dans `cp_sys`.** Les autres crates le refusent a la
   compilation. Seules exceptions : les deux points d'entree exportes, generes
   par `export_reaper_plugin!` et `export_module!`. Chaque bloc `unsafe` porte
   un commentaire `SAFETY:` (clippy le verifie).
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
- Ils ne prouvent ni l'ecran ni le son. C'est Cedric qui regarde et qui ecoute.

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
