// CP_Native — LA CLOCK MIDI D'UN AUTRE (ABI 3.9).
//
// Une machine maitresse (Ableton sur un autre PC, une boite a rythmes) envoie
// 24 impulsions par noire (0xF8), plus Start (0xFA), Continue (0xFB), Stop
// (0xFC) et la position (0xF2, en doubles-croches). REAPER sait suivre la
// POSITION de cette horloge (synchronisation externe) mais ne change jamais le
// TEMPO du projet : tout ce qui se cale sur le tempo -- les cases de la
// session, les LFO, le sequenceur, les delais -- tournait au tempo d'avant.
//
// Cette classe ne fait que le calcul. Le fil audio lui donne les messages,
// dates en echantillons du moteur ; le fil principal lit un instantane.
//
// ---------------------------------------------------------------------------
// LE TEMPO EST UNE PENTE, PAS UN INTERVALLE
// ---------------------------------------------------------------------------
// Une impulsion arrive tous les 20 ms a 120 BPM, et l'USB, le pilote et les
// horodatages de Windows (a la milliseconde) la decalent d'une a deux ms : un
// intervalle isole se trompe de 5 a 10 %. Moyenner les intervalles ne vaut pas
// mieux : la moyenne de N intervalles n'est que (dernier - premier) / N, deux
// points seulement. On ajuste donc une DROITE (moindres carres) sur les
// instants des 384 dernieres impulsions -- seize noires. Sa pente est le
// tempo, a quelques milliemes de BPM, et elle DEBRUITE aussi la phase : la
// position du maitre se lit sur la droite, pas sur la derniere impulsion.
//
// Un changement de tempo se voit sur une droite COURTE (la derniere noire) qui
// s'ecarte de la longue plus que son bruit ne l'explique, douze impulsions de
// suite : la fenetre longue repart alors de la courte. Un saut de 124 a 128 BPM
// est pris en une noire et demie ; une derive de quelques ppm entre les deux
// cartes son, jamais -- et c'est voulu, c'est le travail de la phase.
//
// ---------------------------------------------------------------------------
// LA POSITION EST CELLE DE L'IMPULSION SUIVANTE
// ---------------------------------------------------------------------------
// Apres Start, la norme MIDI dit que le maitre est a zero A LA PROCHAINE
// impulsion. Apres un SPP + Continue, a la position du SPP. On compte donc la
// position de l'impulsion SUIVANTE (`pos_next_`), et chaque 0xF8 recu en
// mouvement prend cette position puis l'avance d'une. Entre Start et la
// premiere impulsion, la position est figee a celle qui vient.
//
// Une impulsion PERDUE (un octet mange par l'interface) decalerait toute la
// suite d'un 24e de noire, et la droite d'un point. Un trou d'un nombre entier
// d'intervalles (2 a 4) est donc rempli : la position avance d'autant. Au-dela,
// le maitre s'etait arrete ; la fenetre repart.
//
// ⚠ UN TROU DE DEUX INTERVALLES EST AUSSI UN TEMPO DIVISE PAR DEUX. 140 -> 70
// ressemble, impulsion apres impulsion, a « une sur deux perdue » : les
// remplir garderait la mesure a 140 et ferait courir la position deux fois
// trop vite. Le trou reste donc EN SUSPENS jusqu'a l'impulsion suivante : si
// elle revient a l'intervalle d'avant, c'etait une perte ; sinon, c'etait le
// tempo. La position a une impulsion de retard pendant ce temps, rien de plus.
#pragma once

#include <atomic>
#include <cstdint>

namespace cp {

// Ce que le fil principal lit, d'un coup (seqlock).
struct ExtClockSnap {
  double   bpm;        // la pente longue (0 = jamais vu)
  double   bpm_err;    // son ecart-type estime, en BPM
  double   bpm_short;  // la pente de la derniere noire
  double   jitter_ms;  // ecart-type des impulsions autour de la droite
  double   beats;      // largeur de la fenetre, en noires
  double   t_last;     // instant AJUSTE de la derniere impulsion (echantillons)
  double   pos_last;   // position du maitre a cette impulsion (impulsions)
  double   pos_next;   // position de la prochaine (figee a l'arret)
  double   spt;        // echantillons par impulsion
  int      flags;
  uint32_t starts, stops, ticks;
};

class ExtClock {
 public:
  enum : int {
    kRing  = 384,   // seize noires
    kShort = 24,    // une noire
  };
  enum : int {
    kPresent = 1,   // une impulsion recente
    kRunning = 2,   // le maitre joue (Start / Continue, pas encore Stop)
    kHasPos  = 4,   // on sait OU il joue (un Start ou un SPP a ete vu)
    kSettled = 8,   // la fenetre couvre au moins deux noires
    kWaiting = 16,  // Start recu, premiere impulsion pas encore
  };

  ExtClock();

  void set_srate(double sr);
  // Oublie tout, sauf la pente (bonne graine pour la suite).
  void reset();

  // Fil audio, dans l'ordre d'arrivee. t = temps du moteur en echantillons.
  void tick(double t);
  void start(double t);
  void cont(double t);
  void stop(double t);
  void spp(int sixteenths);
  // Fin de bloc : presence, puis publication de l'instantane.
  void end_block(double now);

  // Fil principal. Faux tant qu'aucune publication n'a eu lieu.
  bool read(ExtClockSnap& out) const;
  // La position du maitre, en noires, a l'instant t (echantillons du moteur).
  static double qn_at(const ExtClockSnap& s, double t);

  // Pour le harnais : l'etat sans passer par la publication.
  const ExtClockSnap& live() const { return snap_; }

 private:
  void push(double t);
  void accept(double t, int64_t fill, double from);
  void settle_pending();
  void fit();

  double   srate_;
  double   ring_[kRing];
  int      head_;      // prochaine case ecrite
  int      count_;     // impulsions dans la fenetre (<= kRing)
  double   spt_;       // pente courante, graine comprise
  bool     have_spt_;
  double   t_fit_;     // instant ajuste de la derniere impulsion
  double   t_seen_;    // instant brut de la derniere impulsion
  bool     seen_;
  int64_t  pos_next_;
  double   pos_last_;
  bool     has_pos_;
  bool     running_;
  bool     waiting_;
  bool     pend_;      // une impulsion apres un trou, pas encore jugee
  double   t_pend_;
  double   t_pend_from_;
  int64_t  n_pend_;    // combien manqueraient si c'etait une perte
  int      dev_run_;   // impulsions de suite ou la pente courte s'ecarte
  double   jitter_;    // en echantillons
  double   bpm_short_;
  double   bpm_err_;
  uint32_t starts_, stops_, ticks_;

  ExtClockSnap snap_;

  // Publication : un compteur impair pendant l'ecriture.
  enum { kPubFields = 14 };
  std::atomic<uint32_t> seq_;
  std::atomic<double>   pub_[kPubFields];
};

}  // namespace cp

