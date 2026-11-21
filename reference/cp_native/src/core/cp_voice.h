// Une voix : un clip, une position, un rendez-vous.
//
// La voix est la SEULE chose que le moteur connait. Pas de scene, pas de
// colonne, pas de cellule — c'est le principe de propriete du dossier §2.1.
// Une politique de session se decrit entierement avec « joue au frame N »,
// « arrete au frame M » et « enchaine sur la voix H ».
//
// PROPRIETE : cette structure appartient au FIL AUDIO, integralement. Le fil
// principal ne l'ecrit jamais — il pose un mot de propriete (claim_t, cp_types.h)
// et poste une commande. Les trois seules valeurs qu'il LIT sont publiees en fin
// de bloc dans les atomiques `pub_*` : une lecture de dessin, qui peut avoir un
// bloc de retard et pour qui c'est sans importance.
#pragma once

#include <atomic>
#include "cp_types.h"

namespace cp {

class Pool;

// ---------------------------------------------------------------------------
// L'ENVELOPPE A POINTS (ABI 3.3) -- celle qu'on DESSINE dans l'editeur, a la
// maniere d'une enveloppe de volume de REAPER : des points dates en temps
// SOURCE, un gain lineaire chacun, une courbe par segment.
//
// ⚠ DATEE EN TEMPS SOURCE, PAS EN TEMPS DE SORTIE, et c'est toute la
// difference avec l'ADSR. Le gain se lit a la POSITION DE LECTURE : il suit la
// matiere quel que soit le taux, la boucle, l'entree a la phase ou le saut de
// tete. C'est ce qui permettra un jour de redessiner les impacts d'une boucle
// de batterie -- un point pose sur un coup reste sur ce coup.
//
// `k` est le facteur de courbe du segment qui PART de ce point, calcule a la
// validation : 8^(-c) s'il monte, 8^(c) s'il descend -- positif = bombe vers
// le haut, la convention de l'ADSR et du JSFX du kit. Le dernier point n'a pas
// de segment : son `k` ne sert pas.
// ---------------------------------------------------------------------------
struct EnvPt {
  float t;   // secondes source (un float suffit : 7,6 us d'erreur a 64 s)
  float g;   // gain lineaire, 0..4
  float k;   // en attente : la courbe brute ; actif : le facteur de shp
};

// Les deux tableaux d'une voix : celui que le rendu lit, et celui que les
// commandes remplissent. Une validation les ECHANGE -- le rendu ne voit donc
// jamais un lot a moitie ecrit, meme si l'anneau est draine au milieu de
// l'envoi. Sur le tas (Engine), pas dans la voix : 1,5 Ko x 256 voix dans
// l'Engine le ferait deborder d'une pile de 1 Mo, et seules les voix a points
// les touchent.
struct EnvPts {
  EnvPt a[2][kMaxEnvPts];
};

struct Voice {
  // --- identite -------------------------------------------------------------
  int      state;        // VoiceState
  uint16_t gen;          // generation courante, telle que le fil audio la connait
  int      port;         // -1 si non assignee

  // --- matiere --------------------------------------------------------------
  int      clip;         // slot du vivier, -1 si aucun
  double   pos;          // position de lecture en frames source (fractionnaire)

  // --- rendez-vous ----------------------------------------------------------
  frame_t  start_at;     // frame absolu du premier echantillon audible
  frame_t  stop_at;      // frame absolu de la coupure, ou INT64_MAX
  int      mode;         // PlayMode

  // --- transformation -------------------------------------------------------
  double   rate;         // 1.0 = vitesse d'origine
  float    gain;         // lineaire, applique avec rampe
  float    gain_target;
  float    pan;          // -1..+1
  frame_t  loop_start;   // en frames source
  frame_t  loop_end;     // 0 = fin du clip

  // --- fondus ---------------------------------------------------------------
  int      fade_in_len;    // en frames : la PROPRIETE, gardee d'un lancement a l'autre
  int      fade_in_cur;    // le fondu de CE lancement-ci. Vaut fade_in_len, sauf
                           // quand un lancement tombe sur une voix qui sonne encore :
                           // le moteur impose alors un minimum, parce qu'une attaque
                           // seche par-dessus une coupure seche est un clic garanti.
  int      fade_in_pos;
  int      fade_out_len;
  int      fade_out_pos;

  // --- enveloppe ADSR (ABI 3.0) ----------------------------------------------
  // LA MEME QUE CELLE DE L'INSTRUMENT DU KIT (CP_KitSampler.jsfx), etage par
  // etage, parce qu'un son se regle UNE fois -- dans l'editeur -- et doit
  // sonner pareil sur un pad, dans une case de la Session et a l'ecoute. Une
  // troisieme copie de la regle, donc ecrite contre la source et verifiee au
  // harnais (`test_voice_adsr`).
  //
  //   * chaque etage avance une progression lineaire 0..1 (`env_x`) en temps
  //     de SORTIE, et le niveau en est une courbe shp(x, k) = x / (x + k(1-x)),
  //     une droite a k = 1 ;
  //   * attaque 0 -> 1, decroissance 1 -> maintien, maintien tenu ;
  //   * « gate » a zero ouvre le relache DEPUIS le niveau courant, en
  //     niveau x relache secondes ;
  //   * un ONE-SHOT finit par une RAMPE multiplicative qui touche zero au
  //     dernier echantillon de la region, sur `env_r` secondes : un relache
  //     plus long que le son le fait descendre en entier, il ne le tait pas
  //     (c'etait le « plus aucun son ne sort » du kit, corrige le 2026-10-04).
  //
  // Des PROPRIETES, gardees d'un lancement a l'autre comme les fondus : un
  // lancement ne remet que l'etage et le niveau. Eteinte, elle ne coute qu'un
  // test par echantillon, toujours predit.
  bool     env_on;
  float    env_a, env_d, env_s, env_r;   // s, s, niveau, s
  float    env_ka, env_kd, env_kr;       // courbes en k (8^c), 1 = droite
  int      env_st;     // 0 attaque, 1 decroissance, 2 maintien, 3 relache
  double   env_x;      // progression de l'etage courant
  float    env_lv;     // niveau courant
  float    env_rl0;    // niveau au depart du relache
  double   env_rxi;    // pas du relache, par echantillon de sortie

