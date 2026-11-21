#include "cp_blit.h"

#include <cmath>

namespace cp {
namespace gfx {

namespace {

inline int imax(int a, int b) { return a > b ? a : b; }
inline int imin(int a, int b) { return a < b ? a : b; }

inline unsigned int clamp255(double v) {
  if (v <= 0.0) return 0u;
  if (v >= 255.0) return 255u;
  return (unsigned int)(v + 0.5);
}

// La couverture d'un pixel dont le centre est a la distance signee `d` de la
// frontiere, positive a l'interieur. Meme rampe d'un pixel que cp_raster2 : la
// forme est supposee localement droite a l'echelle du pixel.
inline unsigned int cov_of(double d) {
  const double c = d + 0.5;
  if (c <= 0.0) return 0u;
  if (c >= 1.0) return 255u;
  return (unsigned int)(c * 255.0 + 0.5);
}

// Compose en tenant compte du mode. `cov` est la couverture geometrique,
// `sa` l'alpha de la source.
inline void put(const Surface& s, int x, int y, unsigned int src,
                unsigned int cov, int mode) {
  if (x < s.cx || y < s.cy || x >= s.cx + s.cw || y >= s.cy + s.ch) return;
  unsigned int* p = s.px + (size_t)y * s.stride + x;
  if (mode & kModeAdd) {
    // ⚠ L'ADDITIF NE SATURE PAS TOUT SEUL. Sans la borne, deux halos qui se
    // recouvrent repassent par zero — un point NOIR au plus lumineux de
    // l'image, ce qu'on prendrait pour un trou dans le dessin.
    const unsigned int d = *p;
    unsigned int r = ((d >> 16) & 0xFFu) + (((src >> 16) & 0xFFu) * cov + 127u) / 255u;
    unsigned int g = ((d >>  8) & 0xFFu) + (((src >>  8) & 0xFFu) * cov + 127u) / 255u;
    unsigned int b = ( d        & 0xFFu) + (( src        & 0xFFu) * cov + 127u) / 255u;
    if (r > 255u) r = 255u;
    if (g > 255u) g = 255u;
    if (b > 255u) b = 255u;
    *p = (d & 0xFF000000u) | (r << 16) | (g << 8) | b;
    return;
  }
  *p = blend_over(*p, src, cov);
}

} // namespace

// ---------------------------------------------------------------------------
// LE BLIT
// ---------------------------------------------------------------------------
void blit(const Surface& dst, const Img& src,
          double sx, double sy, double sw, double sh,
          double dx, double dy, double dw, double dh,
          unsigned int alpha, int mode) {
  if (!dst.px || !src.px || alpha == 0) return;
  if (sw <= 0.0 || sh <= 0.0 || dw <= 0.0 || dh <= 0.0) return;

  const double kx = sw / dw, ky = sh / dh;
  // Un pixel de destination recouvre kx x ky pixels de source. Au-dela de 1,
  // il en recouvre plusieurs : c'est une REDUCTION, et il faut les moyenner
  // tous. En dessous, on echantillonne.
  const bool box = !(mode & kModeNoFilter) && (kx > 1.0 || ky > 1.0);

  // ⚠⚠ L'ECHANTILLON SE PREND AU CENTRE DU PIXEL (2026-10-06). On lisait la
  // source au coin gauche, tronque vers zero : a une position fractionnaire, la
  // premiere colonne sortait DEUX fois et toute l'image glissait d'un pixel.
  // Au centre, une copie 1:1 a une position quelconque est une copie EXACTE a
  // la position arrondie -- donc une icone reste nette quelle que soit la
  // portee qui la translate. La reduction couvre toujours les pixels touches.
  int x0, y0, x1, y1;
  if (box) {
    x0 = (int)std::floor(dx); y0 = (int)std::floor(dy);
    x1 = (int)std::ceil(dx + dw); y1 = (int)std::ceil(dy + dh);
  } else {
    x0 = (int)std::ceil(dx - 0.5); y0 = (int)std::ceil(dy - 0.5);
    x1 = (int)std::ceil(dx + dw - 0.5); y1 = (int)std::ceil(dy + dh - 0.5);
  }
  x0 = imax(x0, dst.cx); y0 = imax(y0, dst.cy);
  x1 = imin(x1, dst.cx + dst.cw); y1 = imin(y1, dst.cy + dst.ch);
  if (x0 >= x1 || y0 >= y1) return;

  // La fenetre de source, bornee a l'image : on ne lit jamais hors d'elle.
  const double su0 = sx < 0.0 ? 0.0 : sx;
  const double sv0 = sy < 0.0 ? 0.0 : sy;
  const double su1 = (sx + sw > (double)src.w) ? (double)src.w : sx + sw;
  const double sv1 = (sy + sh > (double)src.h) ? (double)src.h : sy + sh;
  if (su0 >= su1 || sv0 >= sv1) return;

  for (int y = y0; y < y1; ++y) {
    for (int x = x0; x < x1; ++x) {
      unsigned int r, g, b, a;

      if (box) {
        // ⚠⚠ CHAQUE PIXEL DE SOURCE PESE CE QU'IL RECOUVRE (2026-10-06). On
        // moyennait a poids EGAUX tous les pixels touches, meme a peine : une
        // icone REAPER de 30 px posee dans 22 melangeait chaque pixel a ses
        // deux voisins entiers, et sortait floue au point d'en etre grossiere.
        // Pondere par la surface, un pixel recouvert a 7 % pese 7 %. Une
        // reduction exacte par deux (la cuisson des icones) ne change pas :
        // tous les poids valent 1.
        double u0 = sx + ((double)x - dx) * kx, u1 = u0 + kx;
        double v0 = sy + ((double)y - dy) * ky, v1 = v0 + ky;
        if (u0 < su0) u0 = su0; if (u1 > su1) u1 = su1;
        if (v0 < sv0) v0 = sv0; if (v1 > sv1) v1 = sv1;
        if (u0 >= u1 || v0 >= v1) continue;
        const int iu0 = (int)std::floor(u0), iu1 = (int)std::ceil(u1);
        const int iv0 = (int)std::floor(v0), iv1 = (int)std::ceil(v1);
        double sr = 0.0, sg = 0.0, sb = 0.0, sa = 0.0, wt = 0.0;
        for (int v = iv0; v < iv1; ++v) {
          double wv = 1.0;
          if (v < v0) wv -= v0 - v;
          if (v + 1 > v1) wv -= (v + 1) - v1;
          if (wv <= 0.0) continue;
          const unsigned int* row = src.px + (size_t)v * src.w;
          for (int u = iu0; u < iu1; ++u) {
            double wu = 1.0;
            if (u < u0) wu -= u0 - u;
            if (u + 1 > u1) wu -= (u + 1) - u1;
            if (wu <= 0.0) continue;
            const double k = wu * wv;
            const unsigned int p = row[u];
            sa += k * (double)((p >> 24) & 0xFFu);
            sr += k * (double)((p >> 16) & 0xFFu);
            sg += k * (double)((p >>  8) & 0xFFu);
            sb += k * (double)( p        & 0xFFu);
            wt += k;
          }
        }
        if (wt <= 0.0) continue;
        r = clamp255(sr / wt); g = clamp255(sg / wt);
        b = clamp255(sb / wt); a = clamp255(sa / wt);
      } else {
        int u = (int)std::floor(sx + ((double)x + 0.5 - dx) * kx);
        int v = (int)std::floor(sy + ((double)y + 0.5 - dy) * ky);
        if (u < (int)su0) u = (int)su0; if (u >= (int)std::ceil(su1)) u = (int)std::ceil(su1) - 1;
        if (v < (int)sv0) v = (int)sv0; if (v >= (int)std::ceil(sv1)) v = (int)std::ceil(sv1) - 1;
        if (u < 0) u = 0; if (u >= src.w) u = src.w - 1;
        if (v < 0) v = 0; if (v >= src.h) v = src.h - 1;
        const unsigned int p = src.px[(size_t)v * src.w + u];
        a = (p >> 24) & 0xFFu; r = (p >> 16) & 0xFFu;
        g = (p >>  8) & 0xFFu; b =  p        & 0xFFu;
      }

      // ⚠⚠ UNE IMAGE HORS ECRAN CONTIENT DE L'ALPHA PREMULTIPLIE. Ce n'est pas
      // un choix, c'est une consequence : `blend_over` compose sur un tampon
      // efface a alpha 0, donc un pixel couvert a 50 % en ressort a
      // (couleur/2, alpha 128). Le blit doit donc composer SANS remultiplier —
      // sinon les bords anticreneles de chaque icone sont assombris deux fois.
      //
      // ⚠ ET LE MODE 2 EST UNE COPIE, PAS UN MELANGE. `gfx.mode = 2` veut dire
      // « pas de melange par l'alpha de la source » : la source ECRASE la
      // destination, SON ALPHA COMPRIS. L'ancienne version forcait la couleur
      // source a alpha 255 et la couverture a 255 : la reduction d'une icone
      // (deux passes en mode 2, `Icons.bake_icon`) ressortait donc en CARRE
      // OPAQUE de la couleur du glyphe. Le defaut etait invisible tant que
      // `kMaxImg` valait 128 et qu'aucun tampon n'existait ; il se serait
      // reveille exactement au moment ou on l'a corrige.
      if (mode & kModeNoSrcA) {
        unsigned int cov = 255u;
        if (alpha < 255) cov = alpha;
        if (!cov) continue;
        put(dst, x, y, (a << 24) | (r << 16) | (g << 8) | b, cov, mode);
        continue;
      }
      unsigned int cov = a;
      if (alpha < 255) cov = (cov * alpha + 127u) / 255u;
      if (!cov) continue;
      // On rend la couleur droite avant de la confier a `blend_over`, qui la
      // repondere par la couverture : les deux operations s'annulent, et le
      // resultat est le « source over » exact d'une source premultipliee.
      if (a > 0 && a < 255) {
        r = (r * 255u + (a >> 1)) / a; if (r > 255u) r = 255u;
        g = (g * 255u + (a >> 1)) / a; if (g > 255u) g = 255u;
        b = (b * 255u + (a >> 1)) / a; if (b > 255u) b = 255u;
      }
      put(dst, x, y, (255u << 24) | (r << 16) | (g << 8) | b, cov, mode);
    }
  }
}

// ---------------------------------------------------------------------------
void mul_add_rect(const Surface& s, int x, int y, int w, int h,
                  double mr, double mg, double mb, double ma,
                  double ar, double ag, double ab, double aa) {
  if (!s.px || w <= 0 || h <= 0) return;
  int x0 = imax(x, s.cx), y0 = imax(y, s.cy);
  int x1 = imin(x + w, s.cx + s.cw), y1 = imin(y + h, s.cy + s.ch);
  for (int j = y0; j < y1; ++j) {
    unsigned int* row = s.px + (size_t)j * s.stride;
    for (int i = x0; i < x1; ++i) {
      const unsigned int p = row[i];
      const double a = (double)((p >> 24) & 0xFFu);
      const double r = (double)((p >> 16) & 0xFFu);
      const double g = (double)((p >>  8) & 0xFFu);
      const double b = (double)( p        & 0xFFu);
      row[i] = (clamp255(a * ma + aa * 255.0) << 24)
             | (clamp255(r * mr + ar * 255.0) << 16)
             | (clamp255(g * mg + ag * 255.0) <<  8)
             |  clamp255(b * mb + ab * 255.0);
    }
  }
}

// ---------------------------------------------------------------------------
void fill_triangle(const Surface& s, double x1, double y1, double x2, double y2,
                   double x3, double y3, unsigned int col, unsigned int alpha) {
  if (!s.px || alpha == 0) return;

  // ⚠ LE SIGNE SE DEDUIT DE LA FORME, PAS D'UNE CONVENTION. L'aire signee dit
  // dans quel sens les sommets tournent ; on la normalise a l'endroit, et les
  // vingt appels existants marchent quel que soit le sens ou ils les passent.
  const double area = (x2 - x1) * (y3 - y1) - (x3 - x1) * (y2 - y1);
  if (area == 0.0) return;
  const double sgn = (area > 0.0) ? 1.0 : -1.0;

  const double ex[3] = { x2 - x1, x3 - x2, x1 - x3 };
  const double ey[3] = { y2 - y1, y3 - y2, y1 - y3 };
  const double px[3] = { x1, x2, x3 };
  const double py[3] = { y1, y2, y3 };
  double len[3];
  for (int i = 0; i < 3; ++i) {
    len[i] = std::sqrt(ex[i] * ex[i] + ey[i] * ey[i]);
    if (len[i] <= 0.0) return;
  }

  double lx = x1, hx = x1, ly = y1, hy = y1;
  const double vx[3] = { x1, x2, x3 }, vy[3] = { y1, y2, y3 };
  for (int i = 1; i < 3; ++i) {
    if (vx[i] < lx) lx = vx[i]; if (vx[i] > hx) hx = vx[i];
    if (vy[i] < ly) ly = vy[i]; if (vy[i] > hy) hy = vy[i];
  }
  int ix0 = imax((int)std::floor(lx - 1.0), s.cx);
  int iy0 = imax((int)std::floor(ly - 1.0), s.cy);
  int ix1 = imin((int)std::ceil(hx + 1.0), s.cx + s.cw - 1);
  int iy1 = imin((int)std::ceil(hy + 1.0), s.cy + s.ch - 1);

  for (int y = iy0; y <= iy1; ++y) {
    unsigned int* row = s.px + (size_t)y * s.stride;
    const double fy = (double)y + 0.5;
    for (int x = ix0; x <= ix1; ++x) {
      const double fx = (double)x + 0.5;
      // La distance a la frontiere du triangle est la PLUS PETITE des trois
      // distances aux cotes : c'est le minimum qui decide, comme pour l'anneau.
      double dmin = 1e30;
      for (int i = 0; i < 3; ++i) {
        const double d = sgn * (ex[i] * (fy - py[i]) - ey[i] * (fx - px[i])) / len[i];
        if (d < dmin) dmin = d;
      }
      unsigned int c = cov_of(dmin);
      if (!c) continue;
      if (alpha < 255) { c = (c * alpha + 127u) / 255u; if (!c) continue; }
      row[x] = blend_over(row[x], col, c);
    }
  }
}

// ---------------------------------------------------------------------------
void gradient_rect(const Surface& s, int x, int y, int w, int h,
                   double r, double g, double b, double a,
                   double drdx, double dgdx, double dbdx, double dadx,
                   double drdy, double dgdy, double dbdy, double dady) {
  if (!s.px || w <= 0 || h <= 0) return;
  const int x0 = imax(x, s.cx), y0 = imax(y, s.cy);
  const int x1 = imin(x + w, s.cx + s.cw), y1 = imin(y + h, s.cy + s.ch);
  for (int j = y0; j < y1; ++j) {
    unsigned int* row = s.px + (size_t)j * s.stride;
    const double fy = (double)(j - y);
    for (int i = x0; i < x1; ++i) {
      const double fx = (double)(i - x);
      const double cr = (r + drdx * fx + drdy * fy) * 255.0;
      const double cg = (g + dgdx * fx + dgdy * fy) * 255.0;
      const double cb = (b + dbdx * fx + dbdy * fy) * 255.0;
      const double ca = (a + dadx * fx + dady * fy) * 255.0;
      const unsigned int cov = clamp255(ca);
      if (!cov) continue;
      const unsigned int col = (255u << 24) | (clamp255(cr) << 16)
                             | (clamp255(cg) << 8) | clamp255(cb);
      row[i] = blend_over(row[i], col, cov);
    }
  }
}

// ---------------------------------------------------------------------------
void radial_gradient(const Surface& s, double cx, double cy, double radius,
                     unsigned int col, unsigned int alpha, double falloff) {
  if (!s.px || alpha == 0 || radius <= 0.0) return;
  if (falloff <= 0.0) falloff = 1.0;
  int x0 = imax((int)std::floor(cx - radius), s.cx);
  int y0 = imax((int)std::floor(cy - radius), s.cy);
  int x1 = imin((int)std::ceil(cx + radius), s.cx + s.cw - 1);
  int y1 = imin((int)std::ceil(cy + radius), s.cy + s.ch - 1);
  const double inv = 1.0 / radius;
  for (int y = y0; y <= y1; ++y) {
    unsigned int* row = s.px + (size_t)y * s.stride;
    const double dy = (double)y + 0.5 - cy;
    for (int x = x0; x <= x1; ++x) {
      const double dx = (double)x + 0.5 - cx;
      const double t = std::sqrt(dx * dx + dy * dy) * inv;
      if (t >= 1.0) continue;
      double k = 1.0 - t;
      if (falloff != 1.0) k = std::pow(k, falloff);
      unsigned int c = (unsigned int)(k * (double)alpha + 0.5);
      if (!c) continue;
      if (c > 255u) c = 255u;
      row[x] = blend_over(row[x], col, c);
    }
  }
}

// ---------------------------------------------------------------------------
void glow_round_rect(const Surface& s, double x, double y, double w, double h,
                     double radius, double spread,
                     unsigned int col, unsigned int alpha) {
  if (!s.px || alpha == 0 || w <= 0.0 || h <= 0.0 || spread <= 0.0) return;
  double r = radius;
  if (r > w * 0.5) r = w * 0.5;
  if (r > h * 0.5) r = h * 0.5;
  if (r < 0.0) r = 0.0;

  const double ix = x + r, iy = y + r;
  const double iw = w - 2.0 * r, ih = h - 2.0 * r;

  int x0 = imax((int)std::floor(x - spread - 1.0), s.cx);
  int y0 = imax((int)std::floor(y - spread - 1.0), s.cy);
  int x1 = imin((int)std::ceil(x + w + spread + 1.0), s.cx + s.cw - 1);
  int y1 = imin((int)std::ceil(y + h + spread + 1.0), s.cy + s.ch - 1);

  for (int py = y0; py <= y1; ++py) {
    unsigned int* row = s.px + (size_t)py * s.stride;
    const double fy = (double)py + 0.5;
    for (int px = x0; px <= x1; ++px) {
      const double fx = (double)px + 0.5;
      // Distance au rectangle arrondi : distance au rectangle interieur, moins
      // le rayon. Positive dehors.
      double qx = fx - ix, qy = fy - iy;
      if (qx < 0.0) qx = -qx; else if (qx > iw) qx -= iw; else qx = 0.0;
      if (qy < 0.0) qy = -qy; else if (qy > ih) qy -= ih; else qy = 0.0;
      const double d = std::sqrt(qx * qx + qy * qy) - r;
      if (d <= 0.0) continue;            // l'interieur n'est pas du halo
      if (d >= spread) continue;
      const double k = 1.0 - d / spread;
      unsigned int c = (unsigned int)(k * k * (double)alpha + 0.5);
      if (!c) continue;
      if (c > 255u) c = 255u;
      row[px] = blend_over(row[px], col, c);
    }
  }
}

} // namespace gfx
} // namespace cp

