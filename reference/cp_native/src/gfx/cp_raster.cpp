#include "cp_raster.h"
#include "cp_cover.h"

namespace cp {
namespace gfx {

namespace {

// Une bande horizontale pleine, deja decoupee par l'appelant.
inline void span(const Surface& s, int x0, int x1, int y,
                 unsigned int col, unsigned int alpha) {
  unsigned int* row = s.px + (size_t)y * s.stride;
  if (alpha >= 255) {
    for (int x = x0; x < x1; ++x) row[x] = col;
  } else {
    for (int x = x0; x < x1; ++x) row[x] = blend_over(row[x], col, alpha);
  }
}

// Rectangle decoupe a la surface. Rend false si rien ne reste.
inline bool clip(const Surface& s, int& x, int& y, int& w, int& h) {
  if (w <= 0 || h <= 0) return false;
  // ⚠ CONTRE LE RECTANGLE DE DECOUPE, PAS CONTRE LA SURFACE. Les deux sont
  // egaux tant que personne n'a pousse de decoupe ; des qu'un panneau en pousse
  // une, tester contre la surface laisserait deborder.
  //
  // ⚠ LES SOMMES EN 64 BITS (le socle, lot 2). `y + h` debordait en negatif
  // pour une hauteur proche de 2^31 -- un NaN converti en INT_MIN y menait --,
  // la hauteur n'etait jamais bornee, et `span` ecrivait des lignes au-dela de
  // l'image. Les entrees sont bornees en amont (cp_sane.h) ; ce calcul ne doit
  // plus pouvoir mentir quoi qu'on lui donne.
  const long long rx = s.cx, ry = s.cy, rr = (long long)s.cx + s.cw,
                  rb = (long long)s.cy + s.ch;
  long long X = x, Y = y, Wd = w, Hd = h;
  if (X < rx) { Wd -= (rx - X); X = rx; }
  if (Y < ry) { Hd -= (ry - Y); Y = ry; }
  if (X + Wd > rr) Wd = rr - X;
  if (Y + Hd > rb) Hd = rb - Y;
  if (Wd <= 0 || Hd <= 0) return false;
  x = (int)X; y = (int)Y; w = (int)Wd; h = (int)Hd;
  return true;
}

// Un coin, lu dans la table. `flip_x` / `flip_y` choisissent lequel des quatre.
// Le pixel (0,0) de la table est le coin EXTERIEUR de la boite.
void corner(const Surface& s, int bx, int by, int r, const unsigned char* cov,
            bool flip_x, bool flip_y, unsigned int col, unsigned int alpha) {
  for (int j = 0; j < r; ++j) {
    const int py = by + (flip_y ? (r - 1 - j) : j);
    const unsigned char* line = cov + (size_t)j * r;
    for (int i = 0; i < r; ++i) {
      const int px = bx + (flip_x ? (r - 1 - i) : i);
      unsigned int c = line[i];
      if (!c) continue;
      if (alpha < 255) c = (c * alpha + 127u) / 255u;
      blend_px(s, px, py, col, c);
    }
  }
}

} // namespace

void fill_rect(const Surface& s, int x, int y, int w, int h,
               unsigned int col, unsigned int alpha) {
  if (!s.px || alpha == 0) return;
  if (!clip(s, x, y, w, h)) return;
  for (int j = 0; j < h; ++j) span(s, x, x + w, y + j, col, alpha);
}

void fill_round_rect(const Surface& s, int x, int y, int w, int h, int radius,
                     unsigned int col, unsigned int alpha) {
  if (!s.px || alpha == 0 || w <= 0 || h <= 0) return;

  // Le rayon est borne par la boite : voir l'en-tete. Sans ca les coins du haut
  // et du bas se recouvriraient au milieu d'une boite basse.
  int r = radius;
  if (r > w / 2) r = w / 2;
  if (r > h / 2) r = h / 2;

  const unsigned char* cov = (r >= kCoverMinR) ? corner_coverage(r) : nullptr;
  if (!cov) { fill_rect(s, x, y, w, h, col, alpha); return; }

  // Les trois bandes. Elles evitent exactement les quatre carres r x r des
  // coins, donc aucune ne recouvre une table.
  fill_rect(s, x + r,         y,     w - 2 * r, h,         col, alpha);
  fill_rect(s, x,             y + r, r,         h - 2 * r, col, alpha);
  fill_rect(s, x + w - r,     y + r, r,         h - 2 * r, col, alpha);

  corner(s, x,         y,         r, cov, false, false, col, alpha);  // haut gauche
  corner(s, x + w - r, y,         r, cov, true,  false, col, alpha);  // haut droit
  corner(s, x,         y + h - r, r, cov, false, true,  col, alpha);  // bas gauche
  corner(s, x + w - r, y + h - r, r, cov, true,  true,  col, alpha);  // bas droit
}

void stroke_round_rect(const Surface& s, int x, int y, int w, int h, int radius,
                       unsigned int col, unsigned int alpha) {
  if (!s.px || alpha == 0 || w <= 0 || h <= 0) return;

  int r = radius;
  if (r > w / 2) r = w / 2;
  if (r > h / 2) r = h / 2;

  const unsigned char* co = (r >= kCoverMinR) ? corner_coverage(r) : nullptr;
  const unsigned char* ci = (r - 1 >= kCoverMinR) ? corner_coverage(r - 1) : nullptr;
  if (!co) {
    // Contour droit : quatre bandes d'un pixel, a coordonnees ENTIERES.
    fill_rect(s, x,         y,         w, 1,     col, alpha);
    fill_rect(s, x,         y + h - 1, w, 1,     col, alpha);
    fill_rect(s, x,         y + 1,     1, h - 2, col, alpha);
    fill_rect(s, x + w - 1, y + 1,     1, h - 2, col, alpha);
    return;
  }

  // Les cotes droits, entre les coins.
  fill_rect(s, x + r,     y,         w - 2 * r, 1,         col, alpha);
  fill_rect(s, x + r,     y + h - 1, w - 2 * r, 1,         col, alpha);
  fill_rect(s, x,         y + r,     1,         h - 2 * r, col, alpha);
  fill_rect(s, x + w - 1, y + r,     1,         h - 2 * r, col, alpha);

  // L'anneau du coin : la couverture du rayon r moins celle du rayon r-1,
  // decalee d'un pixel vers l'interieur. La difference est exactement le trait,
  // et il est anticrenele des DEUX cotes sans jamais sortir de la forme.
  //
  // ⚠ LA TABLE INTERIEURE EST DECALEE, PAS REDIMENSIONNEE. Le coin interieur
  // occupe les pixels (1..r-1) de la boite : son origine est donc (bx+1, by+1)
  // vers l'interieur, ce que le sens de `flip` inverse deja.
  unsigned char ring[kCoverMaxR * kCoverMaxR];
  for (int j = 0; j < r; ++j) {
    for (int i = 0; i < r; ++i) {
      int outer = co[j * r + i];
      int inner = 0;
      if (ci && i >= 1 && j >= 1) inner = ci[(j - 1) * (r - 1) + (i - 1)];
      int v = outer - inner;
      if (v < 0) v = 0;
      ring[j * r + i] = (unsigned char)v;
    }
  }
  corner(s, x,         y,         r, ring, false, false, col, alpha);
  corner(s, x + w - r, y,         r, ring, true,  false, col, alpha);
  corner(s, x,         y + h - r, r, ring, false, true,  col, alpha);
  corner(s, x + w - r, y + h - r, r, ring, true,  true,  col, alpha);
}

} // namespace gfx
} // namespace cp

