// CP_Native — le moteur de lanes MIDI.
//
// C'est le portage de CP_MidiLooper.jsfx (phase 6 de la feuille de route), et
// avec lui disparaissent la piste routeur, ses envois filtres par canal, gmem
// comme protocole, et le plafond de quatre colonnes — qui ne venait pas d'un
// MAX_LANES mais du budget de 16 canaux MIDI d'une seule piste.
//
// REGLE DU DOSSIER src/core, tenue ici aussi : aucun appel a l'API REAPER,
// aucune allocation apres l'initialisation, rien qui puisse bloquer. Ce
// fichier se compile et se verifie sans REAPER.
//
// ---------------------------------------------------------------------------
// CE QUI EST DANS LE FIL AUDIO, ET CE QUI N'Y EST PAS
// ---------------------------------------------------------------------------
// Le JSFX faisait tout dans le fil audio parce qu'il n'avait pas le choix.
// Ici on choisit, et le partage suit une seule question : « est-ce que se
// tromper de 30 ms s'entend ? »
//
//   FIL AUDIO    la porte (quelles notes doivent sonner a cet instant), les
//                transitions d'etat en attente (un lancement quantifie tombe
//                sur SA frontiere), et l'horloge libre. Une double-croche a
//                140 BPM dure 107 ms : une frame de defer se serait entendue.
//   FIL PRINCIPAL les notes elles-memes, la capture live, la persistance, le
//                routage, l'interface. Rien de tout cela n'a d'echeance a la
//                milliseconde.
//
// Consequence directe et voulue : LE FIL AUDIO N'ECRIT JAMAIS DE NOTE. Il les
// lit. La capture entre par MIDI_GetRecentInputEvent, cote Lua, ou chaque
// evenement arrive DEJA horodate en echantillons — donc plus precisement que
// ce que le JSFX pouvait faire d'un midirecv par bloc.
//
// ---------------------------------------------------------------------------
// LA LISTE DE NOTES, ET POURQUOI ELLE EST DOUBLE
// ---------------------------------------------------------------------------
// Le JSFX partageait sa liste par gmem : le fil principal ecrivait pendant que
// le fil audio lisait. Benin en pratique, indefini en droit — et ce depot a
// deja paye une fois pour la difference. Chaque lane porte donc DEUX tampons :
// le principal ecrit dans celui qui n'est pas publie, puis publie l'indice
// d'un seul store atomique. Le fil audio prend l'indice une fois en debut de
// bloc et lit ce tampon-la jusqu'au bout. Aucune lecture dechiree, aucun
// verrou, et le prix est un megaoctet.
//
// ---------------------------------------------------------------------------
// OU SORT LE MIDI
// ---------------------------------------------------------------------------
// Une lane parle dans un PORT — le meme objet qu'une case audio utilise, un
// apercu permanent verse dans une piste, en pre-FX. Chaque lane a le sien,
// donc deux lanes ne se partagent plus un canal et le plafond de 16 disparait
// avec la piste qui l'imposait. Les evenements sont dates en echantillons
// ABSOLUS et deposes dans la file du port ; PortSource les rend au bloc dont
// ils relevent, a l'offset exact.
#pragma once

#include <atomic>
#include <cstdint>
#include <cstring>
#include "cp_types.h"
#include "cp_ring.h"

