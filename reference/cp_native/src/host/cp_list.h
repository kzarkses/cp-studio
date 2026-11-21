#ifndef CP_LIST_H
#define CP_LIST_H

// ============================================================================
// LA LISTE D'AFFICHAGE — POURQUOI ELLE EXISTE MAINTENANT
// ============================================================================
// `cp_win.cpp` portait le commentaire INVERSE : « AUCUNE LISTE D'AFFICHAGE.
// Chaque ordre compose IMMEDIATEMENT [...] pour un gain nul : la traversee
// Lua->C est deja payee au moment ou l'ordre arrive, et c'est ELLE qu'on
// cherche a economiser, pas le dessin. »
//
// Ce raisonnement etait juste, et sa PREMISSE a change. Il supposait qu'une
// frame ne serait jamais dessinee qu'UNE fois, juste apres que Lua l'a decrite.
// Deux faits l'ont demolie :
//
//  1. UN SCRIPT DIFFERE TOURNE A 30 Hz. C'est la cadence du minuteur de REAPER
//     et aucune API ne la change. Un ecran a 120 Hz montre donc chaque image
//     quatre fois — et une extension ne peut PAS rappeler du Lua pour en
//     demander une cinquieme. Ce qui doit tourner plus vite que Lua doit donc
//     etre calculable SANS Lua.
//
//  2. PENDANT UN REDIMENSIONNEMENT, LUA NE TOURNE PAS DU TOUT. Windows entre
//     dans sa propre boucle modale ; le fil principal de REAPER y est bloque,
//     donc `defer` ne passe plus. La fenetre affichait une bitmap qu'on venait
//     de redimensionner et que personne n'avait remplie : le clignotement noir.
//
// Une frame ENREGISTREE se REJOUE. C'est la seule chose qui reponde aux deux.
//
// ⚠ ZERO ALLOCATION. Les arenes sont des tableaux fixes, poses une fois pour la
// vie du processus. Un ordre de trop est JETE, pas alloue — et `lost` le dit,
// parce qu'une frame tronquee en silence est un defaut introuvable.
//
// ⚠ ET CETTE LISTE NE SAIT PAS DESSINER DU TEXTE. Le texte passe par LICE, donc
// par le SDK de REAPER, que ce fichier ne connait pas. `replay` recoit une
// fonction de trace : c'est ce qui garde la liste verifiable hors REAPER.

#include "../gfx/cp_raster.h"
#include "../gfx/cp_blit.h"

namespace cp {
namespace list {

enum Op {
  kOpNone = 0,
  kOpRect,        // d0..3 = x,y,w,h              c0 = couleur   i0 = alpha
  kOpRoundRect,   // d0..3 + i1 = rayon           c0/i0 = fond   c1/i2 = contour
  kOpText,        // d0,d1 = x,y   i0 = taille    c0 = couleur   i1 = offset arene
  // ⚠⚠ UNE DECOUPE EST UNE PILE, PAS UN ETAT COURANT. Le rejeu
  // repartait de la RACINE a chaque `kOpClip` et Lua devait donc lui reposer le
  // rectangle du parent a chaque `Pop` : la decoupe cessait d'etre une
  // propriete de la PILE pour devenir une propriete de l'HISTORIQUE de la
  // frame. Ca marchait tant que rien ne bougeait -- et ca cassait des qu'une
  // portee translatait le contenu, parce que le parent repose etait alors
  // translate lui aussi, ou ne l'etait pas, sans que rien ne puisse trancher.
  //   kOpClip      empile x,y,w,h INTERSECTE avec le sommet courant
  //   kOpClipPop   depile et rend au sommet precedent
  //   kOpClipReset vide la pile : retour a la fenetre entiere
  kOpClip,        // d0..3 = x,y,w,h
  kOpClipPop,
  kOpClipReset,
  kOpCircle,      // d0,d1 = cx,cy   i1 = rayon   c0/i0
  kOpRing,        // d0..5 = cx,cy,ri,ro,a0,a1    c0/i0
  kOpLine,        // d0..4 = x0,y0,x1,y1,largeur  c0/i0
  kOpPoly,        // d0 = largeur  i1 = offset  i2 = nombre de points  c0/i0

  // ============================================================================
  // CE QUE LE PORTAGE DU TOOLKIT A REVELE
  // ============================================================================
  // Comptes sur le code REEL, pas imagines : triangle (20 appels), degrade
  // lineaire (2), et surtout le DESSIN HORS ECRAN - `gfx.dest` (22),
  // `gfx.blit` (35), `gfx.setimgdim` (29), `gfx.muladdrect` (10).
  //
  // ⚠ LE DESSIN HORS ECRAN N'EST PAS UN DETAIL : C'EST LA CUISSON DES
  // ICONES. Sans lui, aucune des cinquante icones du toolkit n'apparait.
  //
  // ⚠ ET IL PASSE PAR LA LISTE, PAS A COTE. Dessiner tout de suite dans une
  // image marcherait a la premiere frame et disparaitrait au premier rejeu :
  // le compositeur ne saurait pas le refaire. Une image est donc peinte par le
  // REJEU, comme le reste.
  kOpDest,        // i0 = numero d'image, -1 = la fenetre
  kOpImgDim,      // i0 = numero, d0/d1 = taille voulue
  kOpBlit,        // i0 = source, i1 = offset (8 doubles), i2 = mode, d0 = alpha
  kOpMulAdd,      // d0..3 = rectangle, i1 = offset (8 doubles)
  kOpTriangle,    // i1 = offset (6 doubles)  c0/i0
  kOpGradRect,    // d0..3 = rectangle, i1 = offset (12 doubles)
  kOpRadial,      // d0..3 = cx,cy,rayon,attenuation  c0/i0
  kOpGlow,        // d0..5 = x,y,w,h,rayon,etalement  c0/i0

