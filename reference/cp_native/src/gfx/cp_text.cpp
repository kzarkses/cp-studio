#include "cp_text.h"

#include <cstring>

namespace cp {
namespace gfx {

namespace {

// ---------------------------------------------------------------------------
// LE REPLI : ce qu'on affiche quand le point de code n'est pas cuit
// ---------------------------------------------------------------------------
// ⚠ UN REPLI LISIBLE, PAS UNE CASE VIDE. Une police n'a pas forcement « ★ », et
// l'atlas a une taille finie. Rendre un rectangle vide ferait chercher un
// defaut de rendu ; rendre « * » dit la meme chose en moins joli et n'envoie
// personne enqueter. C'est le meme principe que le point d'interrogation
// d'origine, en plus utile.
int ascii_fallback(unsigned int cp) {
  switch (cp) {
    // Les tirets et signes moins.
    case 0x2010: case 0x2011: case 0x2012: case 0x2013: case 0x2014:
    case 0x2015: case 0x2212:                               return '-';
    // Les guillemets.
    case 0x2018: case 0x2019: case 0x2032: case 0x00B4:     return '\'';
    case 0x201C: case 0x201D: case 0x2033: case 0x00AB:
    case 0x00BB:                                            return '"';
    // Les points.
    case 0x2026: case 0x00B7: case 0x2022: case 0x00B0:     return '.';
    // Les fleches et les triangles, par direction.
    case 0x2192: case 0x25B6: case 0x25B8: case 0x203A:
    case 0x27A1:                                            return '>';
    case 0x2190: case 0x25C0: case 0x25C2: case 0x2039:     return '<';
    case 0x2191: case 0x25B2: case 0x25B4:                  return '^';
    case 0x2193: case 0x25BC: case 0x25BE:                  return 'v';
    // Les operateurs.
    case 0x00D7: return 'x';
    case 0x00F7: return '/';
    case 0x00B1: return '+';
    case 0x2264: case 0x226A: return '<';
    case 0x2265: case 0x226B: return '>';
    case 0x2260: return '#';
    case 0x2261: case 0x2248: return '=';
    case 0x221E: return '8';
    case 0x2208: return 'e';
    // Les pastilles et les etoiles.
    case 0x25CF: case 0x25CB: case 0x25A0: case 0x25A1:
    case 0x25A3: case 0x2605: case 0x2606: case 0x25AA:     return '*';
    case 0x2713: case 0x2714: return 'v';
    case 0x266A: case 0x266B: return 'd';
    case 0xFF0B: return '+';
    default: break;
  }
  // Les lettres latines accentuees se ramenent a leur base. Ce n'est pas de la
  // typographie, c'est de la lisibilite : « superieure » vaut mieux que
  // « sup?rieure ».
  if (cp >= 0x00C0u && cp <= 0x00FFu) {
    // ⚠ PAS `kBase[64]` : en C++ une chaine de 64 caracteres n'y tient pas,
    // son zero terminal deborde. On laisse le compilateur dimensionner.
    static const char kBase[] =
        "AAAAAAACEEEEIIIIDNOOOOOxOUUUUYPs"
        "aaaaaaaceeeeiiiidnooooo/ouuuuypy";
    return kBase[cp - 0x00C0u];
  }
  return '?';
}

// Dichotomie sur la table triee des glyphes hors ASCII.
const Glyph* ext_find(const Atlas& a, unsigned int cp) {
  int lo = 0, hi = a.ext_n - 1;
  while (lo <= hi) {
    const int mid = (lo + hi) >> 1;
    const unsigned int m = a.ext_cp[mid];
    if (m == cp) return &a.ext[mid];
    if (m < cp) lo = mid + 1; else hi = mid - 1;
  }
  return nullptr;
}

// Le glyphe a poser pour un point de code, quel qu'il soit. Ne rend JAMAIS nul :
// la chaine des replis se termine sur le point d'interrogation, qui est de
// l'ASCII imprimable et donc toujours cuit.
const Glyph& glyph_of(const Atlas& a, unsigned int cp) {
  if (cp >= (unsigned)kGlyphFirst && cp <= (unsigned)kGlyphLast)
    return a.g[cp - kGlyphFirst];
  if (a.ext_n > 0) {
    const Glyph* e = ext_find(a, cp);
    if (e) return *e;
  }
  int f = ascii_fallback(cp);
  if (f < kGlyphFirst || f > kGlyphLast) f = '?';
  return a.g[f - kGlyphFirst];
}

} // namespace

// ⚠ UNE SEQUENCE INVALIDE CONSOMME UN SEUL OCTET. Avancer de la longueur
// ANNONCEE par un premier octet corrompu peut sauter par-dessus le zero
// terminal et partir lire la memoire suivante. On n'avance de la sequence
// entiere qu'apres avoir VU tous ses octets de continuation — et le zero
// terminal n'en est pas un, donc il arrete toujours le decodage.
unsigned int utf8_next(const unsigned char** pp) {
  const unsigned char* p = *pp;
  const unsigned int c = *p;
  if (c < 0x80u) { *pp = p + 1; return c; }

  int n;
  unsigned int cp;
  if      ((c & 0xE0u) == 0xC0u) { n = 1; cp = c & 0x1Fu; }
  else if ((c & 0xF0u) == 0xE0u) { n = 2; cp = c & 0x0Fu; }
  else if ((c & 0xF8u) == 0xF0u) { n = 3; cp = c & 0x07u; }
  else                           { *pp = p + 1; return 0xFFFDu; }

  for (int i = 1; i <= n; ++i) {
    if ((p[i] & 0xC0u) != 0x80u) { *pp = p + 1; return 0xFFFDu; }
    cp = (cp << 6) | (unsigned int)(p[i] & 0x3Fu);
  }
  *pp = p + n + 1;
  return cp;
}

void atlas_reset(Atlas& a) {
  std::memset(a.g, 0, sizeof(a.g));
  std::memset(a.ext, 0, sizeof(a.ext));
  std::memset(a.ext_cp, 0, sizeof(a.ext_cp));
  a.ext_n = 0;
  a.used = 0;
  a.ascent = 0;
  a.line_h = 0;
  a.ready = false;
}

namespace {

// Le corps commun aux deux `atlas_add*` : copier l'encre dans l'arene et
// remplir la fiche. Rend false si l'arene est pleine.
bool fill_glyph(Atlas& a, Glyph& g, const unsigned char* cov, int w, int h,
                int ox, int oy, int adv) {
  g.w = 0; g.h = 0; g.ox = (short)ox; g.oy = (short)oy;
  g.adv = (short)adv; g.off = 0;
  if (!cov || w <= 0 || h <= 0) return true;   // un espace : pas d'encre
  const int n = w * h;
  if (a.used + n > kAtlasBytes) return false;
  std::memcpy(a.px + a.used, cov, (size_t)n);
  g.w = (short)w; g.h = (short)h; g.off = a.used;
  a.used += n;
  return true;
}

} // namespace

bool atlas_add(Atlas& a, int ch, const unsigned char* cov, int w, int h,
               int ox, int oy, int adv) {
  if (ch < kGlyphFirst || ch > kGlyphLast) return false;
  return fill_glyph(a, a.g[ch - kGlyphFirst], cov, w, h, ox, oy, adv);
}

bool atlas_add_ext(Atlas& a, unsigned int cp, const unsigned char* cov,
                   int w, int h, int ox, int oy, int adv) {
  if (a.ext_n >= kExtMax) return false;
  // ⚠ L'ORDRE CROISSANT EST LA CONDITION DE LA DICHOTOMIE. Un appel qui le
  // rompt est refuse : accepter et chercher ensuite rendrait un glyphe faux
  // pour certains caracteres seulement, ce qui est introuvable.
  if (a.ext_n > 0 && cp <= a.ext_cp[a.ext_n - 1]) return false;
  if (!fill_glyph(a, a.ext[a.ext_n], cov, w, h, ox, oy, adv)) return false;
  a.ext_cp[a.ext_n] = cp;
  ++a.ext_n;
  return true;
}

void atlas_draw(const Surface& s, const Atlas& a, int x, int y,
                const char* str, unsigned int col, unsigned int alpha) {
  if (!a.ready || !str || !s.px || alpha == 0) return;
  int pen = x;
  const int base = y + a.ascent;

  const unsigned char* p = (const unsigned char*)str;
  while (*p) {
    const Glyph& g = glyph_of(a, utf8_next(&p));
    if (g.w > 0 && g.h > 0) {
      const int gx = pen + g.ox, gy = base - g.oy;
      // ⚠ TOUTE LA BOITE D'ENCRE EST HORS DE LA DECOUPE : on saute. C'est un
      // raccourci, pas une politique — `blend_px` decoupe deja au pixel, et le
      // resultat est le meme. Sans lui, une liste de mille lignes dont trois
      // sont visibles parcourrait mille fois les glyphes pour ne rien ecrire.
      if (gy + g.h > s.cy && gy < s.cy + s.ch &&
          gx + g.w > s.cx && gx < s.cx + s.cw) {
        const unsigned char* src = a.px + g.off;
        for (int j = 0; j < g.h; ++j) {
          const unsigned char* row = src + (size_t)j * g.w;
          for (int i = 0; i < g.w; ++i) {
            unsigned int c = row[i];
            if (!c) continue;
            if (alpha < 255) { c = (c * alpha + 127u) / 255u; if (!c) continue; }
            blend_px(s, gx + i, gy + j, col, c);
          }
        }
      }
    }
    pen += g.adv;
  }
}

int atlas_measure(const Atlas& a, const char* str) {
  if (!a.ready || !str) return 0;
  int w = 0;
  const unsigned char* p = (const unsigned char*)str;
  while (*p) w += glyph_of(a, utf8_next(&p)).adv;
  return w;
}

} // namespace gfx
} // namespace cp