namespace cp {

enum : int {
  // 32 lanes = 16 colonnes et leur jumelle silencieuse. Le plafond n'est plus
  // musical, il est memoire : 32 x 2 x 1024 notes x 12 octets = 786 Ko.
  kMaxLanes     = 32,
  kMaxLaneNotes = 1024,
  // Evenements MIDI en attente par port et par bloc. Une porte qui bascule
  // integralement produit 128 coupures et 128 attaques ; 256 couvre le pire
  // cas sans jamais avoir a en perdre un.
  kPortMidiCap  = 256,
  // L'AUTOMATION D'UN CLIP (ABI 4.0) : au plus huit courbes par lane, chacune
  // parle sur un CC 14 bits -- poids fort sur `cc`, poids faible sur `cc + 32`
  // --, donc `cc` dans 0..31. 64 points par courbe, comme l'enveloppe d'un son.
  kMaxAutoCurves = 8,
  kMaxAutoPts    = 64,
};

// Modes de lane — memes valeurs que le JSFX et que Loop.lua, parce que trois
// couches qui numerotent les memes etats differemment est exactement le genre
// de traduction qu'on oublie une fois.
enum LaneMode : int {
  kLaneEmpty    = 0,
  kLaneRec      = 1,
  kLaneStopped  = 2,
  kLanePlaying  = 3,
  kLaneArmed    = 4,
  kLaneOverdub  = 5,
};

enum LanePending : int {
  kPendNone    = 0,
  kPendPlay    = 1,
  kPendStop    = 2,
  kPendRec     = 3,
  kPendStopRec = 4,
  kPendOverdub = 5,
};

enum LaneCmdType : int {
  kLcRec      = 1,
  kLcStopRec  = 2,
  kLcClear    = 3,
  kLcPanic    = 4,
  kLcPlay     = 5,
  kLcStop     = 6,
  kLcClearAll = 7,
  kLcOverdub  = 8,
  kLcSetMode  = 9,   // rappel de session : poser l'etat sans le jouer
  // « PARS / ARRETE EXACTEMENT LA », l'argument est un beat de l'horloge du
  // moteur. Ce n'est PAS un quantize : c'est un rendez-vous.
  //
  // Un enchainement automatique tombe a la FIN D'UNE PASSE, et une fin de passe
  // n'est pas une frontiere de quantize. Sur une case de deux mesures avec Q a
  // la mesure, la frontiere la plus proche tombe AU MILIEU du clip : la colonne
  // changeait de case a un endroit que personne n'avait choisi, et ca se voyait
  // sur les sept comportements a la fois puisque tous passent par la.
  //
  // La date vient de l'appelant parce qu'il est le seul a la connaitre : il lit
  // la phase, la longueur de zone et le beat du MEME bloc publie, donc « la fin
  // de cette passe » est un nombre exact et non une estimation.
  kLcPlayAt   = 10,
  kLcStopAt   = 11,
};

// Une cible en attente qui n'a PAS ENCORE DE DATE : elle partira au premier
// bloc ou une horloge existe. Les vraies cibles sont des positions en beats et
// ne sont jamais negatives, donc tout ce qui passe sous kWaitTest est ceci et
// rien d'autre. (Meme convention que le JSFX, aux memes valeurs, pour que
// l'interface qui l'affiche n'ait pas a apprendre un second vocabulaire.)
static const double kWaitClock = -1000000000.0;
static const double kWaitTest  =  -100000000.0;

struct LaneNote {
  float         start;   // en beats depuis le debut de la boucle
  float         len;     // en beats
  unsigned char pitch;
  unsigned char vel;
  // CHANCE DE JOUER, en pourcent, 0..100. Cent = toujours, et c'est le chemin
  // rapide : la porte ne hache rien pour une note qui n'a pas de probabilite,
  // c'est-a-dire pour l'immense majorite d'entre elles.
  //
  // Elle a pris UN DES DEUX OCTETS DE BOURRAGE, donc `sizeof(LaneNote)` ne
  // bouge pas et la liste tient toujours dans le meme megaoctet.
  unsigned char prob;
  unsigned char pad;
};

// ---------------------------------------------------------------------------
// L'AUTOMATION D'UN CLIP (ABI 4.0)
// ---------------------------------------------------------------------------
// « Dessiner des enveloppes de parametres par-dessus l'editeur, et qu'elles
// s'ecrivent quand on enregistre dans l'arrangeur » (Cedric, 2026-10-10). La
// courbe vit DANS le clip, en beats de clip, et la lane l'evalue a sa phase --
// donc elle boucle avec le clip, suit son accolade et son decalage, et part a
// la frontiere du lancement comme ses notes. Elle sort en CC 14 bits sur le
// port de la lane : un petit JSFX en tete de la piste les recoit et les tend a
// un lien de parametre (CP_Engine/ClipAuto.lua). Le meme CC voyage dans l'item
// MIDI qu'Arrange Rec ecrit : l'arrangeur rejoue ce que la Session jouait, par
// le meme chemin.
//
// Un CC ne part que quand la valeur 14 bits CHANGE, une fois par bloc au plus,
// date au debut du bloc. Au lancement (et apres chaque arret) la valeur part
// tout de suite, meme si elle n'a pas bouge depuis : le parametre a pu etre
// change entre-temps par un autre clip.
struct AutoPt {
  float t;   // beats depuis le debut du clip
  float v;   // 0..1
  float k;   // en attente : la courbe brute -1..1 ; publie : le facteur de shp
};

struct AutoCurve {
  int32_t cc;   // 0..31 (poids fort), le poids faible est cc + 32
  int32_t n;
  AutoPt  p[kMaxAutoPts];
};

struct AutoSet {
  int32_t   n;
  int32_t   pad_;
  AutoCurve c[kMaxAutoCurves];
};

struct LaneCmd {
  int32_t lane;
  int32_t cmd;
  double  arg;
};

// ---------------------------------------------------------------------------
// L'ancre de transport. Le coeur ne peut pas demander l'heure a REAPER : le
// fil principal la lui depose, et le fil audio la convertit en comptant ses
// echantillons — exactement comme l'ancre d'horloge du moteur de clips.
//
// Ecrite par un seul fil, lue par un seul fil, mais faite de CINQ champs qui
// doivent s'accorder : un compteur de sequence les publie ensemble. Le lecteur
// ne tourne jamais en boucle — s'il tombe au milieu d'une ecriture il garde la
// derniere version coherente, ce qui coute un bloc et jamais un blocage.
// ---------------------------------------------------------------------------
struct Transport {
  std::atomic<uint32_t> seq;
  double  tempo;
  double  beat;       // beat du projet a l'instant `at_frame`
  double  ts_num;
  // La vitesse de lecture maitresse de REAPER. Elle n'est PAS un facteur de
  // tempo : le tempo du projet ne bouge pas, c'est la ligne de temps entiere
  // qui defile plus vite. Consequence pour nous, et c'est la seule : un beat
  // dure `rate` fois moins d'echantillons. Sans elle, un moteur qui compte ses
  // echantillons continue au tempo nominal pendant que le projet accelere, et
  // les deux se separent lineairement.
  double  rate;
  int     playing;
  frame_t at_frame;