  // --- enveloppe A POINTS (ABI 3.3) -------------------------------------------
  // Exclusive de l'ADSR : valider des points eteint l'ADSR, `env` 1 eteint les
  // points, `env` 0 eteint tout. Une PROPRIETE aussi, gardee d'un lancement a
  // l'autre ; un lancement ne remet que le curseur.
  EnvPts*  envp;       // ses deux tableaux, poses par l'Engine, jamais remis a zero
  bool     envp_on;
  int      envp_n;     // points du tableau actif
  int      envp_act;   // 0 ou 1 : le tableau que le rendu lit
  int      envp_rx;    // points recus depuis la derniere validation
  int      envp_i;     // segment courant : le curseur du rendu

  // --- croisement sur SAUT DE TETE --------------------------------------------
  // Deplacer la tete d'une voix qui sonne (kParamPos, ou un Play sur le MEME
  // clip) etait un saut brut : la sortie passait d'une valeur a une autre sans
  // transition, et ca s'entendait a chaque clic dans une forme d'onde. L'ancienne
  // tete continue donc de lire pendant `tail_len` frames, en fondu descendant,
  // pendant que la nouvelle monte : la somme est constante pour une matiere
  // coherente, et douce pour le reste. Une branche previsible par echantillon,
  // et seulement pendant le croisement.
  double   tail_pos;       // position de l'ancienne tete, en frames source
  int      tail_len;       // frames du croisement, 0 = aucun en cours
  int      tail_i;         // avancement

  // --- enchainement ---------------------------------------------------------
  // Correction §11.12 : legato, fondu croise et fin exacte ont une fenetre d'UN
  // bloc. Lua ne peut structurellement pas la tenir (16 a 74 ms par frame). Le
  // moteur possede donc UN emplacement « suivant » par voix. Un champ, pas un
  // modele de session.
  int      next_voice;   // index, -1 si aucun
  int      xfade_len;    // frames de fondu croise a l'enchainement
  // Frame ABSOLU du premier echantillon reellement audible, -1 tant que la voix
  // n'a pas demarre. C'est la verite terrain de l'attaque : la mesurer de
  // l'exterieur en lisant (horloge, position) expose a une course — dans un
  // bloc, `pos` avance au pull de l'apercu et l'horloge au passage post du hook,
  // donc une lecture qui tombe entre les deux voit un bloc d'ecart. Ici, aucune
  // course : c'est la voix elle-meme qui note l'instant.
  frame_t  started_at;

  frame_t  ended_at;     // frame ABSOLU ou la voix s'est eteinte, -1 sinon.
                         // C'est ce qui rend l'enchainement exact a
                         // l'echantillon plutot qu'exact au bloc.

  // Le fil principal a rendu cet emplacement ; le fil audio doit l'eteindre puis
  // effacer le bit de propriete. Tant que ce drapeau est pose, l'emplacement ne
  // peut PAS etre realloue — c'est ce qui rend la reutilisation sure.
  bool     free_pending;

  // --- publication vers le fil principal ------------------------------------
  // Ecrites une fois par bloc, jamais dans la boucle chaude. En relaxed : il n'y
  // a rien a ordonner autour d'elles, seulement a rendre la lecture definie.
  std::atomic<double>  pub_pos;
  std::atomic<int>     pub_state;
  std::atomic<int64_t> pub_started;
  // QUEL CLIP CETTE VOIX TIENT, lisible depuis le fil principal. Il n'y avait
  // aucune facon definie de le demander : `clip` appartient au fil audio, et le
  // vivier devait donc DEVINER si sa matiere etait encore jouee. Il devinait
  // « non » — le garde-fou `Clip::refs` n'etait incremente nulle part.
  std::atomic<int>     pub_clip;

  void reset();
  bool active() const { return state != kVoiceIdle; }

  // L'enveloppe a points, cote fil audio (appelees par Engine::apply). Un
  // point va dans le tableau en attente ; la validation l'echange avec
  // l'actif si le lot est complet et ordonne, et rend false sinon.
  void env_point(int idx, double t, double g, double c);
  bool env_commit(int n);

  // Publie l'etat visible depuis Lua. Appelee une fois par bloc et par voix, a
  // la fin du rendu du port.
  void publish() {
    pub_pos.store(pos, std::memory_order_relaxed);
    pub_state.store(state, std::memory_order_relaxed);
    pub_started.store((int64_t)started_at, std::memory_order_relaxed);
    pub_clip.store((state == kVoiceIdle) ? -1 : clip, std::memory_order_relaxed);
  }

  // Rend et ADDITIONNE dans out (entrelace, nch canaux, frames images).
  // block_start est le frame absolu du premier echantillon du tampon.
  // Aucune allocation, aucun verrou, aucun appel systeme.
  void render(const Pool& pool, sample_t* out, int frames, int nch,
              frame_t block_start, double engine_srate);
};

} // namespace cp

