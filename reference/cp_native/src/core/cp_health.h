// cp_health.h -- LA SANTE DU MOTEUR : CE QUI A ETE REFUSE, BORNE OU PERDU
#pragma once

#include <atomic>
#include <cstdint>

namespace cp {
namespace health {

// ============================================================================
// ⚠⚠ CE QUI EST REFUSE, BORNE OU PERDU SE COMPTE, ET SE COMPTE PAR NOM
// (le socle, 2026-10-25)
// ============================================================================
// Une borne qui jette en silence se cherche du cote de l'appelant pendant des
// semaines : `kMaxPoly` l'a montre (la moitie du temps d'une forme d'onde,
// deux fois trop large, sous une regle juste). Chaque refus et chaque bornage
// a donc un NOM ici, et un compteur que celui qui le constate incremente --
// fil principal, compositeur ou fil audio : un increment relache n'alloue rien
// et ne prend aucun verrou.
//
// Un compteur qui bouge n'est pas une panne : la valeur fautive a ete rendue
// inoffensive. C'est un DEFAUT DE L'APPELANT qui a ete absorbe, et les harnais
// le font echouer pour qu'il soit corrige a sa source.
enum Id {
  kArgNonFinite = 0,   // un argument NaN venu de Lua, remplace par 0 a la frontiere
  kGeomNonFinite,      // un ordre de dessin dont une valeur n'etait pas finie : il ne dessine rien
  kGeomClamped,        // une coordonnee, un rayon ou une taille d'image hors bornes, bornee
  kBindNonFinite,      // une liaison dont le calcul a rendu NaN (k ou c demesures)
  kAudioRefused,       // une commande audio dont une valeur n'etait pas finie, refusee
  kAudioClamped,       // une commande audio bornee : gain, vitesse, position, duree
  kCount
};

inline const char* name(int id) {
  static const char* const k[kCount] = {
    "arg_non_finite", "geom_non_finite", "geom_clamped",
    "bind_non_finite", "audio_refused", "audio_clamped",
  };
  return (id >= 0 && id < kCount) ? k[id] : "?";
}

// Le tableau est statique : initialise a zero au chargement, sans garde
// d'initialisation (le constructeur d'un atomique est trivial).
inline std::atomic<uint32_t>* table() {
  static std::atomic<uint32_t> t[kCount];
  return t;
}

inline void bump(int id) {
  if (id >= 0 && id < kCount) table()[id].fetch_add(1, std::memory_order_relaxed);
}

inline uint32_t get(int id) {
  return (id >= 0 && id < kCount) ? table()[id].load(std::memory_order_relaxed) : 0u;
}

inline uint32_t total() {
  uint32_t n = 0;
  for (int i = 0; i < kCount; ++i) n += get(i);
  return n;
}

// Pour les harnais : chaque test part de zero.
inline void reset() {
  for (int i = 0; i < kCount; ++i) table()[i].store(0, std::memory_order_relaxed);
}

} // namespace health
} // namespace cp