  // ============================================================================
  // LE TEXTE QUI COMPTE — LA DERNIERE EXCEPTION, ET ELLE ETAIT SUPPRIMABLE
  // ============================================================================
  // ⚠ « UN TEXTE QUI FORMATE UNE VALEUR NE PEUT PAS SUIVRE » ETAIT FAUX, et ca
  // a coute un aller-retour. Un cadran montrait son anneau a 120 Hz et son
  // « -6.0 dB » a 30 : deux cadences dans le meme objet, ce qui se voit PLUS
  // que 30 partout. On en avait conclu une frontiere du procede.
  //
  // La DLL sait formater un `double`. Le gabarit (« %.1f dB ») est pose UNE
  // fois par Lua ; le nombre vit dans `d[2]`, ou n'importe quelle liaison
  // d'argument peut l'ecrire — souris, temps, case de valeur. Aucune nouvelle
  // espece de liaison n'a ete necessaire : c'est le meme mecanisme, applique a
  // un ordre qui savait enfin quoi en faire.
  //
  // ⚠ LA BOITE EST DONNEE, PAS MESUREE. `d[3]`/`d[4]` portent la largeur et la
  // hauteur RESERVEES par la mise en page. Mesurer le texte formate
  // demanderait de le formater d'abord — donc a chaque composition, 120 fois
  // par seconde — et surtout la boite servirait au calcul d'endommagement, qui
  // doit connaitre la zone AVANT de savoir ce qu'on va y ecrire. Lua connait
  // cette boite : c'est lui qui fait la mise en page.
  //
  // d0,d1 = x,y   d2 = la valeur   d3,d4 = largeur,hauteur reservees
  // i0 = taille (indice d'atlas)   i1 = offset du gabarit   i2 = 1 si entier
  // c0 = couleur
  kOpTextVal,

  // d0..3 = x,y,w,h   i1 = offset (2n doubles : min,max dans [-1,1])  i2 = n
  // c0/i0 = couleur/alpha. Voir `gfx::draw_wave` pour le pourquoi.
  kOpWave,
  kOpCount
};

struct Cmd {
  int          op;
  double       d[6];
  unsigned int c0, c1;
  int          i0, i1, i2;
};

// ============================================================================
// LES LIAISONS — CE QUI PERMET DE REDESSINER SANS LUA
// ============================================================================
// Une frame rejouee a l'identique ne sert a rien : c'est la meme image. Le gain
// vient d'une poignee d'ordres qui DECIDENT eux-memes, a partir de ce que la
// DLL connait deja et que Lua ne voit qu'un tour sur quatre : la souris.
//
// ⚠ CE N'EST PAS UN SYSTEME REACTIF GENERAL, ET C'EST VOULU. Un langage
// d'expressions serait a inventer, a deboguer et a documenter. Deux formes
// suffisent a couvrir ce qui se voit vraiment :
//
//   kBindHover  — la couleur change quand la souris entre dans un rectangle.
//                 Tout le survol d'une interface, gratuitement.
//   kBindMouseX / kBindMouseY — un argument suit la souris par une affine
//                 bornee : `arg = borne(k * souris + c, lo, hi)`. Un cadran
//                 qu'on tourne, une liste qu'on fait defiler, un curseur.
//
// ⚠ ET UNE LIAISON N'EST PAS DANS `Cmd`. Elle vit a cote, avec l'index de
// l'ordre qu'elle modifie : un ordre non lie — l'immense majorite — ne paie
// donc rien, ni en memoire ni en temps.
// ============================================================================
// LA SOURCE ET L'ACTION SONT DEUX QUESTIONS DIFFERENTES
// ============================================================================
// ⚠ IL Y AVAIT CINQ GENRES POUR UNE SEULE OPERATION. `kBindMouseX`,
// `kBindMouseY`, `kBindTime`, `kBindOsc` et `kBindValue` faisaient tous
// EXACTEMENT la meme chose — `d[arg] = borne(k * source + c)` — et ne
// differaient que par l'endroit ou ils lisaient. Consequence : chaque nouvelle
// ACTION devait etre ecrite cinq fois pour couvrir les cinq sources, et
// personne n'allait le faire. C'est comme ca qu'on se retrouve avec une
// translation qui ne suit que la souris et pas le temps.
//
// On separe donc « OU JE LIS » de « CE QUE JE FAIS ». Trois actions nouvelles
// heritent d'un coup des cinq sources, et la sixieme source qu'on ajoutera un
// jour vaudra pour toutes les actions.
//
// ⚠ L'ABI NE CHANGE PAS. `CP_UIBindMouse`, `CP_UIBindTime`, `CP_UIBindOsc` et
// `CP_UIBindValue` existent toujours et s'ecrivent maintenant en `kBindArg` +
// source au moment ou elles sont posees. Rien du cote Lua n'a a le savoir.
enum Src {
  kSrcNone = 0,
  kSrcMouseX,
  kSrcMouseY,
  kSrcTime,      // secondes depuis l'ouverture
  kSrcValue,     // `slot` : une case, DEJA amortie par le compositeur
  kSrcOsc,       // sin(2*pi*freq*t + phase) — extrapole, pas interpole
  // ============================================================================
  // LA FORME D'UN LFO, PARCE QU'UN SINUS NE SUFFISAIT PAS
  // ============================================================================
  // ⚠ `kSrcOsc` NE SAIT QUE LE SINUS, ET UN LFO A CINQ AUTRES FORMES. Le point
  // qui parcourt la courbe d'un LFO a une abscisse affine du temps et une
  // ordonnee qui ne l'est PAS : lier la moitie donnerait un point qui glisse
  // horizontalement et saute verticalement, ce qui est PIRE que deux sauts
  // (regle de la cadence unique). Il fallait donc que la DLL sache evaluer la
  // FORME, ou ne rien lier du tout.
  //
  // Les cinq formes sont des expressions closes de la phase : la DLL les calcule
  // exactement, pas approximativement.
  //
  // ⚠⚠ ET CETTE TABLE EXISTE EN TROIS EXEMPLAIRES : ici, dans
  // `FX Constellation/Modules/LFOPanel.lua` (`shapeValue`) et dans le JSFX de la
  // banque. C'est le genre de duplication qui se paie : le jour ou l'une des
  // trois change, le point se separe de sa courbe SANS qu'une erreur soit levee.
  // `gfx_test.cpp` compare donc la table C++ a des valeurs relevees sur la
  // version Lua, forme par forme.
  //
  // ⚠ LES FORMES 5, 6 ET 7 NE SONT PAS ICI, ET C'EST DELIBERE. 6 et 7 tiennent
  // une valeur que seule la banque connait — constante, donc deja juste a
  // 30 Hz, rien a lier. 5 est un HACHAGE (`sin(c*78.233+12.9898)*43758.5453`,
  // partie fractionnaire) : un ecart d'un ULP entre le `sin` de Lua et celui de
  // la CRT donne une valeur COMPLETEMENT differente, donc un point qui saute
  // entre deux verites a chaque passe. Un pas de hasard tient de toute facon sa
  // valeur pendant un cycle entier : il n'a rien a gagner ici.
  kSrcShape,     // shapeValue(shape, freq*t + phase) — voir shape_value()
  // ==========================================================================
  // LA SOURIS D'UN GESTE, QUI SE FIGE AU RELACHEMENT (ABI 4.3)
  // ==========================================================================
  // ⚠⚠ `kSrcMouseX` SUIT LE POINTEUR, BOUTON TENU OU NON. Juste pour un
  // survol ; faux pour un geste : apres le relachement, l'objet tire continuait
  // de suivre la souris jusqu'a la passe suivante de Lua, puis revenait la ou
  // Lua l'avait pose. Celles-ci valent la souris tant qu'un bouton est tenu,
  // et la position du RELACHEMENT ensuite (`Live::ux/uy`) -- celle que Lua
  // recoit pour sa passe de relachement (`mouse_at`). Ajoutees EN FIN de liste :
  // les numeros des autres sont l'ABI.
  kSrcHeldX,
  kSrcHeldY,
  // ==========================================================================
  // LA POSITION DU TRANSPORT, TENUE PAR LA DLL (le socle, lot 8)
  // ==========================================================================
  // ⚠⚠ UNE LOI ABSOLUE, PAS UN ECART. `kSrcTime` + une ancre posee par Lua
  // (`c = x - k * maintenant`) faisait de chaque passe une re-ancre : la tete
  // sautait de l'erreur de mesure de Lua, trente fois par seconde. Celle-ci est
  // la position qu'on ENTEND, en secondes de projet, estimee par la DLL seule
  // (cp_playclock.h) et evaluee a chaque image : une tete liee par
  // `x = k * position + c`, ou `c` ne depend que de la VUE, ne peut plus sauter
  // a une passe -- Lua et la DLL lisent la meme droite.
  kSrcPlay
};

enum BindKind {
  kBindNone = 0,
  kBindHover,    // rect + couleur de remplacement
  // d[arg] = borne(k * source + c, lo, hi)
  //
  // ⚠ `arg` VA JUSQU'A 7, ET LES DEUX DERNIERS NE SONT PAS DES `d[]`. 6 designe
  // `i0` et 7 designe `i2` — les ALPHAS. Presque tout ce qui « apparait » ou
  // « clignote » dans le depot module l'alpha et non une coordonnee : une case
  // qui pulse, le halo d'un pad, l'etoile d'un favori. Sans ces deux numeros, la
  // liaison ne savait faire bouger que de la geometrie, et la moitie de ce qui
  // bouge a l'ecran lui echappait.
  kBindArg,
  // ⚠ IL N Y A PLUS DE `kBindMouseX`, `kBindMouseY`, `kBindTime`, `kBindOsc`
  // NI `kBindValue`. Ils faisaient tous EXACTEMENT la meme chose et ne
  // differaient que par la SOURCE : `kBindArg` + un `Src` les remplace tous les
  // cinq. L ABI ne change pas - `CP_UIBindMouse` et ses voisines existent
  // toujours et se reecrivent a la pose.

