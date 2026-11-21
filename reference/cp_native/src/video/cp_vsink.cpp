// cp_vsink — voir cp_vsink.h pour le pourquoi.
#include "cp_vsink.h"
#include "cp_vscale.h"

#include <atomic>
#include <cstring>
#include <new>
#include <shared_mutex>

namespace cp {
namespace video {

namespace {

struct Sink {
  const void* owner = nullptr;   // nullptr : case libre
  int  img_id = -1;
  int  source = kSrcNone;
  int  w = 0, h = 0;
  unsigned* buf[3] = { nullptr, nullptr, nullptr };
  // Ce que porte chaque tampon : le rectangle de l'image et sa taille source.
  // Range PAR TAMPON, pour que l'avant dise toujours la verite sur lui-meme.
  int  rect[3][4] = {};
  int  srcwh[3][2] = {};
  // Les indices ne changent que sous `lk`.
  int  back = 0, mid = 1, frnt = 2;
  std::atomic<bool> fresh{false};
  std::atomic<long> seq{0};
  std::atomic_flag lk = ATOMIC_FLAG_INIT;
};

Sink g_sinks[kMaxSinks];
// En LECTURE : publier, composer. En ECRITURE : lier, delier.
std::shared_mutex g_struct;

inline void spin_lock(std::atomic_flag& f) {
  while (f.test_and_set(std::memory_order_acquire)) { /* quelques instructions */ }
}
inline void spin_unlock(std::atomic_flag& f) { f.clear(std::memory_order_release); }

void release(Sink& s) {
  for (int i = 0; i < 3; ++i) { delete[] s.buf[i]; s.buf[i] = nullptr; }
  s.owner = nullptr; s.img_id = -1; s.source = kSrcNone; s.w = 0; s.h = 0;
  s.back = 0; s.mid = 1; s.frnt = 2;
  s.fresh.store(false); s.seq.store(0);
  std::memset(s.rect, 0, sizeof s.rect);
  std::memset(s.srcwh, 0, sizeof s.srcwh);
}

Sink* find(const void* owner, int img_id) {
  for (int i = 0; i < kMaxSinks; ++i)
    if (g_sinks[i].owner == owner && g_sinks[i].img_id == img_id) return &g_sinks[i];
  return nullptr;
}

} // namespace

int bind(const void* owner, int img_id, int source, int w, int h) {
  if (!owner || img_id < 0 || source <= kSrcNone || source >= kSrcCount) return -1;
  if (w < 2 || h < 2 || w > kMaxSinkW || h > kMaxSinkH) return -1;
  std::unique_lock<std::shared_mutex> g(g_struct);
  Sink* s = find(owner, img_id);
  if (s && s->w == w && s->h == h) { s->source = source; return 0; }
  if (s) release(*s);
  if (!s) {
    for (int i = 0; i < kMaxSinks && !s; ++i) if (!g_sinks[i].owner) s = &g_sinks[i];
    if (!s) return -2;
  }
  const size_t n = (size_t)w * (size_t)h;
  for (int i = 0; i < 3; ++i) {
    s->buf[i] = new (std::nothrow) unsigned[n];
    if (!s->buf[i]) { release(*s); return -3; }
    // Transparent tant que rien n'est arrive : la case montre le fond de la
    // fenetre, pas un noir qui ferait croire a une image.
    std::memset(s->buf[i], 0, n * sizeof(unsigned));
  }
  s->owner = owner; s->img_id = img_id; s->source = source; s->w = w; s->h = h;
  return 0;
}

void unbind(const void* owner, int img_id) {
  std::unique_lock<std::shared_mutex> g(g_struct);
  if (Sink* s = find(owner, img_id)) release(*s);
}

void unbind_all(const void* owner) {
  if (!owner) return;
  std::unique_lock<std::shared_mutex> g(g_struct);
  for (int i = 0; i < kMaxSinks; ++i) if (g_sinks[i].owner == owner) release(g_sinks[i]);
}

bool info(const void* owner, int img_id, Info* out) {
  std::shared_lock<std::shared_mutex> g(g_struct);
  Sink* s = find(owner, img_id);
  if (!s || !out) return false;
  spin_lock(s->lk);
  const int f = s->frnt;
  out->w = s->w; out->h = s->h;
  out->rx = s->rect[f][0]; out->ry = s->rect[f][1];
  out->rw = s->rect[f][2]; out->rh = s->rect[f][3];
  out->src_w = s->srcwh[f][0]; out->src_h = s->srcwh[f][1];
  spin_unlock(s->lk);
  out->seq = s->seq.load();
  out->source = s->source;
  return true;
}

int readers(int source) {
  std::shared_lock<std::shared_mutex> g(g_struct);
  int n = 0;
  for (int i = 0; i < kMaxSinks; ++i)
    if (g_sinks[i].owner && g_sinks[i].source == source) ++n;
  return n;
}

namespace {
// Le geste du producteur sur UNE case : ecrire l'arriere, l'echanger avec le
// milieu, lever `fresh`. `back` n'appartient qu'au producteur : on le lit sans
// verrou. Deux producteurs d'une meme case ne coexistent pas (une camera, un
// robinet, un chargeur d'images fixes : chacun ses cases).
void put(Sink& s, const unsigned* src, int sw, int sh, int stride, bool swap_rb,
         bool opaque, int orig_w, int orig_h) {
  const int b = s.back;
  int rx, ry, rw, rh;
  letterbox(src, sw, sh, stride, s.buf[b], s.w, s.h, swap_rb, opaque, &rx, &ry, &rw, &rh);
  s.rect[b][0] = rx; s.rect[b][1] = ry; s.rect[b][2] = rw; s.rect[b][3] = rh;
  s.srcwh[b][0] = orig_w; s.srcwh[b][1] = orig_h;
  spin_lock(s.lk);
  s.back = s.mid; s.mid = b;
  s.fresh.store(true, std::memory_order_release);
  spin_unlock(s.lk);
  s.seq.fetch_add(1, std::memory_order_relaxed);
}
} // namespace

int publish(int source, const unsigned* src, int sw, int sh, int stride,
            bool swap_rb, bool opaque) {
  if (!src || sw <= 0 || sh <= 0) return 0;
  std::shared_lock<std::shared_mutex> g(g_struct);
  int served = 0;
  for (int i = 0; i < kMaxSinks; ++i) {
    Sink& s = g_sinks[i];
    if (!s.owner || s.source != source) continue;
    put(s, src, sw, sh, stride, swap_rb, opaque, sw, sh);
    ++served;
  }
  return served;
}

bool publish_to(const void* owner, int img_id, const unsigned* src, int sw, int sh,
                int stride, bool swap_rb, bool opaque, int orig_w, int orig_h) {
  if (!src || sw <= 0 || sh <= 0) return false;
  std::shared_lock<std::shared_mutex> g(g_struct);
  Sink* s = find(owner, img_id);
  if (!s) return false;
  put(*s, src, sw, sh, stride, swap_rb, opaque, orig_w, orig_h);
  return true;
}

void begin_read() { g_struct.lock_shared(); }
void end_read()   { g_struct.unlock_shared(); }

int acquire(const void* owner, int* ids, int max) {
  int n = 0;
  for (int i = 0; i < kMaxSinks; ++i) {
    Sink& s = g_sinks[i];
    if (!s.owner || s.owner != owner) continue;
    if (!s.fresh.load(std::memory_order_acquire)) continue;
    spin_lock(s.lk);
    if (s.fresh.load(std::memory_order_relaxed)) {
      const int t = s.frnt; s.frnt = s.mid; s.mid = t;
      s.fresh.store(false, std::memory_order_relaxed);
      if (ids && n < max) ids[n] = s.img_id;
      ++n;
    }
    spin_unlock(s.lk);
  }
  return n;
}

const unsigned* front(const void* owner, int img_id, int* w, int* h) {
  for (int i = 0; i < kMaxSinks; ++i) {
    Sink& s = g_sinks[i];
    if (s.owner != owner || s.img_id != img_id || !s.owner) continue;
    if (w) *w = s.w;
    if (h) *h = s.h;
    return s.buf[s.frnt];   // `frnt` n'est change que par `acquire`, du meme fil
  }
  return nullptr;
}

bool pending(const void* owner) {
  for (int i = 0; i < kMaxSinks; ++i) {
    const Sink& s = g_sinks[i];
    if (s.owner == owner && s.owner && s.fresh.load(std::memory_order_acquire)) return true;
  }
  return false;
}

} // namespace video
} // namespace cp

