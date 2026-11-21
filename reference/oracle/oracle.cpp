// L'ancien moteur C++ (reference/cp_native), expose en fonctions C pour
// cp_oracle. Rien de l'Engine n'est modifie : chaque fonction est un appel
// direct, avec les commandes POD telles que Lua les envoyait.
#include "../cp_native/src/core/cp_engine.h"

#include <cstring>

#define ORA extern "C" __declspec(dllexport)

using cp::Engine;

static Engine* E(void* e) { return static_cast<Engine*>(e); }

ORA void* ora_new(double srate) {
  Engine* e = new Engine();
  e->init(srate);
  return e;
}

ORA void ora_free(void* e) { delete E(e); }

ORA void ora_set_srate(void* e, double srate) { E(e)->set_srate(srate); }

// Le chemin que suivait l'hote : reserver, allouer, remplir, publier.
ORA int ora_clip(void* e, const float* data, long long frames, int nch, double srate) {
  cp::Pool& p = E(e)->pool();
  const int s = p.acquire();
  if (s < 0) return -1;
  if (!p.alloc(s, frames, nch, srate)) {
    p.fail(s);
    return -1;
  }
  std::memcpy(p.writable(s), data, sizeof(float) * (size_t)frames * (size_t)nch);
  p.publish(s);
  return s;
}

ORA unsigned ora_alloc(void* e, int port) { return E(e)->voice_alloc(port); }
ORA void ora_release(void* e, unsigned h) { E(e)->voice_release(h); }
ORA int ora_voice_port(void* e, unsigned h) { return E(e)->voice_port(h); }

ORA int ora_post(void* e, int port, unsigned type, unsigned voice, long long at,
                 double a, double b, unsigned u0, unsigned u1) {
  cp::Cmd c;
  std::memset(&c, 0, sizeof(c));
  c.type = type;
  c.voice = voice;
  c.at = at;
  c.a = a;
  c.b = b;
  c.u0 = u0;
  c.u1 = u1;
  return E(e)->post(port, c) ? 1 : 0;
}

ORA long long ora_render(void* e, int port, float* out, int frames, int nch) {
  return E(e)->render_port(port, out, frames, nch);
}

ORA void ora_tick(void* e, int frames) { E(e)->tick(frames); }

ORA int ora_query(void* e, unsigned h, double* pos, int* state) {
  return E(e)->voice_query(h, pos, state) ? 1 : 0;
}

ORA long long ora_started_at(void* e, unsigned h) { return E(e)->voice_started_at(h); }
ORA void ora_panic(void* e) { E(e)->panic(); }
ORA void ora_port_reset(void* e, int port) { E(e)->port_reset(port); }
ORA long long ora_clock(void* e) { return E(e)->clock_now(); }
ORA int ora_owned(void* e) { return E(e)->owned_voices(); }
ORA int ora_port_list_size(void* e, int port) { return E(e)->port_list_size(port); }