  // ============================================================================
  // LES PORTEES — CE QUI FAIT PASSER LE PROCEDE DE « PONCTUEL » A « D'ENSEMBLE »
  // ============================================================================
  // ⚠ TOUT CE QUI PRECEDE PATCHE UN ARGUMENT D'UN ORDRE. C'est correct et c'est
  // structurellement du petit a petit : il faut cabler chaque widget a la main,
  // chaque cablage peut etre a moitie fait, et rien ne suit ni le texte ni la
  // mise en page. Le symptome se voit — un potard qui tourne a 120 Hz avec son
  // etiquette figee a 30 — et il est PIRE que 30 partout, parce que l'oeil
  // compare deux choses cote a cote au lieu d'en juger une seule.
  //
  // Une portee lie une PLAGE d'ordres par ce qu'ils ont en commun. Un widget
  // entier s'allume par UNE declaration : fond, cadre, icone, etiquette. Une
  // liste entiere defile par UNE declaration, textes compris. Et la declaration
  // se pose au niveau du CONTENEUR — dans `Layout`, a l'ouverture d'un widget —
  // pas ordre par ordre. C'est ce qui rend le cablage impossible a faire a
  // moitie : on n'entoure pas la moitie d'un widget.
  kBindRotate,   // PLAGE : tourne les points autour de (rx, ry)

  // PLAGE : substitue des couleurs quand la souris est dans le rectangle.
  // ⚠ UNE SUBSTITUTION, PAS UN DELTA. La tentation est d'eclaircir d'un cran :
  // le theme dit « un etat est un DEPLACEMENT sur la rampe » et hover monte
  // d'une marche (0,275 -> 0,320). Mais le theme CLAIR descend au survol
  // (0,96 sur un fond a 1,0), et trois themes livres ont des accents dont le
  // survol change de teinte et pas de luminance. Un delta unique serait faux
  // pour la moitie du depot, et faux d'une facon qu'on ne verrait qu'en
  // changeant de theme. Les paires viennent donc du theme lui-meme, que Lua
  // connait deja.
  kBindTint,

