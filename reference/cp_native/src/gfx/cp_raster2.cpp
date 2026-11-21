// cp_raster2 — les primitives qu'un portage exige, et que la premiere passe
// n'avait pas : cercle, anneau, trait epais, polyligne.
//
// ============================================================================
// DEUX METHODES DE COUVERTURE, ET LE CHOIX N'EST PAS UNE PREFERENCE
// ============================================================================
// Le rectangle arrondi calcule sa couverture EXACTEMENT, par integration, parce
// que ca lui coute une table de 4 900 octets cuite une fois pour la vie du
// processus. Un cercle de rayon entier reutilise la MEME table : un quart de
// disque, c'est exactement ce qu'elle contient.
//
// L'anneau d'un knob et le trait epais, eux, n'ont pas de table possible : leur
// forme depend d'angles et de directions continus. On y calcule donc la
// couverture par DISTANCE SIGNEE — `couverture = borne(0, 1, d + 0.5)`, ou d est
// la distance du centre du pixel a la frontiere, comptee positive a l'interieur.
//
// ⚠ CE N'EST PAS EXACT, ET IL FAUT LE DIRE. La distance signee suppose la
// frontiere localement DROITE a l'echelle du pixel. Sur un cercle de rayon r,
// l'erreur d'aire par pixel de bord vaut environ 1/(8r) — donc 3 % a r = 4,
// 0,9 % a r = 14, invisible au-dela. C'est acceptable pour un anneau de knob et
// pour un trait ; ca ne l'aurait pas ete pour la forme qu'on regarde de pres,
// et c'est pour ca que le rectangle arrondi ne passe pas par la.
//
// ⚠ ET C'EST DEJA MIEUX QUE `gfx`, DE LOIN. Un trait epais y etait un quad SANS
// anticrenelage plus deux aretes de Wu — la route 3 d'ANALYSE_APIGraphique.md
// §10, la meilleure des dix SANS cuisson, et elle laissait encore un creux aux
// jointures colinéaires. Ici le trait a une couverture continue sur toute sa
// longueur, jointures comprises, parce qu'il n'y a plus de jointure du tout :
// une polyligne est UNE forme, pas N segments composes les uns sur les autres.
#include "cp_raster.h"
#include "cp_cover.h"

#include <cmath>

