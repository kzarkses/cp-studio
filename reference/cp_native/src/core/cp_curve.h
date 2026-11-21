// CP_Native — la loi de courbe d'un segment, ecrite UNE fois.
//
// Deux enveloppes la suivent : celle qu'on dessine sur un son (la voix, mode
// ENV, cp_voice.cpp) et l'automation d'un clip (les lanes, cp_lanes.cpp). Leur
// miroir Lua est `Env.Eval` / `Env.SegValue` (CP_Engine/Env.lua), et l'editeur
// dessine avec lui : changer la loi ici sans changer Env.lua ferait dessiner une
// courbe et en entendre une autre.
//
//   valeur = v0 + (v1 - v0) * shp(x, k),   shp(x, k) = x / (x + k (1 - x))
//   k      = 8^(-c) si le segment monte, 8^(c) s'il descend ; c dans -1..1
//
// `k` se calcule a la VALIDATION d'un lot de points (fil principal), jamais par
// echantillon : un `pow` par point.
#pragma once

#include <cmath>

namespace cp {

static inline float curve_shp(float x, float k) {
  return (k == 1.0f) ? x : x / (x + k * (1.0f - x));
}

static inline float curve_factor(float c, bool rising) {
  if (c < -1.0f) c = -1.0f; else if (c > 1.0f) c = 1.0f;
  if (c == 0.0f) return 1.0f;
  return (float)(rising ? std::pow(8.0, -(double)c) : std::pow(8.0, (double)c));
}

} // namespace cp

