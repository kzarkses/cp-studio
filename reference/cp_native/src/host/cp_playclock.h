#ifndef CP_PLAYCLOCK_H
#define CP_PLAYCLOCK_H

// ============================================================================
// L'HORLOGE DU TRANSPORT, TENUE PAR LA DLL (le socle, lot 8, 2026-10-26)
// ============================================================================
// ⚠⚠ « LA TETE DE LECTURE SCINTILLE », ET ELLE AVAIT DEUX PROPRIETAIRES. Lua
// lisait `GetPlayPosition` a chaque passe -- une position qui avance PAR BLOC
// audio, lue a un instant quelconque du bloc --, la filtrait, et re-ancrait la
// liaison de la tete ; entre deux passes, la DLL la prolongeait. Chaque passe
// corrigeait donc la tete d'une fraction de l'erreur de mesure, d'un COUP :
// un pas a la couture, trente fois par seconde, d'autant plus grand que le
// zoom est fort. Un filtre plus doux ne fait que deplacer le compromis entre
// ce pas et le retard : tant que la correction est un SAUT, elle se voit.
//
// Ici, un seul proprietaire et une seule regle :
//   · la position est une DROITE `p(t) = base + vitesse * (t - t_base)`, que le
//     compositeur evalue a chaque image et que Lua lit par la meme fonction
//     (`CP_UIPlayHead`) -- personne d'autre ne calcule la tete ;
//   · une mesure ne DEPLACE jamais la droite : on la re-ancre la ou elle passe
//     a cet instant (continuite), et l'ecart se rattrape par la VITESSE, sur
//     une demi-seconde, a 3 % au plus. Un pas de position devient une pente
//     a peine differente : invisible ;
//   · un vrai saut (on clique ailleurs, on boucle a la main) se reconnait a ce
//     qu'il PERSISTE : deux mesures de suite au-dela de la tolerance, et la
//     droite repart de la mesure. Une mesure isolee qui s'egare est ignoree ;
//   · la boucle du transport (repetition allumee) est dans la droite : passee
//     la fin, la position revient au debut, au meme instant que REAPER, sans
//     attendre une mesure.
//
// ⚠ PUR, ET C'EST CE QUI LE REND VERIFIABLE : ni REAPER, ni horloge, ni fil.
// `cp_main.cpp` le nourrit (le battement du moteur, fil principal) et publie
// l'etat ; le compositeur l'evalue. `gfx_test` le joue sur une position par
// blocs, mesuree a des instants irreguliers.

#include <cmath>

namespace cp {
namespace playclock {

// Ce que le compositeur lit : une droite, et la boucle ou elle se replie.
struct State {
  bool   playing;
  double base;     // position (s, ce qu'on ENTEND) a `t_base`
  double t_base;   // l'instant (s, l'horloge de la machine)
  double speed;    // secondes de projet par seconde, correction comprise
  double ls, le;   // la boucle, quand elle reboucle (le > ls), sinon 0, 0
};

inline State stopped_at(double pos) {
  State s;
  s.playing = false; s.base = pos; s.t_base = 0.0; s.speed = 0.0;
  s.ls = 0.0; s.le = 0.0;
  return s;
}

// La position a l'instant `t`. A l'arret, la ou on s'est arrete.
//
// ⚠ LE REPLIEMENT VAUT POUR UNE TETE PARTIE AVANT LA FIN DE LA BOUCLE : celle
// qui y est, et celle qui y arrive par la gauche (REAPER reboucle a la fin,
// d'ou qu'on vienne). Une tete partie apres la fin la laisse derriere elle.
inline double at(const State& s, double t) {
  if (!s.playing) return s.base;
  double p = s.base + s.speed * (t - s.t_base);
  if (s.le > s.ls && s.base < s.le && p >= s.le) {
    const double L = s.le - s.ls;
    p = s.ls + std::fmod(p - s.ls, L);
  }
  return p;
}

class Filter {
 public:
  // Le rattrapage : l'ecart se resorbe en `kCatchS` secondes, la vitesse ne
  // s'ecartant jamais de plus de `kSlew` de celle du transport.
  static constexpr double kCatchS = 0.5;
  static constexpr double kSlew = 0.03;
  // L'amorce : apres un depart ou un saut, la mesure fait foi pendant ce
  // temps -- `GetPlayPosition` reste sur place le temps de la latence de
  // sortie, puis repart ; le depart de la tete se voit de toute facon.
  static constexpr double kPrimeS = 0.3;