  Transport() : seq(0), tempo(120.0), beat(0.0), ts_num(4.0), rate(1.0),
                playing(0), at_frame(0) {}
};

// ---------------------------------------------------------------------------
// LA CASE AUDIO QUE LA LANE FAIT SONNER (ABI 3.0)
// ---------------------------------------------------------------------------
// Une case audio etait declenchee par Lua : `Cells.drive` lisait l'etat de la
// lane a chaque frame et programmait ses voix. Les frontieres de passe etaient
// datees juste -- le moteur les publie --, mais tout ce que Lua apprenait
// APRES coup partait une frame en retard, 16 a 33 ms : un lancement immediat
// (Q: Off, ou un clic dans la fenetre de tolerance), un arret, un demute, un
// redemarrage du transport.
//
// La lane porte maintenant sa case : un clip du vivier, sa region, son taux,
// et DEUX voix allouees par Lua sur le port de la colonne. C'est le fil audio
// qui les lance, au bloc ou la lane part, a l'echantillon -- comme il datait
// deja ses notes. Lua arme la case, il ne la joue plus.
//
// Ecrite par le fil principal dans le tampon NON publie, puis publiee d'un
// store, comme les notes. Le fil audio en prend une copie en debut de bloc.
struct LaneAudio {
  int32_t  port;     // port des deux voix, -1 = la lane ne porte pas de son
  int32_t  clip;     // slot du vivier
  uint32_t v[2];     // les deux voix de cette moitie (handles)
  int32_t  loop;     // la matiere se repete dans sa passe
  int32_t  pad_;
  double   f0, f1;   // la region, en frames source
  double   sr;       // frequence du clip, frames source par seconde
  double   rate;     // taux de la matiere : celui que le tempo de la case demande
  double   gain;
};

// ---------------------------------------------------------------------------
// LA PRISE AUDIO DANS UNE CASE (ABI 4.2)
// ---------------------------------------------------------------------------
// « Il arrive sur une colonne en IN 1/2, je veux enregistrer un clip audio, et
// rien ne s'enregistre » (Cedric, 2026-10-10). La capture d'une case ne lisait
// que le MIDI (MIDI_GetRecentInputEvent, cote Lua) : une colonne qui ecoute une
// entree AUDIO montrait son niveau, clignotait N mesures, et rendait une case
// vide.
//
// Une capture est attachee a UNE lane et suit SA prise : elle commence a
// l'echantillon ou la lane entre en enregistrement (`rec_start`, la passe que
// `rec_target` a choisie) et dure exactement la passe. Le fil audio ecrit
// l'entree materielle dans un clip du VIVIER -- la memoire que la case jouera
// ensuite. La lane peut donc rejouer la prise des la fin de la passe, sans
// attendre qu'un fichier existe : Lua lui decrit la case audio pendant la
// prise, et `run_pendings` ferme la prise DANS le bloc de sa fin (voir
// `cap_running_on`).
//
// LA LATENCE. Le musicien joue sur ce qu'il ENTEND (la sortie, en retard de la
// latence de sortie) et son son arrive en retard de la latence d'entree. Ce
// qu'il joue « sur le temps » entre donc dans l'entree `lat` echantillons apres
// le temps : la matiere commence a `s0 + lat`. C'est la compensation que
// REAPER applique a ses propres enregistrements.
//
// LA MEMOIRE D'AVANT. Une prise peut commencer dans le PASSE : un clic juste
// apres le temps fort prend CETTE passe (la tolerance de `rec_target`). Tant
// que la capture attend, l'entree s'ecrit dans un anneau (`pre`) ; au depart,
// ce qui est deja passe y est repris. Sans lui, le premier coup -- celui qu'on
// joue precisement sur le temps -- manquait.
//
// LE PARTAGE. Le fil principal pose tout AVANT `state = kCapArmed` (release) et
// ne touche plus a rien tant que l'etat est Armed ou Running. Annuler est une
// DEMANDE (`cancel`) a laquelle le fil audio repond par kCapIdle : c'est le
// seul moment ou l'hote peut rendre la memoire. Une fois Done ou Cut, le fil
// audio n'ecrit plus.
enum : int { kMaxCaptures = 4 };

enum CapState : int {
  kCapIdle    = 0,
  kCapArmed   = 1,   // attend que sa lane entre en prise ; l'anneau se remplit
  kCapRunning = 2,   // la prise est datee, la matiere s'ecrit
  kCapDone    = 3,   // la matiere est entiere (ou arretee plus tot, `end`)
  kCapCut     = 4,   // le transport s'est arrete pendant la prise
};

struct Capture {
  // --- pose par le fil principal AVANT l'armement ------------------------------
  int       lane;
  int       nch;      // 1 ou 2
  int       in0;      // premier canal d'entree materiel (lu par l'hote)
  sample_t* data;     // nch * cap, entrelace : le clip du vivier
  frame_t   cap;
  sample_t* pre;      // nch * npre, circulaire : indice = frame absolu mod npre
  frame_t   npre;
  frame_t   lat;      // la latence compensee, en frames (peut etre negative)
  uint32_t  gen0;     // `rec_gen` de la lane a l'armement
  // --- fil audio ---------------------------------------------------------------
  frame_t   s0;       // frame absolu du debut de la prise
  frame_t   t0;       // s0 + lat : l'echantillon 0 de la matiere
  frame_t   pre_hi;   // frame absolu juste apres le dernier ecrit dans `pre`
  // --- publie ------------------------------------------------------------------
  std::atomic<int>     state;
  std::atomic<int>     cancel;
  std::atomic<int64_t> len;      // frames de la prise, connus au depart
  std::atomic<int64_t> end;      // ou l'ecriture s'arrete, <= len
  std::atomic<int64_t> written;  // frames de matiere ecrits depuis 0
  std::atomic<int64_t> start;    // s0, publie