  // PLAGE : translate toute la geometrie. `arg` 0 = dx, 1 = dy — deux liaisons
  // pour les deux axes plutot qu'une structure deux fois plus grosse pour un
  // cas (les deux a la fois) qui est le plus rare.
  //
  // ⚠ C'EST CE QUI DEPLAFONNE LE DEFILEMENT, ET DONC TOUTES LES LISTES DU DEPOT
  // D'UN SEUL COUP. Une liste qui defile ne se RECALCULE pas : elle se DEPLACE.
  // Le texte ne change pas, seule sa position change — et une position, ca se
  // translate sans savoir ce qu'on translate.
  kBindMove,

  // PLAGE : saute les ordres de DESSIN quand la condition ne tient pas.
  // Infobulles, fantomes de glisser, anneaux de selection, surbrillances qui
  // APPARAISSENT au lieu de changer de teinte.
  //
  // ⚠ ELLE NE SAUTE JAMAIS UN ORDRE D'ETAT. `kOpClip`, `kOpClipReset`,
  // `kOpDest` et `kOpImgDim` posent l'etat du rejeu : en sauter un laisserait
  // tout ce qui suit dessiner dans la mauvaise cible ou la mauvaise decoupe, et
  // le defaut apparaitrait LOIN de sa cause.
  kBindShow
};

// ⚠ UNE LIAISON PONCTUELLE ET UNE PORTEE NE VIVENT PAS DANS LE MEME TABLEAU, ET
// C'EST STRUCTUREL. Les ponctuelles sont posees JUSTE APRES leur ordre, donc
// deja triees par `cmd` croissant — c'est ce qui permet le curseur qui ne
// recule jamais, et donc le O(n+m). Une portee, elle, est posee a sa FERMETURE :
// son `cmd` est la marque d'ouverture, plus petit que celui de la ponctuelle
// d'avant. Une seule portee melangee au tableau casserait le tri, le curseur
// sauterait des liaisons, et le symptome serait « certains survols marchent ».
struct Bind {
  int          kind;
  int          src;   // ou elle LIT (enum Src)
  int          cmd;   // l'ordre vise — le PREMIER pour une portee
  int          last;  // le DERNIER d'une portee (= cmd pour une ponctuelle)
  int          arg;   // kBindArg : quel d[] — kBindMove : 0 = dx, 1 = dy
                      // kBindHover : l'alpha de remplacement (0 = inchange)
  int          slot;  // kSrcValue : le numero de case
  double       k, c, lo, hi;
  // ⚠ UNE PHASE REBOUCLE, UNE AFFINE BORNEE NON. Un point de lecture revient a
  // zero a la fin de la passe ; `borne(k*t + c)` le collerait au bord droit et
  // l'y laisserait jusqu'a la passe Lua suivante. `mod > 0` replie la valeur
  // dans [lo, lo + mod[ APRES l'affine et AVANT le bornage — c'est ce qui fait
  // qu'une boucle de huit mesures defile a la cadence de l'ecran sans que Lua
  // ait a la reveiller.
  double       mod;
  // ==========================================================================
  // LE MAGNETISME EST UNE FORME CLOSE, ET ON AVAIT ECRIT LE CONTRAIRE
  // ==========================================================================
  // ⚠⚠ << UNE LIAISON N'EVALUE QU'UNE AFFINE, ET UN MAGNETISME N'EN EST PAS
  // UNE >> etait ecrit dans CP_Editor comme une limite du procede. C'est faux, et
  // ca a coute les deux surfaces qu'on regarde le plus : tout le piano roll et
  // toute la vue d'onde restaient a 30 Hz des que la grille etait active --
  // c'est-a-dire PAR DEFAUT.
  //
  // Un escalier UNIFORME est une expression close :
  //     q(v) = floor((v - qphase) / step) * step + qphase
  // La DLL la calcule exactement, et le bornage qui suit ne change pas.
  //
  // ⚠ << UNIFORME >> EST LA CONDITION, ET ELLE SE VERIFIE CHEZ L'APPELANT. Une
  // grille MUSICALE n'est uniforme en PIXELS que si le tempo ne change pas dans
  // la vue. Quand une carte de tempo la traverse, l'appelant doit renoncer :
  // poser un pas moyen ferait deriver le magnetisme d'une fraction de case a
  // l'autre bout de l'ecran, ce qui est pire qu'attendre.
  //
  // ⚠ ET L'ARRONDI N'EST PAS ICI. `floor` seul ; le << au plus proche >> -- qui
  // est ce que fait le magnetisme de REAPER -- se ramene a un decalage de phase
  // d'un demi-pas, que la facade Lua applique, avec la compensation
  // correspondante sur `c`. Deux conventions d'arrondi dans la DLL auraient donne
  // un champ de plus et une occasion de les confondre.
  //
  // ⚠ EFFET DE BORD HEUREUX : `bind_differs` compare deux evaluations, donc un
  // mouvement de souris qui reste dans la meme case NE SALIT RIEN. Un glisser
  // magnetise coute donc moins qu'un glisser libre, au lieu de couter plus.
  double       step, qphase;     // 0 = aucune quantification
  double       freq, phase;      // kSrcOsc et kSrcShape
  int          shape;            // kSrcShape : 0 sinus, 1 triangle, 2 rampe
                                 // montante, 3 rampe descendante, 4 carre
  int          rx, ry, rw, rh;   // le rectangle de condition — rotation : centre
  unsigned int col;              // kBindHover : la couleur de remplacement
  int          p_off, p_n;       // kBindTint : les paires (de, vers) dans `pts`
  int          inv;              // kBindShow : 1 = CACHE quand la condition tient
  // La boite de la plage, en coordonnees ecran. Calculee une fois par
  // `finalize`, pas a chaque composition : c'est ce qui rend le calcul
  // d'endommagement negligeable devant le dessin qu'il evite.
  double       bx, by, bw, bh;
};

// ⚠ CE QUE LA ROTATION SAIT TOURNER, ET CE QU'ELLE NE SAIT PAS. Un trait, un
// disque et un anneau portent des POINTS : ils tournent. Un rectangle, un
// rectangle arrondi et du texte sont axe-alignes par construction — les faire
// tourner demanderait un rasteriseur different, pas une transformation. Ils
// sont donc laisses tels quels, et le harnais l'ASSERTE plutot que de laisser
// croire qu'on a oublie.

// ⚠ SOIXANTE-QUATRE ETAIT UN PLAFOND DE PROTOTYPE. Une case par chose qui bouge
// sans la souris : un VU-metre par piste, un defilement lisse par liste, une
// animation d'ouverture par panneau. CP_Session seule en demande une par
// colonne. `Core.Slot` distribue par nom et rend nil quand il n'y en a plus —
// donc le symptome d'un plafond trop bas n'est pas une erreur, c'est un metre
// qui reste a 30 Hz sans rien dire. Le cout est de 8 octets par case et par
// fenetre : 2 Ko pour 256, contre les 156 Ko qu'une fenetre coute deja.
enum { kSlots = 256 };

// Ce que la DLL sait et que Lua ne voit qu'a 30 Hz.
//
// ⚠ `val` PEUT ETRE NUL, et une liaison de valeur rend alors zero. C'est
// volontaire : le harnais hors REAPER n'a pas de cases, et une frame doit
// rester dessinable sans elles.
struct Live {
  int           mx, my;
  int           buttons;
  double        t;      // secondes depuis l'ouverture de la fenetre
  const double* val;    // kSlots valeurs, DEJA amorties par le compositeur
  // La fin du dernier geste (kSrcHeld*, ABI 4.3), voir `note_release`. EN
  // DERNIER : les initialisations par position ({ mx, my, buttons, t, val })
  // restent justes et mettent ceux-ci a zero.
  int           ux, uy;
  // La position du transport a cet instant (kSrcPlay, lot 8), en secondes.
  double        play;
};

// ============================================================================
// LES ARENES GRANDISSENT — ELLES NE PLAFONNENT PLUS
// ============================================================================
// ⚠ UN PLAFOND FIXE ETAIT UN CHOIX FAIT SANS COMPTER. 4 096 ordres, c'etait
// << une frame d'interface qui en demande plus se trompe >>. C'est faux : une
// seule courbe de 126 points, tracee segment par segment comme le toolkit le
// fait, coute a elle seule 504 ordres. FX Constellation en pose plusieurs par
// panneau. La frame etait donc TRONQUEE, en silence, sur la plus grosse fenetre
// du depot — et la troncature se lisait comme un defaut de dessin.
//
// ⚠ ET LE PLAFOND COUTAIT AUSSI QUAND IL NE SERVAIT PAS. Les quatre arenes
// faisaient 434 Ko, trois frames par fenetre, donc 1,3 Mo — que la fenetre
// dessine trois rectangles ou trois mille. Sur trente-deux fenetres, 42 Mo de
// tampons dont l'essentiel n'est jamais touche. La cible est un PC de 2005.
//
// On part donc PETIT et on grandit a la demande. La croissance :
//   - n'arrive QUE sur la frame que Lua remplit (`build`), jamais sur celle que
//     le fil de rejeu lit — c'est ce qui la rend sure sans verrou ;
//   - converge en deux ou trois frames et ne se reproduit plus ;
//   - ne viole PAS << zero allocation par frame >>, qui porte sur le chemin de
//     dessin en regime etabli, pas sur l'ouverture d'une fenetre.
// Rien ne retient de pointeur dans une arene : les ordres portent des DECALAGES
// (`push_text` et `push_pts` rendent un offset). Un `realloc` est donc sans
// danger, ce qui n'aurait pas ete vrai avec des pointeurs.
enum {
  kCmd0  = 1024,   kCmdMax  = 262144,
  kTxt0  = 4096,   kTxtMax  = 1048576,
  kPts0  = 2048,   kPtsMax  = 1048576,
  kBind0 = 512,    kBindMax = 65536,
  kRng0  = 128,    kRngMax  = 16384
};

// La profondeur d'imbrication des portees. Huit couvre tout ce que le toolkit
// empile — fenetre, panneau, liste, ligne, widget, sous-widget — et le
// depassement est SILENCIEUX PAR DEFAUT ET FAUX : une portee qu'on n'empile pas
// laisse ses ordres sans translation, donc un contenu qui ne defile pas au
// milieu d'une liste qui defile. `finalize` leve donc `lost`.
enum { kMaxScope = 16 };

// ⚠ LA PROFONDEUR DE DECOUPE RETENUE, ET ELLE EST LA MEME DES DEUX COTES
// (le socle, lot 3). L'enregistrement (cp_win.cpp `clip_push`) retenait
// seize parents, le rejeu quinze : a la seizieme decoupe imbriquee, le rejeu
// dessinait PLUS LARGE que ce que Lua avait decrit. Une constante, une regle
// (voir `replay`), et ce qui la depasse se compte (`kClipTooDeep`).
enum { kClipDepth = 16 };

// Une frame decrite mais pas encore dessinee. Trois exemplaires vivent dans
// `cp_win` : celui que Lua remplit, celui qui attend, celui que le rejeu lit.
struct Frame {
  Cmd*    cmd  = nullptr;   int cap_cmd  = 0;
  char*   txt  = nullptr;   int cap_txt  = 0;
  double* pts  = nullptr;   int cap_pts  = 0;
  Bind*   bind = nullptr;   int cap_bind = 0;   // ponctuelles, DEJA triees
  Bind*   rng  = nullptr;   int cap_rng  = 0;   // portees, triees par `finalize`
  int     n_cmd = 0, n_txt = 0, n_pts = 0, n_bind = 0, n_rng = 0;
  int     w = 0, h = 0;   // la taille pour laquelle elle a ete DECRITE
  bool    lost = false;   // un ordre a-t-il ete jete faute de place
  bool    sealed = false; // `finalize` est-elle passee (tri + boites)