  Filter() : s_(stopped_at(0.0)), rate_(1.0), t_reset_(-1e9), strikes_(0) {}

  const State& state() const { return s_; }

  // Une mesure : la position qu'on ENTEND (`GetPlayPosition`) a l'instant `t`,
  // la vitesse du transport, sa boucle (le <= ls : pas de boucle) et la
  // tolerance au-dela de laquelle un ecart n'est plus du bruit (deux blocs et
  // demi, 25 ms au moins).
  void observe(double t, bool playing, double meas, double rate,
               double ls, double le, double tol) {
    if (!(rate > 0.0)) rate = 1.0;
    if (!(le > ls)) { ls = 0.0; le = 0.0; }
    if (!playing) {
      s_ = stopped_at(meas);
      strikes_ = 0;
      return;
    }
    const bool changed = !s_.playing || rate != rate_ || ls != s_.ls || le != s_.le;
    if (changed) { reset(t, meas, rate, ls, le); return; }
    const double pred = at(s_, t);
    double e = meas - pred;
    // Dans la boucle, un ecart d'un tour n'en est pas un : une mesure prise
    // juste apres le repliement contre une prediction juste avant.
    if (le > ls) {
      const double L = le - ls;
      if (e > 0.5 * L) e -= L; else if (e < -0.5 * L) e += L;
    }
    if (e > tol || e < -tol) {
      // Un vrai saut PERSISTE ; une mesure qui s'egare, non.
      if (++strikes_ >= 2) reset(t, meas, rate, ls, le);
      return;
    }
    strikes_ = 0;
    remember(t, meas);
    if (t - t_reset_ < kPrimeS) {
      // L'amorce : la mesure fait foi, la vitesse est celle du transport.
      s_.base = meas; s_.t_base = t; s_.speed = rate;
      return;
    }
    // Le bord haut (a rebours : bas) des mesures de la fenetre, ramenees a
    // maintenant par la vitesse nominale. Voir `kEnvS`.
    const double sg = (rate > 0.0) ? 1.0 : -1.0;
    for (int i = 0; i < on_; ++i) {
      const int j = (oh_ - 1 - i + kEnvN) % kEnvN;
      const double dt = t - ot_[j];
      if (dt > kEnvS) break;
      double ei = om_[j] - (pred - rate * dt);
      if (le > ls) {
        const double L = le - ls;
        if (ei > 0.5 * L) ei -= L; else if (ei < -0.5 * L) ei += L;
      }
      if (ei > tol || ei < -tol) continue;
      if (sg * ei > sg * e) e = ei;
    }
    // ⚠⚠ CONTINUITE : on re-ancre la ou la droite passe MAINTENANT, et
    // l'ecart part dans la pente. Jamais un pas de position.
    double corr = e / kCatchS;
    const double lim = kSlew * rate;
    if (corr > lim) corr = lim; else if (corr < -lim) corr = -lim;
    s_.base = pred; s_.t_base = t; s_.speed = rate + corr;
  }

 private:
  void reset(double t, double meas, double rate, double ls, double le) {
    s_.playing = true; s_.base = meas; s_.t_base = t; s_.speed = rate;
    s_.ls = ls; s_.le = le;
    rate_ = rate; t_reset_ = t; strikes_ = 0;
  }

  State  s_;
  double rate_;
  double t_reset_;
  int    strikes_;
};

} // namespace playclock
} // namespace cp

#endif // CP_PLAYCLOCK_H