  Capture() : lane(-1), nch(0), in0(0), data(nullptr), cap(0), pre(nullptr),
              npre(0), lat(0), gen0(0), s0(0), t0(0), pre_hi(0), state(kCapIdle),
              cancel(0), len(0), end(0), written(0), start(0) {}
};

struct Lane {
  // --- fil principal ---------------------------------------------------------
  std::atomic<int>    port;        // -1 = ne parle nulle part
  std::atomic<int>    channel;     // 0..15
  std::atomic<double> bars;        // longueur de boucle, en mesures
  // DEUX MUTES, ET LE MOTEUR FAIT LEUR OU.
  //
  // `muted` est MECANIQUE : « le MIDI de cette lane ne doit pas sortir ». La
  // Session le pose sur une case audio, dont la lane porte une note unique qui
  // ne doit pas atteindre l'instrument de la colonne. C'est du cablage.
  //
  // `user_muted` est MUSICAL : « tais cette lane ». C'est le bouton du Looper,
  // et il doit taire aussi la VOIX de la case audio, qui n'est pas une lane —
  // c'est Lua qui s'en charge, en lisant ce champ.
  //
  // ⚠️ POURQUOI ICI ET NON EN LUA. La separation a d'abord ete ecrite dans deux
  // tables Lua, et c'etait faux : `Loop.lua` est charge SEPAREMENT par chaque
  // fenetre (trois ReaScript, trois etats Lua) alors que la lane est UNE. Chaque
  // fenetre ne voyait donc que la moitie des gestes, et recomposait le OU a
  // partir de ce qu'elle seule avait pose — en ecrasant l'autre moitie. Le mute
  // du Looper n'atteignait jamais la case audio de la Session, c'est-a-dire
  // exactement le defaut que la separation devait corriger.
  //
  // Un fait partage se range la ou il est partage.
  std::atomic<int>    muted;
  std::atomic<int>    user_muted;
  std::atomic<double> tag;         // identite du clip que la lane tient
  // LE DECALAGE DE PHASE — « joue a partir d'ici ».
  //
  // La phase d'une lane est ancree sur le beat ZERO du projet : c'est ce qui
  // verrouille toutes les boucles sur la meme grille, et on n'y touche pas.
  // Mais un decalage CONSTANT ne casse pas ce verrou — il le deplace. La lane
  // reste sur la grille, a distance fixe, et rien ne derive : c'est exactement
  // le legato launch d'Ableton, et c'est ce que veut dire « lire a partir
  // d'ici ».
  //
  // Il est lu au MEME endroit par le portail MIDI et par la phase publiee (que
  // lit Cells pour l'audio) : les deux moteurs doivent voir la meme boucle, ou
  // le son et les notes se separeraient d'un decalage exactement egal a celui
  // qu'on vient de poser.
  //
  // DELIBEREMENT NON PERSISTE. C'est un geste de jeu, comme un scrub ; le
  // retrouver a la reouverture d'un projet serait une surprise, pas un service.
  std::atomic<double> phase_off;   // en beats

  // « QUAND CETTE LANE PARTIRA, QU'ELLE PARTE D'ICI. » En beats, ou < 0 pour
  // « rien de demande ».
  //
  // Poser `phase_off` depuis Lua a la place ne peut pas marcher, et le symptome
  // le dit : le clip demarre a l'ANCIEN endroit pendant une frame, puis saute.
  // Lua ne voit le lancement qu'apres coup — le mode a deja bascule, le premier
  // bloc a deja sonne. Et il ne peut pas non plus le PREVOIR : la frontiere de
  // quantize est choisie ici, exactement pour qu'il n'y ait pas deux horloges
  // qui pourraient diverger.
  //
  // On arme donc l'intention, et c'est le moteur qui la consomme a l'instant
  // ou il choisit la frontiere — il connait `tq`, donc le decalage est exact du
  // premier echantillon.
  std::atomic<double> play_from;   // en beats, < 0 = aucun