namespace cp {
namespace gfx {

namespace {

inline int imax(int a, int b) { return a > b ? a : b; }
inline int imin(int a, int b) { return a < b ? a : b; }

// La couverture d'un pixel dont le centre est a la distance signee `d` de la
// frontiere (positive dedans). Une rampe d'un pixel de large, centree sur la
// frontiere : c'est l'approximation « la frontiere est droite ici ».
inline unsigned int cov_of(double d) {
  const double c = d + 0.5;
  if (c <= 0.0) return 0u;
  if (c >= 1.0) return 255u;
  return (unsigned int)(c * 255.0 + 0.5);
}

// Distance d'un point au SEGMENT [a, b] — pas a la droite qui le porte. La
// difference est tout l'interet : elle donne des bouchons ronds gratuits, et
// deux segments consecutifs d'une polyligne se raccordent sans creux ni perle.
inline double dist_seg(double px, double py,
                       double ax, double ay, double bx, double by) {
  const double vx = bx - ax, vy = by - ay;
  const double wx = px - ax, wy = py - ay;
  const double vv = vx * vx + vy * vy;
  double t = (vv > 1e-12) ? ((wx * vx + wy * vy) / vv) : 0.0;
  if (t < 0.0) t = 0.0; else if (t > 1.0) t = 1.0;
  const double dx = wx - t * vx, dy = wy - t * vy;
  return std::sqrt(dx * dx + dy * dy);
}

} // namespace

// ---------------------------------------------------------------------------
// LE CERCLE
// ---------------------------------------------------------------------------
// Rayon entier dans la plage des tables : QUATRE quadrants lus, zero calcul.
// Un quart de disque est exactement ce que `corner_coverage` contient — c'est
// la meme geometrie, et s'en apercevoir evite d'ecrire une seconde table.
// ============================================================================
// ⚠⚠ LES BORNES DE BOUCLE SE RABATTENT SUR LA DECOUPE, PAS SEULEMENT LE PIXEL
// ============================================================================
// MESURE, PAS SUPPOSEE. Le banc a mesure qu'une retouche de 32x20 pixels coutait
// 54 % d'une frame pleine — au lieu du pour cent que sa surface laisse attendre.
// Le calcul d'endommagement ne servait donc presque a rien, et on ne l'aurait
// jamais su en lisant le code : il est JUSTE, il etait seulement inutile.
//
// La cause : ces primitives parcourent leur boite ENGLOBANTE et laissent
// `blend_px` rejeter chaque pixel hors decoupe. Le rejet est bon marche ; le
// `sqrt`, l'`atan2` et la distance au segment qui le PRECEDENT ne le sont pas,
// et ils etaient payes pour chaque pixel de l'anneau entier.
//
// `fill_rect` et `draw_polyline` rabattaient deja leurs bornes. Ces trois-la ne
// le faisaient pas, et ce sont exactement les trois que le banc voit dominer.
void fill_circle(const Surface& s, int cx, int cy, int radius,
                 unsigned int col, unsigned int alpha) {
  if (!s.px || alpha == 0 || radius < 1) return;
  // ⚠ LE REJET TOTAL D'ABORD. Les quatre miroirs de la table interdisent de
  // rabattre la boucle sans casser la symetrie ; en revanche un disque
  // entierement hors decoupe ne doit rien couter du tout — et c'est le cas de
  // toutes les pastilles d'une liste dont on ne repeint qu'une rangee.
  if (cx + radius < s.cx || cx - radius >= s.cx + s.cw ||
      cy + radius < s.cy || cy - radius >= s.cy + s.ch) return;
  const unsigned char* cov = (radius >= kCoverMinR && radius <= kCoverMaxR)
                             ? corner_coverage(radius) : nullptr;
  const int r = radius;
  if (cov) {
    for (int j = 0; j < r; ++j) {
      for (int i = 0; i < r; ++i) {
        const unsigned int c0 = cov[j * r + i];
        if (!c0) continue;
        const unsigned int c = (alpha < 255) ? (c0 * alpha + 127u) / 255u : c0;
        // Les quatre miroirs. La table est indexee depuis le coin EXTERIEUR,
        // donc (i, j) est a distance (r-i, r-j) du centre.
        blend_px(s, cx - r + i, cy - r + j, col, c);
        blend_px(s, cx + r - 1 - i, cy - r + j, col, c);
        blend_px(s, cx - r + i, cy + r - 1 - j, col, c);
        blend_px(s, cx + r - 1 - i, cy + r - 1 - j, col, c);
      }
    }
    return;
  }
  // Hors plage : distance signee. Un rayon de 40 pixels n'a pas de table, et
  // en cuire une couterait 1 600 octets pour une forme qu'on dessine une fois.
  const double R = (double)radius;
  const int qy0 = imax(cy - radius - 1, s.cy);
  const int qy1 = imin(cy + radius + 1, s.cy + s.ch - 1);
  const int qx0 = imax(cx - radius - 1, s.cx);
  const int qx1 = imin(cx + radius + 1, s.cx + s.cw - 1);
  for (int y = qy0; y <= qy1; ++y) {
    for (int x = qx0; x <= qx1; ++x) {
      const double dx = (double)x + 0.5 - (double)cx;
      const double dy = (double)y + 0.5 - (double)cy;
      unsigned int c = cov_of(R - std::sqrt(dx * dx + dy * dy));
      if (!c) continue;
      if (alpha < 255) c = (c * alpha + 127u) / 255u;
      blend_px(s, x, y, col, c);
    }
  }
}

// ---------------------------------------------------------------------------
// L'ANNEAU — l'arc epais d'un knob
// ---------------------------------------------------------------------------
// ⚠ LE REPERE EST CELUI DE `gfx.arc` : 0 = vers le haut, sens HORAIRE. Pas
// celui de cos/sin. Melanger les deux a fait pointer l'aiguille d'un knob a
// quatre-vingt-dix degres de son propre arc pendant des mois dans le toolkit ;
// on garde donc le repere que tout le code appelant connait deja.
//
// La couverture est le produit de trois rampes : dehors du rayon interieur,
// dedans du rayon exterieur, et dans le secteur angulaire. Le produit est ce
// qui rend les extremites propres sans cas particulier — a l'angle de depart,
// la rampe angulaire descend pendant que les deux radiales tiennent.
void fill_ring(const Surface& s, double cx, double cy, double ri, double ro,
               double a0, double a1, unsigned int col, unsigned int alpha) {
  if (!s.px || alpha == 0 || ro <= ri) return;
  if (a1 < a0) { const double t = a0; a0 = a1; a1 = t; }
  double sweep = a1 - a0;
  if (sweep <= 0.0) return;
  const double kTwoPi = 6.283185307179586;
  if (sweep > kTwoPi) sweep = kTwoPi;
  const bool full = (sweep >= kTwoPi - 1e-9);
  const double mid = a0 + sweep * 0.5;
  const double half = sweep * 0.5;

  int x0 = (int)std::floor(cx - ro - 1.0);
  int x1 = (int)std::ceil(cx + ro + 1.0);
  int y0 = (int)std::floor(cy - ro - 1.0);
  int y1 = (int)std::ceil(cy + ro + 1.0);
  x0 = imax(x0, s.cx); x1 = imin(x1, s.cx + s.cw - 1);
  y0 = imax(y0, s.cy); y1 = imin(y1, s.cy + s.ch - 1);
  if (x0 > x1 || y0 > y1) return;

  for (int y = y0; y <= y1; ++y) {
    for (int x = x0; x <= x1; ++x) {
      const double dx = (double)x + 0.5 - cx;
      const double dy = (double)y + 0.5 - cy;
      const double d = std::sqrt(dx * dx + dy * dy);
      unsigned int c = cov_of(ro - d);
      if (!c) continue;
      const unsigned int ci = cov_of(d - ri);
      if (!ci) continue;
      c = (c * ci + 127u) / 255u;

      if (!full) {
        // L'angle du pixel, dans le repere de gfx.arc.
        double ang = std::atan2(dx, -dy);
        // L'ecart au milieu du secteur, ramene dans [-pi, pi] : c'est ce qui
        // fait que le secteur ne se coupe pas en deux quand il croise le haut.
        double delta = ang - mid;
        while (delta > 3.141592653589793) delta -= kTwoPi;
        while (delta < -3.141592653589793) delta += kTwoPi;
        // La rampe angulaire se mesure en PIXELS le long de l'arc, pas en
        // radians : un demi-pixel a rayon 4 et a rayon 40, ce n'est pas le meme
        // angle, et une rampe en radians ferait des bouts flous sur les grands
        // knobs et durs sur les petits.
        const double edge_px = (half - std::fabs(delta)) * d;
        const unsigned int ca = cov_of(edge_px);
        if (!ca) continue;
        c = (c * ca + 127u) / 255u;
      }
      if (alpha < 255) c = (c * alpha + 127u) / 255u;
      blend_px(s, x, y, col, c);
    }
  }
}

// ---------------------------------------------------------------------------
// LE TRAIT EPAIS, ET LA POLYLIGNE
// ---------------------------------------------------------------------------
// Un seul segment : la distance au SEGMENT donne les bouchons ronds sans les
// demander.
void draw_thick_line(const Surface& s, double x0, double y0, double x1, double y1,
                     double width, unsigned int col, unsigned int alpha) {
  if (!s.px || alpha == 0 || width <= 0.0) return;
  const double hw = width * 0.5;
  int ix0 = (int)std::floor((x0 < x1 ? x0 : x1) - hw - 1.0);
  int ix1 = (int)std::ceil((x0 > x1 ? x0 : x1) + hw + 1.0);
  int iy0 = (int)std::floor((y0 < y1 ? y0 : y1) - hw - 1.0);
  int iy1 = (int)std::ceil((y0 > y1 ? y0 : y1) + hw + 1.0);
  ix0 = imax(ix0, s.cx); ix1 = imin(ix1, s.cx + s.cw - 1);
  iy0 = imax(iy0, s.cy); iy1 = imin(iy1, s.cy + s.ch - 1);
  if (ix0 > ix1 || iy0 > iy1) return;
  for (int y = iy0; y <= iy1; ++y) {
    for (int x = ix0; x <= ix1; ++x) {
      const double d = dist_seg((double)x + 0.5, (double)y + 0.5, x0, y0, x1, y1);
      unsigned int c = cov_of(hw - d);
      if (!c) continue;
      if (alpha < 255) c = (c * alpha + 127u) / 255u;
      blend_px(s, x, y, col, c);
    }
  }
}

// ⚠ UNE POLYLIGNE EST UNE FORME, PAS N SEGMENTS. C'est toute la difference avec
// `gfx`, et elle se voit. La-bas, chaque segment se composait sur le precedent :
// aux sommets partages le dessin passait deux fois — une perle claire sous alpha,
// un creux sombre aux jointures colinéaires (ANALYSE_APIGraphique.md §10 a
// mesure ce creux : une perle tous les 3,9 px sur une courbe lisse). Ici on
// prend, pour chaque pixel, la distance MINIMALE a l'ensemble des segments, et
// on ne compose qu'UNE fois. Il n'y a plus de jointure a rater.
// ============================================================================
// LA POLYLIGNE — UNE FORME, ET DESORMAIS EN TEMPS LINEAIRE
// ============================================================================
// ⚠ LA VERSION NAIVE TESTAIT CHAQUE PIXEL DE LA BOITE TOTALE CONTRE CHAQUE
// SEGMENT. Sur la courbe de la demo — 284 x 60 pixels, 96 segments — ca faisait
// 1,6 million de racines carrees et 5,0 ms des 5,6 ms de la frame entiere.
// Mesure par `build_gfx_bench.cmd`, pas devinee.
//
// ⚠ ET L'IMAGE NE CHANGE PAS D'UN PIXEL. `cov_of` DECROIT avec la distance,
// donc `cov(hw - min_i d_i)` vaut EXACTEMENT `max_i cov(hw - d_i)`. On peut
// donc n'ouvrir que la boite de CHAQUE segment et accumuler par maximum : un
// segment exclu est a plus de hw + 1 du pixel, sa couverture y vaut zero, et
// zero ne change pas un maximum. `gfx_test.cpp` le verrouille — deux segments
// colineaires doivent rendre le meme dessin qu'un seul, au pixel pres.
//
// ⚠ ET LE MAXIMUM N'EST PAS UNE ADDITION. C'est ce qui garde la promesse de la
// forme unique : additionner les couvertures rendrait les jointures plus
// sombres, ce qui est exactement la perle de `gfx` qu'on est venu supprimer.
//
// La ligne de travail est sur la PILE, pas en statique : le rejeu tournera un
// jour sur son propre fil, et un tampon statique y serait partage.
// ⚠ LE DECALAGE BOUGE LA QUESTION, PAS LES POINTS. On pourrait copier les
// points translates dans un tampon : ce serait une allocation par courbe et par
// composition, donc 120 par seconde et par courbe, dans le chemin de dessin ou
// la regle est ZERO. On decale donc le point d'INTERROGATION -- on demande la
// couverture en (x - ox, y - oy) au lieu de deplacer la forme. Le resultat est
// identique au pixel pres, et l'arene reste constante.
void draw_polyline(const Surface& s, const double* pts, int n,
                   double width, unsigned int col, unsigned int alpha,
                   double ox, double oy) {
  if (!s.px || alpha == 0 || !pts || n < 2 || width <= 0.0) return;
  const double hw = width * 0.5;
  const double pad = hw + 1.0;

  double lx = pts[0], ly = pts[1], hx = pts[0], hy = pts[1];
  for (int i = 1; i < n; ++i) {
    const double px = pts[i * 2], py = pts[i * 2 + 1];
    if (px < lx) lx = px; if (px > hx) hx = px;
    if (py < ly) ly = py; if (py > hy) hy = py;
  }

  // La boite est rabattue sur la DECOUPE des maintenant : les boucles ecrivent
  // ensuite dans la rangee sans retester, ce qui retire un test par pixel.
  int ix0 = (int)std::floor(lx + ox - pad), ix1 = (int)std::ceil(hx + ox + pad);
  int iy0 = (int)std::floor(ly + oy - pad), iy1 = (int)std::ceil(hy + oy + pad);
  ix0 = imax(ix0, s.cx); ix1 = imin(ix1, s.cx + s.cw - 1);
  iy0 = imax(iy0, s.cy); iy1 = imin(iy1, s.cy + s.ch - 1);
  if (ix0 > ix1 || iy0 > iy1) return;

  enum { kMaxRow = 4096 };
  unsigned char cov[kMaxRow];

  for (int bx0 = ix0; bx0 <= ix1; bx0 += kMaxRow) {
    const int bx1 = imin(ix1, bx0 + kMaxRow - 1);
    const int bw = bx1 - bx0 + 1;
    for (int y = iy0; y <= iy1; ++y) {
      const double py = (double)y + 0.5 - oy;
      bool any = false;
      for (int i = 0; i < bw; ++i) cov[i] = 0;

      for (int i = 0; i + 1 < n; ++i) {
        const double ax = pts[i * 2],     ay = pts[i * 2 + 1];
        const double bx = pts[i * 2 + 2], by = pts[i * 2 + 3];
        const double ymin = (ay < by ? ay : by) - pad;
        const double ymax = (ay > by ? ay : by) + pad;
        if (py < ymin || py > ymax) continue;
        const double xmin = (ax < bx ? ax : bx) - pad;
        const double xmax = (ax > bx ? ax : bx) + pad;
        int x0 = imax((int)std::floor(xmin + ox), bx0);
        int x1 = imin((int)std::ceil(xmax + ox),  bx1);
        for (int x = x0; x <= x1; ++x) {
          const unsigned int c = cov_of(hw - dist_seg((double)x + 0.5 - ox, py,
                                                      ax, ay, bx, by));
          if (!c) continue;
          unsigned char& slot = cov[x - bx0];
          if ((unsigned char)c > slot) { slot = (unsigned char)c; any = true; }
        }
      }

      if (!any) continue;
      unsigned int* row = s.px + (size_t)y * s.stride;
      for (int i = 0; i < bw; ++i) {
        unsigned int c = cov[i];
        if (!c) continue;
        if (alpha < 255) { c = (c * alpha + 127u) / 255u; if (!c) continue; }
        unsigned int* px = row + bx0 + i;
        *px = blend_over(*px, col, c);
      }
    }
  }
}

void draw_wave(const Surface& s, double x, double y, double w, double h,
               const double* pts, int n, unsigned int col, unsigned int alpha,
               double ox, double oy) {
  if (!s.px || alpha == 0 || !pts || n < 1 || w <= 0.0 || h <= 0.0) return;
  x += ox; y += oy;
  const double cy = y + h * 0.5;
  const double half = h * 0.5;

  int ix0 = (int)std::floor(x), ix1 = (int)std::ceil(x + w) - 1;
  ix0 = imax(ix0, s.cx); ix1 = imin(ix1, s.cx + s.cw - 1);
  if (ix0 > ix1) return;
  const int iy0 = imax((int)std::floor(y), s.cy);
  const int iy1 = imin((int)std::ceil(y + h) - 1, s.cy + s.ch - 1);
  if (iy0 > iy1) return;

  for (int px = ix0; px <= ix1; ++px) {
    // Quel echantillon ce pixel montre-t-il. On prend le MAXIMUM sur la plage
    // que le pixel recouvre : reduire une forme d onde en n echantillonnant
    // qu un point sur k fait CLIGNOTER les transitoires d une frame a l autre,
    // et c est le defaut le plus visible d un affichage de forme d onde.
    const double u0 = ((double)px - x) / w * (double)n;
    const double u1 = ((double)(px + 1) - x) / w * (double)n;
    int a = (int)u0, b = (int)std::ceil(u1);
    if (a < 0) a = 0;
    if (b > n) b = n;
    if (b <= a) b = a + 1;
    if (a >= n) break;
    double lo = pts[a * 2], hi = pts[a * 2 + 1];
    for (int i = a + 1; i < b && i < n; ++i) {
      if (pts[i * 2] < lo) lo = pts[i * 2];
      if (pts[i * 2 + 1] > hi) hi = pts[i * 2 + 1];
    }
    if (lo > hi) { const double t = lo; lo = hi; hi = t; }
    double y0 = cy - hi * half;
    double y1 = cy - lo * half;
    if (y1 - y0 < 1.0) {
      const double m = (y0 + y1) * 0.5;
      y0 = m - 0.5; y1 = m + 0.5;
    }
    int py0 = (int)std::floor(y0), py1 = (int)std::ceil(y1) - 1;
    if (py1 < py0) py1 = py0;
    py0 = imax(py0, iy0); py1 = imin(py1, iy1);
    for (int py = py0; py <= py1; ++py) {
      unsigned int* p = s.px + (size_t)py * s.stride + px;
      *p = blend_over(*p, col, alpha);
    }
  }
}

} // namespace gfx
} // namespace cp

