// L'ancienne clock externe (reference/cp_native/src/core/cp_extclock.cpp)
// exposee en fonctions C pour cp_oracle (tests/extclock.rs).
#include "../../../reference/cp_native/src/core/cp_extclock.h"

#define ORA extern "C" __declspec(dllexport)

using cp::ExtClock;

static ExtClock* C(void* p) { return static_cast<ExtClock*>(p); }

ORA void* ec_new() { return new ExtClock(); }
ORA void ec_free(void* p) { delete C(p); }
ORA void ec_set_srate(void* p, double sr) { C(p)->set_srate(sr); }
ORA void ec_reset(void* p) { C(p)->reset(); }
ORA void ec_tick(void* p, double t) { C(p)->tick(t); }
ORA void ec_start(void* p, double t) { C(p)->start(t); }
ORA void ec_cont(void* p, double t) { C(p)->cont(t); }
ORA void ec_stop(void* p, double t) { C(p)->stop(t); }
ORA void ec_spp(void* p, int s) { C(p)->spp(s); }
ORA void ec_end_block(void* p, double now) { C(p)->end_block(now); }

// L'etat vivant, champ par champ (13 doubles).
ORA void ec_live(void* p, double* o) {
  const cp::ExtClockSnap& s = C(p)->live();
  o[0] = s.bpm;
  o[1] = s.bpm_err;
  o[2] = s.bpm_short;
  o[3] = s.jitter_ms;
  o[4] = s.beats;
  o[5] = s.t_last;
  o[6] = s.pos_last;
  o[7] = s.pos_next;
  o[8] = s.spt;
  o[9] = (double)s.flags;
  o[10] = (double)s.starts;
  o[11] = (double)s.stops;
  o[12] = (double)s.ticks;
}

ORA double ec_qn_at(void* p, double t) { return ExtClock::qn_at(C(p)->live(), t); }