  // L'ACCOLADE DE BOUCLE — « je ne veux entendre que ces deux mesures-la ».
  //
  // Une sous-region de la case, en beats depuis son debut. `loop_b <= loop_a`
  // veut dire « pas d'accolade », et c'est l'etat de toutes les lanes : une
  // case boucle sur toute sa longueur tant qu'on ne lui a rien demande.
  //
  // CE N'EST PAS UNE PORTE, C'EST UNE LONGUEUR DE BOUCLE, et toute la valeur
  // de la fonctionnalite tient dans cette distinction. Taire les notes du
  // dehors aurait ete plus simple a ecrire : la case aurait continue de
  // tourner sur ses quatre mesures en n'en faisant sonner que deux, donc deux
  // mesures de musique suivies de deux mesures de silence. Le loop brace
  // d'Ableton fait l'autre chose, la seule qui soit musicale : la case DEVIENT
  // une boucle de deux mesures, qui revient deux fois plus souvent.
  //
  // Le verrouillage de phase y survit intact, parce que c'est exactement le
  // meme verrou : une lane de longueur Ls ancree sur le beat zero du projet.
  // On n'a pas ajoute une horloge, on a change une longueur — et c'est pour ca
  // que ce champ ne coute rien au reste du moteur.
  std::atomic<double> loop_a;      // en beats depuis le debut de la case
  std::atomic<double> loop_b;      // <= loop_a : pas d'accolade

  // ---------------------------------------------------------------------------
  // LA SIGNATURE DE LA BOUCLE — 0 = suivre le projet
  // ---------------------------------------------------------------------------
  // La longueur d'une boucle valait `bars * ts_num`, ou `ts_num` etait la
  // signature rythmique A L'ENDROIT OU LA TETE DE LECTURE SE TROUVE. Une seule
  // mesure en 3/4 quelque part dans le projet changeait donc la longueur de
  // TOUTES les lanes au moment ou le transport la traversait — alors que les
  // notes, elles, sont en beats absolus. La musique se decalait toute seule, et
  // rien ne pouvait l'expliquer depuis la fenetre.
  //
  // Une boucle porte donc la sienne. Zero veut dire « suis le projet », ce qui
  // est le comportement d'avant et donc ce que valent tous les projets deja
  // ecrits — la correction ne change rien tant que personne ne s'en sert.
  std::atomic<double> ts_num;

  std::atomic<int>    nbuf;        // tampon de notes PUBLIE (0 ou 1)
  std::atomic<int>    ncount[2];   // nombre de notes de chaque tampon
  // Les notes elles-memes vivent dans UN bloc, alloue une seule fois par
  // Lanes::init et jamais ensuite. Les poser dans la structure aurait mis
  // 786 Ko dans l'Engine, donc sur la pile de tout ce qui en declare un — le
  // harnais s'y est effondre, et une pile qui deborde ne dit pas pourquoi.
  LaneNote*           buf[2];

  // La case audio (voir LaneAudio) : deux tampons, celui que le fil audio lit
  // est designe par `apub`.
  LaneAudio           abuf[2];
  std::atomic<int>    apub;

  // L'automation (voir AutoSet) : deux lots dans le bloc de `Lanes::init`. Le
  // fil audio lit `autos[auto_gen & 1]` ; une GENERATION et non un indice, pour
  // qu'il sache qu'un lot NEUF est arrive meme apres deux publications entre
  // deux de ses blocs.
  AutoSet*              autos[2];
  std::atomic<uint32_t> auto_gen;