  // ⚠ NI EXCEPTION NI COPIE. On alloue par `malloc` et on tolere l'echec : une
  // arene a zero laisse `push` rendre false et lever `lost`, ce qui est deja le
  // chemin d'erreur existant. Et une Frame ne se copie pas — deux exemplaires
  // qui partagent une arene la libereraient deux fois.
  Frame();
  ~Frame();
  Frame(const Frame&) = delete;
  Frame& operator=(const Frame&) = delete;
};

void reset(Frame& f, int w, int h);

// Rendent false / -1 quand l'arene est pleine. L'appelant n'a rien a faire de
// plus : `lost` porte le drapeau et la ligne d'etat le montre.
//
// ⚠⚠ UN ORDRE N'ENTRE QUE SAIN (le socle, lot 2). `push` range une COPIE
// assainie : une valeur non finie fait de l'ordre un `kOpNone` (il garde sa
// place -- une liaison vise un INDEX d'ordre -- et ne dessine rien), une
// coordonnee est bornee a +-2^24, un rayon a la meme borne, une taille d'image
// a `sane::kImgDimMax`. Rien en aval n'a plus a s'en mefier. Voir cp_sane.h.
bool push(Frame& f, const Cmd& c);
int  push_text(Frame& f, const char* s);
// Des POINTS : des coordonnees, bornees comme celles d'un ordre.
int  push_pts(Frame& f, const double* p, int n);
// ⚠ `push_pts` COMPTE DES POINTS, `push_raw` COMPTE DES NOMBRES. Le premier
// copie 2n doubles, le second n. Les confondre pour un blit - huit nombres, pas
// huit points - ecrirait seize doubles dont huit d'ordures, et le dessin serait
// juste assez plausible pour qu'on cherche ailleurs.
//
// ⚠ ET `lim` DIT CE QUE SONT CES NOMBRES : `sane::kCoordMax` pour des
// coordonnees (blit, triangle, degrade), `sane::kColorMax` pour des couleurs
// (les paires de teinte, jusqu'a 2^32). Une borne commune aurait soit casse
// les couleurs, soit laisse deborder les coordonnees.
int  push_raw(Frame& f, const double* p, int n, double lim);
bool push_bind(Frame& f, const Bind& b);
bool push_range(Frame& f, const Bind& b);

// La forme d'un LFO (kSrcShape) : phase en cycles, resultat 0..1. Publique pour
// les harnais (le socle, lot 6) : `Tools/laws_test.py` la confronte a sa copie
// Lua (`Core.ShapeValue`), celle qui decrit la frame que la DLL prolonge.
double lfo_shape(int shape, double p);

// ============================================================================
// LE SCELLEMENT — CE QUI SEPARE « DECRITE » DE « REJOUABLE »
// ============================================================================
// Trie les portees par ordre d'ouverture et calcule la boite de chacune. Deux
// choses que le rejeu ne peut pas faire lui-meme :
//
//  - LE TRI. Une portee est posee a sa FERMETURE, donc l'imbriquee avant
//    l'englobante. Le balayage du rejeu empile les portees a mesure qu'il
//    avance dans les ordres : il lui faut l'ordre d'OUVERTURE. Trier a chaque
//    composition, ce serait le refaire 120 fois par seconde pour un tableau qui
//    ne bouge plus.
//  - LES BOITES. Le calcul d'endommagement en a besoin AVANT de decider s'il
//    compose. Les calculer pendant la composition serait les calculer trop
//    tard.
//
// ⚠ SUR LE FIL DE LUA, PAS SUR CELUI DU REJEU. Elle ecrit dans la frame ; le
// rejeu ne fait que la lire. C'est la meme regle que la croissance des arenes,
// et c'est ce qui rend les deux sans verrou.
void finalize(Frame& f);

// La fonction de trace du texte, fournie par l'hote parce qu'elle passe par
// LICE. Signature volontairement pauvre : la liste ne doit rien savoir de plus.
typedef void (*TextFn)(const gfx::Surface& s, int x, int y, const char* str,
                       unsigned int col, int size);

// ============================================================================
// LES IMAGES QUE LA LISTE NE POSSEDE PAS
// ============================================================================
// ⚠⚠ UN PNG N'EST PAS UNE IMAGE CUITE, ET LES CONFONDRE COUTE UN MEGA-OCTET
// PAR FRAME. Les icones du toolkit sont DESSINEES dans un tampon par les ordres
// de la frame : le rejeu les refabrique, elles n'ont donc rien a transporter.
// Un PNG, lui, vient du disque — et le seul fil qui puisse le decoder est celui
// de Lua, parce que c'est lui qui recoit le chemin.
//
// La tentation etait de coller ses pixels dans la frame. Le selecteur d'icones
// de la barre flottante en montre soixante a la fois, pris dans les 3 473 que
// REAPER livre : c'est plus d'un mega-octet, dans une arene qui en fait un.
// La troncature aurait ete SILENCIEUSE, et se serait lue comme un defaut de
// dessin.
//
// L'hote garde donc les pixels et le rejeu les LIT. Rien n'est copie, rien
// n'est transporte, et une image chargee une fois sert toutes les frames
// suivantes. ⚠ Le fournisseur est appele depuis le fil de rejeu : c'est a lui
// de se proteger de son propre chargeur (`cp_win` prend le verrou d'echange).
//
// ⚠⚠ `ctx` EST LA FENETRE, ET SANS LUI LES PNG ETAIENT GLOBAUX AU PROCESSUS.
// Les numeros d'image sont distribues PAR SCRIPT (Widgets.LoadImage part de
// 200 dans chaque etat Lua) ; un magasin unique faisait donc que trois barres
// flottantes chargeaient chacune leur premiere icone dans la meme case, et la
// derniere chargee gagnait pour les trois. Le fournisseur recoit la fenetre
// dont la frame se rejoue, et repond avec SES images.
typedef const unsigned int* (*ImgFn)(void* ctx, int id, int* w, int* h);

// ⚠ `live` PEUT ETRE NUL. Le rejeu rend alors la frame telle que Lua l'a
// decrite, liaisons ignorees — c'est ce que fait le harnais hors REAPER, et
// c'est ce qui rend la comparaison « enregistrer puis rejouer = dessiner »
// possible : sans souris, les deux chemins doivent donner le meme pixel.
// ⚠ LES IMAGES APPARTIENNENT A L'HOTE, ET LEUR TAILLE CHANGE PENDANT LE
// REJEU. `kOpImgDim` alloue : c'est la SEULE allocation du chemin de rejeu, et
// elle n'arrive qu'au changement de taille, pas par frame. Le harnais le
// verrouille - << 200 rejeux : zero allocation >> porte sur une frame sans
// image, ce qui est le cas de toutes celles qui se redessinent en continu.
// ⚠⚠ MILLE VINGT-QUATRE, PARCE QUE C'EST LE NOMBRE QUE `gfx` DOCUMENTE.
// « Resize image referenced by index 0..1024-1 » — et le toolkit s'en sert sur
// TOUTE la plage, par le haut : les icones cuisent en 926..989, leurs brouillons
// en 990..992, les potards en 910..925, leur brouillon en 993, les champs de
// saisie en 900..901, le selecteur de couleur en 902..903, les halos que
// `ImageIDs.Alloc` distribue en descendant depuis 1023.
//
// Avec 128, TOUT cela tombait hors plage — donc `set_img_dim` et `blit_img`
// rendaient sans rien pousser dans la liste. Aucune erreur, aucune trace : les
// icones n'existaient pas, les potards perdaient leur fond, les champs de
// saisie leur tampon. Le symptome (« pas d'icone », « les knobs n'ont pas de
// fond ») ne designait pas sa cause, un plafond choisi sans regarder ce que les
// appelants numerotaient.
//
// Le cout est de 24 Ko par fenetre — 16 octets d'`Img` plus deux entiers
// d'ombre — face aux 1,3 Mo qu'une fenetre coute deja. Rien n'est alloue tant
// qu'une image n'est pas dimensionnee.
enum { kMaxImg = 1024 };

// ⚠ `state_only` : n'execute que ce qui MODIFIE DES IMAGES -- les cibles, les
// dimensions, et le dessin qui part dans une image -- et ne peint rien a
// l'ecran. C'est ce qu'on rejoue d'une frame publiee puis ECRASEE avant
// d'avoir ete composee : ses pixels n'interessent plus personne, mais la
// cuisson d'icone qu'elle portait est un etat que Lua croit acquis. `dst` peut
// alors etre nulle.
void replay(const Frame& f, const gfx::Surface& dst, TextFn text_fn,
            const Live* live = nullptr,
            gfx::Img* imgs = nullptr, int n_imgs = 0,
            ImgFn img_fn = nullptr, void* img_ctx = nullptr,
            bool state_only = false);

// Rend la memoire de toutes les images. A appeler a la fermeture d'une fenetre.
void free_imgs(gfx::Img* imgs, int n_imgs);

// La frame changerait-elle d'un pixel entre `prev` et `now` ? C'est `damage`
// sans la boite : gardee pour les appelants qui ne veulent qu'une reponse.
bool hover_changed(const Frame& f, const Live& now, const Live& prev);

// ============================================================================
// L'ENDOMMAGEMENT — POURQUOI « TOUT EN 120 Hz » TIENT SUR UN PC DE 2005
// ============================================================================
// ⚠ SANS LUI, DEPLAFONNER VEUT DIRE RECOMPOSER LA FENETRE ENTIERE 120 FOIS PAR
// SECONDE. Sur FX Constellation — 1400 x 800, soit 1,12 million de pixels — ca
// fait 134 millions de pixels par seconde a effacer, redessiner et recopier,
// pour un survol qui change cent pixels. La cible est un PC de 2005 : c'est
// exactement le chiffre qui transforme un deplafonnement en ralentissement, et
// on aurait « gagne » des fps en perdant la machine.
//
// La question juste n'est pas « que faut-il redessiner » mais « QU'EST-CE QUI A
// CHANGE ». Et la reponse est exacte, pas heuristique : seule une LIAISON peut
// faire differer deux compositions de la meme frame. On evalue donc chaque
// liaison sous l'etat d'AVANT et sous celui de MAINTENANT ; celles qui rendent
// la meme chose ne salissent rien.
//
// Trois consequences, et la troisieme est la plus importante :
//   1. survoler un bouton coute la boite du bouton, pas la fenetre ;
//   2. un VU-metre dont le niveau a fini de descendre cesse de couter — le
//      lissage exponentiel converge, donc le repos revient TOUT SEUL ;
//   3. ⚠ QUAND RIEN NE CHANGE, ON NE COMPOSE PAS DU TOUT. Une fenetre ouverte
//      que personne ne touche coute zero, meme avec le fil de rejeu a 120 Hz.
//      C'est ce qui permet d'en ouvrir seize sans y penser.
//
// Rend false quand rien ne change — l'appelant ne doit alors RIEN faire. Rend
// true avec la boite en pixels sinon ; la boite peut valoir la fenetre entiere,
// et l'appelant decide seul a partir de quelle proportion il preferera une
// composition pleine.
//
// ⚠⚠ EXACT, ET CE N'ETAIT PAS LE CAS. La premiere version salissait TOUTE la
// boite d'une portee de deplacement des que la souris bougeait, parce qu'elle
// ne savait pas ou tester les conditions des teintes qu'elle contient. Mesure
// sur une liste de trente rangees teintees : un pixel de souris HORS de toute
// rangee salissait 68 % de la fenetre -- au-dessus du seuil des deux tiers,
// donc une composition PLEINE a chaque mouvement de souris, a la cadence de
// l'ecran, sur toute fenetre qui contient une liste qui deborde. C'est le
// scenario des « 134 millions de pixels par seconde » que ce calcul existe
// pour empecher, produit par le calcul lui-meme.
//
// Chaque condition est maintenant evaluee sous la translation des
// deplacements qui l'englobent, avant et maintenant -- la meme pile que tient
// le rejeu, en une passe lineaire sur les portees triees. Une souris qui bouge
// sans changer l'etat d'aucune condition ne salit RIEN.
bool damage(const Frame& f, const Live& now, const Live& prev,
            int* x0, int* y0, int* x1, int* y1);

// ============================================================================
// L'ENDOMMAGEMENT D'UNE CASE VIDEO (ABI 4.1)
// ============================================================================
// Une case video change sans qu'aucune liaison ne bouge : c'est un AUTRE FIL
// (la camera, le robinet de REAPER) qui en a remplace les pixels. `damage` ne
// peut pas le voir, et `diff` non plus -- l'ordre de blit est le meme d'une
// frame a l'autre. Le compositeur demande donc ici la boite, a l'ecran et sous
// les translations de maintenant, de tous les blits qui montrent l'une des
// images `ids`. Rend false si aucun ordre ne les montre.
bool img_damage(const Frame& f, const Live& now, const int* ids, int n_ids,
                int* x0, int* y0, int* x1, int* y1);

// ============================================================================
// LE DIFF ENTRE DEUX FRAMES LUA — CE QUI EVITE UNE COMPOSITION PLEINE A CHAQUE
// PASSE DE LUA
// ============================================================================
// ⚠ UNE FRAME NEUVE ETAIT TOUJOURS UNE COMPOSITION PLEINE, plus un blit de
// toute la fenetre : trente fois par seconde par fenetre active, et toutes les
// fenetres publient dans le MEME tick de REAPER puisque leurs scripts
// s'enchainent. La passe de rejeu qui suit recevait quatre fenetres pleines
// d'un coup et pouvait manquer son balayage -- un accroc a 30 Hz par-dessus
// un mouvement a 120, ce qui est la signature exacte de « la logique tourne a
// 30 Hz et le moteur est uncapped ».
//
// Or deux frames Lua consecutives se ressemblent presque toujours : une
// valeur, une couleur, une rangee. On compare donc ordre par ordre CE QUI EST
// MONTRE (l'ancienne frame resolue sous l'etat ou elle a ete composee) a CE
// QUI EST VRAI (la neuve, resolue sous l'etat de maintenant), par contenu et
// non par decalage d'arene, et on ne salit que ce qui differe.
//
// Rend 0 : identiques au pixel pres, rien a composer. 1 : la boite. 2 : tout
// -- des qu'un ordre d'ETAT differe (decoupe, cible, dimension), qu'une frame
// est tronquee, ou que les tailles different. Une insertion au milieu decale
// tout ce qui suit : la boite grandit, et l'appelant repasse en plein au-dela
// des deux tiers comme pour tout endommagement.
int diff(const Frame& shown, const Live& shown_at,
         const Frame& next,  const Live& next_at,
         int* x0, int* y0, int* x1, int* y1);

// ============================================================================
// LA COUTURE -- CE QUI SAUTE QUAND UNE FRAME LUA ARRIVE (le socle, lot 8)
// ============================================================================
// ⚠⚠ TOUT CE QUI BOUGE A L'ECRAN EST CALCULE DEUX FOIS. Lua decrit sa frame
// trente fois par seconde ; entre deux passes, la DLL la prolonge par ses
// liaisons. A l'arrivee d'une frame neuve, les deux doivent tomber d'accord au
// pixel pres, sinon l'objet SAUTE -- « la tete scintille », « la selection
// glitche une fois sur deux ». Ces sauts se voyaient et ne se comptaient pas :
// c'est Cedric qui les trouvait, un soir, un par un.
//
// `seam` les compte. L'ancienne frame et la neuve sont resolues sous le MEME
// etat -- l'instant ou la neuve arrive -- donc leur difference est exactement
// ce que l'oeil voit sauter. Seuls comptent les ordres qu'une liaison deplacait
// d'un cote ou de l'autre : un ordre que Lua decrit a la main et qui change,
// c'est Lua qui le change, pas la couture. Et une couleur qui change n'est pas
// un saut (un survol, une selection) : l'ordre n'est plus le meme objet.
//
// ⚠ ORDRE A ORDRE, DONC SEULEMENT ENTRE DEUX FRAMES DE MEME FORME. Une frame
// qui a gagne ou perdu des ordres decale tous les indices : on ne mesure rien
// (`compared` < 0) plutot que de compter des sauts imaginaires.
//
// ⚠ CE QU'ELLE NE VOIT PAS : une rotation (cadrans, les points ne sont pas
// dans la boite), un ordre qui APPARAIT a la passe (la frame change de forme).
struct Seam {
  int    compared;   // ordres deplaces compares ; -1 : frames non comparables
  int    jumps;      // ceux que la frame neuve pose ailleurs, a plus de `tol`
  double max_px;     // le plus grand saut, en pixels
  double x, y;       // ou il a eu lieu : le coin de la boite NEUVE, ecran
  int    op;         // sur quel genre d'ordre (enum Op)
};
void seam(const Frame& shown, const Frame& next, const Live& at, double tol,
          Seam* out);

// La boite d'un ordre, en coordonnees ecran, translations comprises. Exposee
// parce que le harnais en a besoin pour verifier `damage` sans rejouer.
// Rend false pour un ordre qui ne peint rien (decoupe, cible, dimension).
bool bbox_of(const Frame& f, const Cmd& c,
             double* x0, double* y0, double* x1, double* y1);

// La frame contient-elle quelque chose qui bouge tout seul ? C'est ce que le
// fil de rejeu interroge : une frame sans horloge n'a aucune raison d'etre
// recomposee soixante fois par seconde, et ne doit rien couter.
bool has_clock(const Frame& f);

} // namespace list
} // namespace cp

#endif // CP_LIST_H

