// cp_sane.h -- LA FRONTIERE : CE QUE LUA ENVOIE DEVIENT UN NOMBRE SUR
#pragma once

#include <cmath>

#include "../core/cp_health.h"

namespace cp {
namespace sane {

// ============================================================================
// ⚠⚠ RIEN NE TRAVERSE LA FRONTIERE SANS CONTROLE (le socle, lot 2, 2026-10-25)
// ============================================================================
// Il n'y avait qu'UN controle de valeur finie dans toute la couche qui recoit
// Lua. Un `0/0` en Lua -- une jauge dont le minimum egale le maximum -- donnait
// une hauteur NaN ; `(int)NaN` vaut INT_MIN sous MSVC, la hauteur debordait en
// un nombre positif enorme, la decoupe debordait a son tour, et le
// rasteriseur ecrivait des lignes au-dela de l'image : REAPER tombait. Dans
// REAPER, `gfx` absorbe ces valeurs ; la DLL doit faire au moins aussi bien.
//
// Trois couches, et chacune a sa raison :
//   1. `arg` -- TOUT argument venu de Lua (`argd`, cp_main.cpp) : plus aucun
//      NaN n'entre dans la DLL, pour les deux moteurs a la fois ;
//   2. les PROPRIETAIRES bornent leurs invariants : `cp::list::push` la
//      geometrie, `Engine::apply` le son. Ce sont eux que les harnais
//      compilent, donc eux que les tests tiennent ;
//   3. toute conversion d'un `double` en entier du rejeu passe par `px` ou
//      `trunc` : aucune ne peut deborder, quoi qu'on lui donne.

// La plus grande coordonnee qu'un ordre garde. Bien au-dela de tout ecran, et
// assez petite pour que seize translations bornees (kMaxScope) additionnees a
// une largeur bornee tiennent dans un `int` : 2^24 x 18 < 2^31.
constexpr double kCoordMax = 16777216.0;

// Le plus grand cote d'une image hors ecran : celui de `gfx.setimgdim`.
constexpr int kImgDimMax = 8192;

// Un infini venu de Lua est un « sans borne » voulu (`math.huge` comme borne
// d'une liaison) : il devient un tres grand nombre FINI, sans etre compte.
constexpr double kArgMax = 1e300;

// Ce qu'une liaison peut rendre : assez pour un nombre formate, assez peu pour
// qu'aucune somme ne redevienne infinie.
constexpr double kValMax = 1e12;

// Une couleur rangee en double (les paires de teinte).
constexpr double kColorMax = 4294967295.0;

inline bool finite(double v) { return v == v && v - v == 0.0; }

// LA FRONTIERE. NaN -> 0, compte ; +-inf -> +-kArgMax, non compte.
inline double arg(double v) {
  if (v != v) { health::bump(health::kArgNonFinite); return 0.0; }
  if (v > kArgMax) return kArgMax;
  if (v < -kArgMax) return -kArgMax;
  return v;
}

// Borne a +-lim ; `clamped` dit si la valeur a change. NaN -> 0.
inline double clamp(double v, double lim, bool* clamped) {
  if (v != v) { if (clamped) *clamped = true; return 0.0; }
  if (v > lim)  { if (clamped) *clamped = true; return lim; }
  if (v < -lim) { if (clamped) *clamped = true; return -lim; }
  return v;
}

inline double coord(double v) { return clamp(v, kCoordMax, nullptr); }

// LES CONVERSIONS DU REJEU. `px` arrondit vers le bas (un bord), `trunc` vers
// zero (ce que faisait `(int)`, pour ne deplacer aucun pixel existant). Les
// deux bornent a +-2^30 AVANT de convertir : `(int)` d'un double hors de la
// plage d'un int est un comportement indefini, et rend INT_MIN sous MSVC.
constexpr double kPxMax = 1073741824.0;
inline int px(double v) {
  if (!(v == v)) return 0;
  if (v >= kPxMax) return (int)kPxMax;
  if (v <= -kPxMax) return -(int)kPxMax;
  return (int)std::floor(v);
}
inline int trunc(double v) {
  if (!(v == v)) return 0;
  if (v >= kPxMax) return (int)kPxMax;
  if (v <= -kPxMax) return -(int)kPxMax;
  return (int)v;
}

} // namespace sane
} // namespace cp