  // --- fil audio -------------------------------------------------------------
  // La case audio, cote fil audio. `au` est la copie de la liaison prise en
  // debut de bloc ; le reste dit ce qui sonne.
  LaneAudio           au;
  int                 au_run;      // une passe sonne, sur la voix `au_cur`
  int                 au_cur;      // 0 ou 1
  double              au_last;     // ou la passe en cours a commence (ligne de la lane)
  double              au_expect;   // ou la lane devrait etre au bloc suivant
  double              au_fresh;    // un depart neuf (beat), < kWaitTest = aucun
  double              au_prate;    // le taux de lecture applique a la voix qui sonne
  std::atomic<int>    au_pub;      // publie : la voix qui sonne, -1 = aucune
  // Quelle note sonne, par hauteur : indice+1, ou 0. C'est LA structure de la
  // porte, et elle n'appartient qu'au fil audio.
  int16_t             sounding[128];
  // DE QUELLE PASSE VIENT LA NOTE QUI SONNE, sur huit bits.
  //
  // La porte reconcilie un ENSEMBLE : tant que la meme note couvre la phase,
  // rien ne change et rien n'est emis. C'est juste pour une note ordinaire, et
  // FAUX pour celle qui remplit sa passe d'un bout a l'autre — elle couvre
  // toutes les phases, donc l'ensemble ne varie jamais, donc elle attaque une
  // fois et plus jamais. Une note aussi longue que la boucle ne se declenchait
  // pas : il fallait la raccourcir d'un cheveu pour l'entendre, ce qui est le
  // genre de contournement qu'on finit par croire normal.
  //
  // Le numero de passe rentre donc dans l'identite du desir. Il se calcule DEJA
  // pour le tirage de probabilite (`onset / Ls`) et ne coute rien de plus : il
  // est CONSTANT pendant une note ordinaire — y compris pour la queue de celle
  // qui traverse la frontiere, qui garde la passe de son attaque — et il change
  // a chaque tour pour celle qui fait le tour. Une coupure et une attaque au
  // meme echantillon : c'est ce que fait n'importe quel sequenceur au point de
  // boucle.
  unsigned char       spass[128];
  // L'automation, cote fil audio : la generation vue, la derniere valeur
  // 14 bits emise par courbe (-1 = a envoyer), et le curseur de segment.
  uint32_t            auto_seen;
  int16_t             auto_last[kMaxAutoCurves];
  int16_t             auto_seg[kMaxAutoCurves];
  // ⚠ L'IDENTITE DE LA NOTE QUI SONNE EST SON DEBUT, PAS SON RANG DANS LA
  // LISTE. `sounding` porte un indice, et l'indice d'une note change des
  // qu'on en pose ou qu'on en efface une AVANT elle : Lua republie la liste
  // triee, tout ce qui suit se decale d'un cran. La porte voyait donc une
  // AUTRE note couvrir la phase, coupait celle qui sonnait et la relancait
  // aussitot -- « quand j'insere ou supprime une note dans l'editeur, ca joue
  // un sample, pas forcement de la rangee selectionnee, et Listen est coupe ».
  // Ce n'etait pas une ecoute : c'etait la lane qui rejouait ce qu'elle jouait
  // deja. L'indice ne sert plus qu'a lire la note DU BLOC COURANT.
  float               sstart[128];
  // Sa FIN, en phase de boucle, relue a chaque bloc tant qu'elle sonne (une
  // note raccourcie pendant qu'elle sonne coupe a sa nouvelle fin). La coupure
  // ne relit plus `notes[cur - 1]`, qui designe une autre note des que la
  // liste a change sous elle.
  double              send[128];
  // ⚠ L'ARRET QUI TOMBE DANS CE BLOC, A SA FRONTIERE (beat), -1 = aucun.
  // Un arret quantifie se resolvait au bloc APRES sa frontiere : pendant le
  // bloc qui la contient, la lane sortante jouait encore -- elle relancait sa
  // boucle A la frontiere, puis coupait ses notes au debut du bloc suivant.
  // Les deux moities d'une colonne partagent port et canal : la note que la
  // case ENTRANTE attaque a la frontiere (le kick du premier temps, une note
  // commune d'accord) etait coupee un bloc plus tard, ou doublee sur un
  // sampler one-shot. `run_pendings` le pose, `run_gate` le consomme : la
  // porte s'arrete juste avant la frontiere et coupe tout un echantillon avant.
  double              stop_due;
  double              rec_start;   // beat auquel la prise en cours a commence

  // --- publie par le fil audio, lu par le principal --------------------------
  std::atomic<int>    mode;
  std::atomic<int>    pending;
  std::atomic<double> pend_target;
  std::atomic<double> phase;
  std::atomic<double> len_beats;
  // La zone de lecture EFFECTIVE, apres bornage. Publiee et non recalculee en
  // Lua : une accolade posee sur quatre mesures puis ramenee a une doit rendre
  // la meme reponse des deux cotes, et deux copies de la regle de bornage
  // finiraient par diverger sur le cas ou elle sert — celui ou l'accolade ne
  // tient plus dans la case.
  std::atomic<double> span_a;
  std::atomic<double> span_len;
  // Incremente a CHAQUE debut de prise. C'est ainsi que Lua sait qu'il doit
  // vider la liste : le fil audio ne touche pas aux notes, il dit seulement
  // qu'une nouvelle prise commence.
  std::atomic<uint32_t> rec_gen;

  Lane();
  void reset();
};

// File de sortie MIDI d'un port. Ecrite et lue par le FIL AUDIO seul (tick()
// et GetSamples tournent tous deux dessus), donc sans atomiques : un tableau
// et un compteur. Les evenements portent un frame ABSOLU ; l'hote rend ceux
// qui tombent dans le bloc qu'il est en train de servir.
struct PortMidi {
  frame_t       at[kPortMidiCap];
  unsigned char msg[kPortMidiCap][3];
  int           count;
  PortMidi() : count(0) { std::memset(at, 0, sizeof(at)); std::memset(msg, 0, sizeof(msg)); }
};

class Lanes {
 public:
  Lanes();
  ~Lanes();

  void init(double srate);
  void set_srate(double srate) { if (srate > 1.0) srate_ = srate; }

  // --- fil principal ---------------------------------------------------------
  // `rate` a une valeur par defaut pour que les appelants qui ne connaissent
  // pas la vitesse de lecture — le harnais, et tout ce qui teste le coeur hors
  // REAPER — restent justes sans avoir a la nommer.
  void publish_transport(double tempo, double beat, int playing, double ts_num,
                         frame_t at_frame, double rate = 1.0);
  bool post(int lane, int cmd, double arg);

  void   set_freerun(bool on)   { freerun_.store(on ? 1 : 0, std::memory_order_relaxed); }
  bool   freerun() const        { return freerun_.load(std::memory_order_relaxed) != 0; }
  void   set_launch_q(double b) { launch_q_.store(b, std::memory_order_relaxed); }
  double launch_q() const       { return launch_q_.load(std::memory_order_relaxed); }
  // « Quelque chose A MOI sonne » — dit par un hote qui joue des cases audio,
  // que ce moteur-ci ne voit pas. L'horloge libre est le transport de la
  // SESSION, et il resterait a zero sous un echantillon qui joue.
  void set_audio_run(bool on)   { audio_run_.store(on ? 1 : 0, std::memory_order_relaxed); }

