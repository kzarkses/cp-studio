// CP_Native — la clock MIDI d'un autre. Voir cp_extclock.h pour le pourquoi.
#include "cp_extclock.h"

#include <cmath>

namespace cp {

// La plage plausible. Hors d'elle, une pente vient d'un melange (deux
// peripheriques, un taux qui change) et non d'un tempo.
static const double kMinBpm = 20.0;
static const double kMaxBpm = 400.0;

// Le champ publie, par numero. kPubFields dans l'en-tete les compte.
enum {
  F_BPM, F_BPM_ERR, F_BPM_SHORT, F_JITTER, F_BEATS, F_T_LAST, F_POS_LAST,
  F_POS_NEXT, F_SPT, F_FLAGS, F_STARTS, F_STOPS, F_TICKS, F_COUNT
};
static_assert(F_COUNT <= 14, "kPubFields trop petit");

ExtClock::ExtClock()
    : srate_(48000.0), head_(0), count_(0), spt_(0.0), have_spt_(false),
      t_fit_(0.0), t_seen_(0.0), seen_(false), pos_next_(0), pos_last_(0.0),
      has_pos_(false), running_(false), waiting_(false), pend_(false),
      t_pend_(0.0), t_pend_from_(0.0), n_pend_(0), dev_run_(0),
      jitter_(0.0), bpm_short_(0.0), bpm_err_(0.0), starts_(0), stops_(0),
      ticks_(0), snap_(), seq_(0) {
  for (int i = 0; i < kRing; ++i) ring_[i] = 0.0;
  for (int i = 0; i < kPubFields; ++i) pub_[i].store(0.0, std::memory_order_relaxed);
  // Une graine a 120 BPM : la position entre Start et la deuxieme impulsion
  // doit s'extrapoler avec QUELQUE CHOSE. Elle n'est jamais publiee comme un
  // tempo (have_spt_ reste faux).
  spt_ = srate_ * 60.0 / (120.0 * 24.0);
}

void ExtClock::set_srate(double sr) {
  if (!(sr > 1.0) || sr == srate_) return;
  // Les instants de la fenetre sont des echantillons a l'ANCIEN taux : les
  // melanger aux nouveaux donnerait une pente fausse. La pente, elle, se
  // convertit.
  spt_ *= sr / srate_;
  srate_ = sr;
  count_ = 0;
  dev_run_ = 0;
}

void ExtClock::reset() {
  head_ = 0;
  count_ = 0;
  seen_ = false;
  pos_next_ = 0;
  pos_last_ = 0.0;
  has_pos_ = false;
  running_ = false;
  waiting_ = false;
  pend_ = false;
  dev_run_ = 0;
  jitter_ = 0.0;
  bpm_short_ = 0.0;
  bpm_err_ = 0.0;
}

void ExtClock::push(double t) {
  ring_[head_] = t;
  head_ = (head_ + 1) % kRing;
  if (count_ < kRing) ++count_;
}

// Une impulsion entre dans la fenetre, avec `fill` impulsions perdues avant
// elle (depuis `from`) : leurs places a intervalles egaux, et la position qui
// avance d'autant. Sans ca, toute la suite tombe d'un 24e de noire.
void ExtClock::accept(double t, int64_t fill, double from) {
  if (fill > 0 && count_ > 0) {
    const double step = (t - from) / (double)(fill + 1);
    for (int64_t j = 1; j <= fill; ++j) push(from + step * (double)j);
    if (running_ && !waiting_) pos_next_ += fill;
  }
  push(t);
  if (running_) {
    pos_last_ = (double)pos_next_;
    ++pos_next_;
    waiting_ = false;
  }
  fit();
}

// Un trou qu'on n'a pas pu juger (le maitre s'est tu, ou un message de
// transport arrive) : l'impulsion compte, sans rien remplir.
void ExtClock::settle_pending() {
  if (!pend_) return;
  pend_ = false;
  accept(t_pend_, 0, 0.0);
}

void ExtClock::tick(double t) {
  ++ticks_;
  const double lo = srate_ * 60.0 / (kMaxBpm * 24.0);
  const double hi = srate_ * 60.0 / (kMinBpm * 24.0);
  // La meme impulsion vue deux fois (deux entrees ouvertes sur le meme
  // cable) : la seconde ne compte pas.
  if (seen_ && (t - t_seen_) < 0.3 * (have_spt_ ? spt_ : lo)) return;

  // LE TROU EN SUSPENS SE JUGE ICI : revenue a l'intervalle d'avant, c'etait
  // une perte. Le juger avant de mesurer l'ecart de celle-ci, sinon l'ecart
  // partirait d'une impulsion pas encore dans la fenetre.
  if (pend_) {
    pend_ = false;
    const double k2 = (t - t_pend_) / spt_;
    accept(t_pend_, (std::fabs(k2 - 1.0) < 0.35) ? n_pend_ : 0, t_pend_from_);
  }

  if (seen_) {
    const double dt = t - t_seen_;
    if (have_spt_) {
      const double k = dt / spt_;
      if (k > 4.5) {
        count_ = 0;              // le maitre s'etait arrete : nouvelle fenetre
        dev_run_ = 0;
      } else if (k > 1.5) {
        const double r = std::floor(k + 0.5);
        if (std::fabs(k - r) < 0.15) {
          pend_ = true;
          n_pend_ = (int64_t)r - 1;
          t_pend_ = t;
          t_pend_from_ = t_seen_;
          t_seen_ = t;
          return;
        }
      }
    } else if (dt > hi) {
      count_ = 0;
    }
  }
  t_seen_ = t;
  seen_ = true;
  accept(t, 0, 0.0);
}

void ExtClock::fit() {
  const int m = count_;
  if (m <= 0) return;
  const int newest = (head_ - 1 + kRing) % kRing;
  if (m == 1) {
    t_fit_ = ring_[newest];
    return;
  }
  const double lo = srate_ * 60.0 / (kMaxBpm * 24.0);
  const double hi = srate_ * 60.0 / (kMinBpm * 24.0);

  // LA DROITE LONGUE. x = rang de l'impulsion, y = son instant, relatif a la
  // plus ancienne (des doubles de quelques millions d'echantillons, pas des
  // milliards : la precision reste entiere). Les rangs etant consecutifs,
  // Sxx est une formule et sum((x - xbar) * y) suffit -- sum(x - xbar) = 0.
  const int oldest = (head_ - m + kRing) % kRing;
  const double t0 = ring_[oldest];
  const double xbar = 0.5 * (double)(m - 1);
  double sy = 0.0, sxy = 0.0;
  for (int k = 0; k < m; ++k) {
    const double y = ring_[(oldest + k) % kRing] - t0;
    sy += y;
    sxy += ((double)k - xbar) * y;
  }
  const double sxx = (double)m * ((double)m * (double)m - 1.0) / 12.0;
  const double b = sxy / sxx;
  if (!(b >= lo && b <= hi)) {
    // Une pente impossible : la fenetre melange deux regimes. On repart de
    // la derniere impulsion.
    count_ = 1;
    dev_run_ = 0;
    t_fit_ = ring_[newest];
    return;
  }
  const double ybar = sy / (double)m;
  spt_ = b;
  have_spt_ = true;
  t_fit_ = t0 + ybar + b * ((double)(m - 1) - xbar);

  if (m < 8) {
    bpm_err_ = 60.0 * srate_ / (24.0 * b);   // inconnue : aussi grande que lui
    return;
  }

  // Le bruit autour de la droite, et ce qu'il dit de la pente.
  const double a = ybar - b * xbar;
  double ss = 0.0;
  for (int k = 0; k < m; ++k) {
    const double r = (ring_[(oldest + k) % kRing] - t0) - (a + b * (double)k);
    ss += r * r;
  }
  jitter_ = std::sqrt(ss / (double)(m - 2));
  const double bpm = 60.0 * srate_ / (24.0 * b);
  bpm_err_ = bpm * (jitter_ / std::sqrt(sxx)) / b;

  // LA DROITE COURTE, la derniere noire : ce qui voit un changement.
  const int ns = (m < (int)kShort) ? m : (int)kShort;
  const int so = (head_ - ns + kRing) % kRing;
  const double ts0 = ring_[so];
  const double xs = 0.5 * (double)(ns - 1);
  double sxys = 0.0;
  for (int k = 0; k < ns; ++k) {
    sxys += ((double)k - xs) * (ring_[(so + k) % kRing] - ts0);
  }
  const double sxxs = (double)ns * ((double)ns * (double)ns - 1.0) / 12.0;
  const double bs = sxys / sxxs;
  if (bs > 0.0) bpm_short_ = 60.0 * srate_ / (24.0 * bs);

  // Un changement de tempo : la courte s'ecarte de la longue plus que son
  // propre bruit ne l'explique, douze impulsions de suite. La fenetre longue
  // repart alors de ces DOUZE-LA, pas de la noire entiere : la noire garde une
  // dizaine d'impulsions de l'ancien tempo, et ce coude dans la droite tenait
  // la mesure a cote pendant des noires (127,88 trois noires apres 124 -> 128).
  if (m >= 2 * (int)kShort && bs > 0.0) {
    const double rel = std::fabs(bs - b) / b;
    double thr = 4.0 * jitter_ / (b * std::sqrt(sxxs));
    if (thr < 0.002) thr = 0.002;
    if (rel > thr) {
      if (++dev_run_ >= 12) {
        dev_run_ = 0;
        count_ = 12;
        fit();
      }
    } else {
      dev_run_ = 0;
    }
  }
}

void ExtClock::start(double t) {
  (void)t;
  settle_pending();
  ++starts_;
  running_ = true;
  has_pos_ = true;
  waiting_ = true;
  pos_next_ = 0;
  pos_last_ = 0.0;
}

void ExtClock::cont(double t) {
  (void)t;
  settle_pending();
  ++starts_;
  running_ = true;
  // Repart d'ou il s'etait arrete, ou du SPP recu entre-temps : c'est deja
  // pos_next_. Fige jusqu'a la premiere impulsion.
  waiting_ = true;
}

void ExtClock::stop(double t) {
  (void)t;
  settle_pending();
  ++stops_;
  running_ = false;
  waiting_ = false;
}

void ExtClock::spp(int sixteenths) {
  settle_pending();
  if (sixteenths < 0) sixteenths = 0;
  pos_next_ = (int64_t)sixteenths * 6;
  has_pos_ = true;
  // La norme ne l'envoie qu'a l'arret ; un maitre qui saute EN jouant
  // l'envoie quand meme. La position reste figee jusqu'a l'impulsion qui suit.
  if (running_) waiting_ = true;
}

void ExtClock::end_block(double now) {
  // Un trou en suspens que plus rien ne vient juger.
  if (pend_ && (now - t_pend_) > 2.5 * spt_) settle_pending();
  double gone = 0.5 * srate_;
  if (gone < 4.0 * spt_) gone = 4.0 * spt_;
  const bool present = seen_ && (now - t_seen_) < gone;
  if (!present) dev_run_ = 0;
  int flags = 0;
  if (present) flags |= kPresent;
  if (running_) flags |= kRunning;
  if (has_pos_) flags |= kHasPos;
  if (count_ >= 2 * (int)kShort) flags |= kSettled;
  if (waiting_) flags |= kWaiting;

  snap_.bpm       = have_spt_ ? 60.0 * srate_ / (24.0 * spt_) : 0.0;
  snap_.bpm_err   = bpm_err_;
  snap_.bpm_short = bpm_short_;
  snap_.jitter_ms = jitter_ * 1000.0 / srate_;
  snap_.beats     = (double)count_ / 24.0;
  snap_.t_last    = t_fit_;
  snap_.pos_last  = pos_last_;
  snap_.pos_next  = (double)pos_next_;
  snap_.spt       = spt_;
  snap_.flags     = flags;
  snap_.starts    = starts_;
  snap_.stops     = stops_;
  snap_.ticks     = ticks_;

  const uint32_t s = seq_.load(std::memory_order_relaxed);
  seq_.store(s + 1, std::memory_order_relaxed);
  std::atomic_thread_fence(std::memory_order_release);
  pub_[F_BPM].store(snap_.bpm, std::memory_order_relaxed);
  pub_[F_BPM_ERR].store(snap_.bpm_err, std::memory_order_relaxed);
  pub_[F_BPM_SHORT].store(snap_.bpm_short, std::memory_order_relaxed);
  pub_[F_JITTER].store(snap_.jitter_ms, std::memory_order_relaxed);
  pub_[F_BEATS].store(snap_.beats, std::memory_order_relaxed);
  pub_[F_T_LAST].store(snap_.t_last, std::memory_order_relaxed);
  pub_[F_POS_LAST].store(snap_.pos_last, std::memory_order_relaxed);
  pub_[F_POS_NEXT].store(snap_.pos_next, std::memory_order_relaxed);
  pub_[F_SPT].store(snap_.spt, std::memory_order_relaxed);
  pub_[F_FLAGS].store((double)snap_.flags, std::memory_order_relaxed);
  pub_[F_STARTS].store((double)snap_.starts, std::memory_order_relaxed);
  pub_[F_STOPS].store((double)snap_.stops, std::memory_order_relaxed);
  pub_[F_TICKS].store((double)snap_.ticks, std::memory_order_relaxed);
  seq_.store(s + 2, std::memory_order_release);
}

bool ExtClock::read(ExtClockSnap& o) const {
  for (int tries = 0; tries < 16; ++tries) {
    const uint32_t s0 = seq_.load(std::memory_order_acquire);
    if (s0 == 0) return false;          // jamais publie
    if (s0 & 1u) continue;              // ecriture en cours
    o.bpm       = pub_[F_BPM].load(std::memory_order_relaxed);
    o.bpm_err   = pub_[F_BPM_ERR].load(std::memory_order_relaxed);
    o.bpm_short = pub_[F_BPM_SHORT].load(std::memory_order_relaxed);
    o.jitter_ms = pub_[F_JITTER].load(std::memory_order_relaxed);
    o.beats     = pub_[F_BEATS].load(std::memory_order_relaxed);
    o.t_last    = pub_[F_T_LAST].load(std::memory_order_relaxed);
    o.pos_last  = pub_[F_POS_LAST].load(std::memory_order_relaxed);
    o.pos_next  = pub_[F_POS_NEXT].load(std::memory_order_relaxed);
    o.spt       = pub_[F_SPT].load(std::memory_order_relaxed);
    o.flags     = (int)pub_[F_FLAGS].load(std::memory_order_relaxed);
    o.starts    = (uint32_t)pub_[F_STARTS].load(std::memory_order_relaxed);
    o.stops     = (uint32_t)pub_[F_STOPS].load(std::memory_order_relaxed);
    o.ticks     = (uint32_t)pub_[F_TICKS].load(std::memory_order_relaxed);
    std::atomic_thread_fence(std::memory_order_acquire);
    if (seq_.load(std::memory_order_relaxed) == s0) return true;
  }
  return false;
}

double ExtClock::qn_at(const ExtClockSnap& s, double t) {
  if (!(s.flags & kHasPos)) return 0.0;
  if (!(s.flags & kRunning) || (s.flags & kWaiting) || !(s.spt > 0.0)) {
    return s.pos_next / 24.0;
  }
  // Sur la droite, a partir de la derniere impulsion AJUSTEE. La borne n'est
  // la que pour un maitre disparu sans Stop : deux noires de part et d'autre,
  // assez large pour n'importe quel bloc a n'importe quel tempo.
  double f = (t - s.t_last) / s.spt;
  if (f < -48.0) f = -48.0;
  if (f > 48.0) f = 48.0;
  return (s.pos_last + f) / 24.0;
}

}  // namespace cp