  // Position de l'horloge sur laquelle ce moteur travaille : le beat de l'hote
  // quand il le suit, sa propre horloge libre sinon. Les cibles en attente sont
  // exprimees sur CETTE ligne de temps.
  double engine_beat() const { return pub_beat_.load(std::memory_order_relaxed); }
  bool   clock_running() const { return pub_active_.load(std::memory_order_relaxed) != 0; }

  Lane& lane(int i) { return lanes_[i]; }
  const Lane& lane(int i) const { return lanes_[i]; }

  // Ecriture des notes : le principal remplit le tampon NON publie, puis
  // publie. Rend le tampon a remplir, ou nullptr si la lane est hors bornes.
  LaneNote* write_buf(int lane);
  void      publish_notes(int lane, int count);
  int       note_count(int lane) const;
  const LaneNote* read_buf(int lane) const;

  // L'automation (ABI 4.0). `auto_back` rend le lot qui DORT, a remplir en
  // entier ; `auto_publish` le valide (dates croissantes, CC dans 0..31), en
  // calcule les facteurs de courbe et le publie. Rend false sans rien publier
  // si le lot est refuse.
  AutoSet*  auto_back(int lane);
  bool      auto_publish(int lane);
  int       auto_count(int lane) const;
  uint32_t  auto_sent() const { return auto_sent_.load(std::memory_order_relaxed); }

  // --- fil audio -------------------------------------------------------------
  // Un bloc. `clock` est le frame absolu du PREMIER echantillon a produire,
  // `frames` sa longueur. Appele depuis Engine::tick, donc une fois par bloc
  // materiel et jamais par port.
  void tick(frame_t clock, int frames);

  // Rend les evenements du port qui tombent dans [from, to), les retire de la
  // file, et laisse les autres. Rend le nombre ecrit dans out_at/out_msg.
  int drain_midi(int port, frame_t from, frame_t to,
                 frame_t* out_at, unsigned char (*out_msg)[3], int cap);

  // Tout couper, sans laisser une note tenue derriere soi.
  void all_notes_off();

  // --- la case audio (ABI 3.0) -------------------------------------------------
  // Fil principal : `audio_back` rend le tampon non publie, recopie du publie
  // (on n'y change donc que ce qu'on veut) ; `audio_publish` le publie.
  LaneAudio* audio_back(int lane);
  void       audio_publish(int lane);
  // Fil audio, cote rendu : les commandes de voix que les lanes ont posees pour
  // ce port. L'Engine les applique apres celles du fil principal, dans le meme
  // bloc. Une file a part parce que l'anneau du port a deja un producteur -- le
  // fil principal -- et qu'un anneau SPSC n'en supporte qu'un.
  bool       pop_vcmd(int port, Cmd& c) {
    return (port >= 0 && port < kMaxPorts) ? vout_[port].pop(c) : false;
  }

  uint32_t dropped_commands() const { return dropped_.load(std::memory_order_relaxed); }

  // --- la prise audio (ABI 4.2, voir Capture) ----------------------------------
  // Fil principal. `data` (un clip du vivier) et `pre` appartiennent a l'hote
  // et doivent vivre jusqu'a ce que l'emplacement redevienne libre. Rend false
  // si l'emplacement n'est pas libre ou si les arguments ne tiennent pas.
  bool cap_arm(int slot, int lane, int nch, int in0, sample_t* data,
               frame_t cap, sample_t* pre, frame_t npre, frame_t lat);
  // Armee ou en cours : on DEMANDE, le fil audio repond (kCapIdle). Finie
  // (Done, Cut) : elle redevient libre tout de suite. Rend true si
  // l'emplacement est libre en sortant -- sa memoire peut alors etre rendue.
  bool cap_release(int slot);
  const Capture* cap(int slot) const {
    return (slot >= 0 && slot < kMaxCaptures) ? &caps_[slot] : nullptr;
  }
  // Fil audio, passage PRE du crochet materiel : l'entree du bloc qui commence
  // au frame absolu `clock`. `in0`/`in1` peuvent etre nuls (canal absent).
  void cap_feed(int slot, frame_t clock, int frames, const double* in0,
                const double* in1);
  // L'hote doit-il lire l'entree pour cet emplacement ?
  bool cap_wants_input(int slot) const {
    if (slot < 0 || slot >= kMaxCaptures) return false;
    const int st = caps_[slot].state.load(std::memory_order_acquire);
    return st == kCapArmed || st == kCapRunning;
  }

  // --- diagnostic (lu par le fil principal, ecrit par le fil audio) ----------
  // Ce que le fil audio a fait d'inhabituel : il ne peut rien journaliser
  // lui-meme (ni E/S, ni allocation), il COMPTE, et le fil principal ecrit.
  //   follow_resyncs : l'horloge suivie s'est recalee sur l'hote (un saut)
  //   audio_cuts     : une case audio a ete coupee net (saut, arret, mute)
  //   lane_flushes   : une lane a relache ses notes tenues
  //   lane_launches  : une lane est passee a « joue »
  uint32_t follow_resyncs() const { return fol_resync_.load(std::memory_order_relaxed); }
  uint32_t audio_cuts() const     { return au_cuts_.load(std::memory_order_relaxed); }
  uint32_t lane_flushes() const   { return flushes_.load(std::memory_order_relaxed); }
  uint32_t lane_launches() const  { return launches_.load(std::memory_order_relaxed); }

 private:
  // L'horloge du mode Suivre, debruitee : voir `tick`.
  double  follow_clock(double host, bool playing, double block_beats, int frames);

  double  lane_len_beats(int li, double ts_num) const;
  // La zone de lecture effective d'une lane : `a` son debut dans la case,
  // `len` sa longueur. Sans accolade, (0, longueur de la case). Tout ce qui
  // LIT la lane passe par ici — un seul endroit sait ce qu'une accolade veut
  // dire, et il est du cote du fil audio qui s'en sert.
  void    lane_span(int li, double ts_num, double* a, double* len) const;
  double  launch_target(double pb, bool active) const;
  // OU COMMENCE UNE PRISE. Pas la meme question que « ou tombe un depart » :
  // voir cp_lanes.cpp.
  double  rec_target(int li, double pb, bool active, double ts_num) const;
  double  stop_target(double pb, bool active) const;
  void    emit(int port, frame_t at, unsigned char s, unsigned char d1,
               unsigned char d2);
  void    flush_lane(int li, frame_t at);
  // Les courbes d'une lane qui joue, a la position `pos` (beats de clip).
  void    run_auto(int li, int port, int ch, double pos, frame_t at);
  void    drain_cmds(double pb, bool active, double ts_num, frame_t at);
  // Consomme `play_from` : la lane part a `at_beat`, on veut qu'elle y soit a
  // la phase demandee. Appelee partout ou un mode devient « joue ».
  void    take_play_from(int li, double at_beat, double ts_num);
  void    run_pendings(double pb, bool active, double ts_num, frame_t at,
                       double block_beats);
  void    run_gate(double pb, bool active, double ts_num, frame_t at,
                   int frames, double block_beats);
  // La case audio d'une lane, un bloc : entree, frontieres de passe, arret.
  void    run_audio(int li, double pb, double ts_num, frame_t at, int frames,
                    double block_beats);
  void    au_start(int li, double sb, double pb, frame_t at, frame_t last,
                   double bpf, double block_beats, double sa, double Ls,
                   double off, bool relaunch);
  void    au_cut(int li);
  void    au_post(int port, uint32_t type, uint32_t voice, frame_t at,
                  double a, double b, uint32_t u0, uint32_t u1);
  // La prise audio, cote fil audio : une capture ecrit-elle pour cette lane ?
  // Et, une fois par bloc, ce que les lanes viennent de decider (une prise
  // commence, finit, s'arrete avec le transport).
  bool    cap_running_on(int li) const;
  void    cap_track(double pb, double block_beats, int frames, frame_t clock,
                    double ts_num, bool active);

  Lane      lanes_[kMaxLanes];
  Capture   caps_[kMaxCaptures];
  LaneNote* note_store_;         // kMaxLanes * 2 * kMaxLaneNotes, alloue a l'init
  AutoSet*  auto_store_;         // kMaxLanes * 2, alloue a l'init
  PortMidi  pmidi_[kMaxPorts];
  // Les commandes de voix des cases audio, par port. Ecrites par `tick`,
  // drainees par `Engine::render_port` : deux appels qui ne tournent pas
  // forcement sur le meme fil, d'ou un anneau et non un tableau.
  SpscRing<Cmd, 64> vout_[kMaxPorts];
  // Le tempo et le taux de lecture EFFECTIF (1 en horloge libre) du bloc en
  // cours, poses par `tick` pour la case audio.
  double    au_tempo_;
  double    au_prate_;
  SpscRing<LaneCmd, 64> cmds_;

  Transport tr_;
  Transport tr_cache_;          // fil audio : la derniere version coherente

  std::atomic<int>    freerun_;
  std::atomic<double> launch_q_;
  std::atomic<int>    audio_run_;
  std::atomic<double> pub_beat_;
  std::atomic<int>    pub_active_;
  std::atomic<uint32_t> dropped_;

  double  srate_;
  double  free_beat_;
  int     prev_active_;
  int     prev_freerun_;
  double  last_pb_;

  // L'HORLOGE SUIVIE. Le beat que le moteur se donne en mode Suivre, avance par
  // son propre compte d'echantillons ; l'ancre de l'hote ne fait que le
  // corriger. `anchor_seq_` est la sequence de la derniere ancre lue : une
  // ancre NEUVE est la seule qui puisse confirmer un saut.
  double   fol_pb_;
  int      fol_ok_;
  uint32_t anchor_seq_;
  uint32_t fol_seq_;
  int      fol_miss_;
  double   fol_err_;

  std::atomic<uint32_t> fol_resync_;
  std::atomic<uint32_t> au_cuts_;
  std::atomic<uint32_t> flushes_;
  std::atomic<uint32_t> launches_;
  std::atomic<uint32_t> auto_sent_;
};

} // namespace cp

