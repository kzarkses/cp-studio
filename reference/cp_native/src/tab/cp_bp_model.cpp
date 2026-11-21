// cp_bp_model.cpp -- voir cp_bp_model.h.
//
// L'ordre des operations suit le graphe ONNX de nmp.onnx (CP_Native/tools/
// gen_bp_weights.py le lit) :
//   1. CQT2010v2 : l'octave du haut par 36 noyaux complexes de 256 points, pas
//      de 256 ; puis huit fois : demi-bande (firwin2, 256 points, pas de 2,
//      zeros autour), meme noyaux, pas divise par deux. Chaque octave est
//      reflechie de 128 points avant ses noyaux. 9 x 36 = 324 cases, on garde
//      les 309 du haut, multipliees par la racine de leur longueur ;
//   2. log : 10 log10(m^2 + 1e-10), ramene a 0..1 sur la FENETRE entiere
//      (min et max de toute la matrice), puis la normalisation par lot ;
//   3. empilement : huit copies decalees de -36, 0, 36, 57, 72, 84, 93, 101
//      cases (harmoniques 0.5, 1, 2 .. 7), les 264 premieres ;
//   4. six convolutions, deux branches (voir run_window).
//
// ⚠ AUCUNE ALLOCATION ICI. Tout vit dans `Work`, que l'appelant possede.
#include "cp_bp_model.h"
#include "cp_bp_weights.h"

#include <cmath>
#include <cstring>

namespace cp { namespace bp {

namespace {

const double kPi = 3.14159265358979323846;

// Le Q du CQT : un filtre par 1/36 d'octave.
double q_factor() { return 1.0 / (std::pow(2.0, 1.0 / kBinsPerOct) - 1.0); }

// scipy.signal.firwin2(256, [0, 0.5/1.001, 0.5*1.001, 1], [1, 1, 0, 0]) :
// echantillonnage en frequence sur 257 points, phase lineaire, irfft sur 512,
// les 256 premiers points fenetres par une Hamming SYMETRIQUE.
void make_lowpass(float* out)
{
    const int nfreqs = 257;
    const int nfft = 2 * (nfreqs - 1);
    const double band = 0.5, trans = 0.001;
    const double f1 = band / (1.0 + trans), f2 = band * (1.0 + trans);
    double re[257], im[257];
    for (int k = 0; k < nfreqs; ++k) {
        const double x = (double)k / (nfreqs - 1);
        double g;
        if (x <= f1) g = 1.0;
        else if (x >= f2) g = 0.0;
        else g = (f2 - x) / (f2 - f1);              // np.interp
        const double ph = -(kTaps - 1) / 2.0 * kPi * x;
        re[k] = g * std::cos(ph);
        im[k] = g * std::sin(ph);
    }
    for (int n = 0; n < kTaps; ++n) {
        // irfft : la partie imaginaire du continu et de Nyquist est ignoree.
        double s = re[0] + re[nfreqs - 1] * ((n & 1) ? -1.0 : 1.0);
        for (int k = 1; k < nfreqs - 1; ++k) {
            const double a = 2.0 * kPi * k * n / nfft;
            s += 2.0 * (re[k] * std::cos(a) - im[k] * std::sin(a));
        }
        s /= nfft;
        const double w = 0.54 - 0.46 * std::cos(2.0 * kPi * n / (kTaps - 1));
        out[n] = (float)(s * w);
    }
}

// Le miroir de numpy / torch : x[1], x[2] .. sans repeter le bord.
inline void reflect_pad(const float* x, int L, int pad, float* out)
{
    for (int i = 0; i < pad; ++i) out[i] = x[pad - i];
    std::memcpy(out + pad, x, sizeof(float) * (size_t)L);
    for (int j = 0; j < pad; ++j) out[pad + L + j] = x[L - 2 - j];
}

inline float sigmoid(float x) { return 1.0f / (1.0f + std::exp(-x)); }

// Convolution 2D, disposition [canal][temps][frequence], pas de 1 en temps.
// `ph` et `pwl` : les marges de debut (temps, frequence) du graphe ONNX ; la
// marge de fin decoule des tailles.
void conv(const float* in, int cin, int H, int Win,
          const float* W, const float* B, int cout, int kh, int kw,
          int sw, int ph, int pwl, float* out, int Wout)
{
    const int plane_in = H * Win, plane_out = H * Wout;
    for (int o = 0; o < cout; ++o) {
        float* op = out + (size_t)o * plane_out;
        const float b = B[o];
        for (int i = 0; i < plane_out; ++i) op[i] = b;
        for (int c = 0; c < cin; ++c) {
            const float* ip = in + (size_t)c * plane_in;
            for (int dh = 0; dh < kh; ++dh) {
                const int t0 = (ph - dh > 0) ? ph - dh : 0;
                const int t1 = (H - 1 + ph - dh < H - 1) ? H - 1 + ph - dh : H - 1;
                for (int dw = 0; dw < kw; ++dw) {
                    const float wv = W[((o * cin + c) * kh + dh) * kw + dw];
                    // colonnes de sortie dont la source tombe dans [0, Win)
                    const int off = dw - pwl;
                    int f0 = 0;
                    while (f0 < Wout && f0 * sw + off < 0) ++f0;
                    int f1 = Wout - 1;
                    while (f1 >= f0 && f1 * sw + off >= Win) --f1;
                    if (f1 < f0) continue;
                    for (int t = t0; t <= t1; ++t) {
                        const float* irow = ip + (size_t)(t + dh - ph) * Win + off;
                        float* orow = op + (size_t)t * Wout;
                        if (sw == 1) {
                            for (int f = f0; f <= f1; ++f) orow[f] += wv * irow[f];
                        } else {
                            for (int f = f0; f <= f1; ++f) orow[f] += wv * irow[f * sw];
                        }
                    }
                }
            }
        }
    }
}

inline void relu(float* x, size_t n)
{
    for (size_t i = 0; i < n; ++i) if (x[i] < 0.0f) x[i] = 0.0f;
}

} // namespace

void make_kernels(Kernels& k)
{
    const double Q = q_factor();
    const double fs = kSr;
    // CQT2010v2.build : la case du haut de l'octave du haut, puis son bas.
    const int remainder = kCqtBins % kBinsPerOct;                     // 21
    double fmin_t = 27.5 * std::pow(2.0, kOctaves - 1);
    const double fmax_t = fmin_t * std::pow(2.0, (remainder - 1) / (double)kBinsPerOct);
    fmin_t = fmax_t / std::pow(2.0, 1.0 - 1.0 / kBinsPerOct);
    // Pas de sous-echantillonnage precoce a ces reglages (early_downsample_count
    // vaut 0 : 0,85 x Nyquist est sous la coupure). fftLen = 256.
    std::memset(k.re, 0, sizeof(k.re));
    std::memset(k.im, 0, sizeof(k.im));
    for (int b = 0; b < kBinsPerOct; ++b) {
        const double freq = fmin_t * std::pow(2.0, b / (double)kBinsPerOct);
        const double l = std::ceil(Q * fs / freq);
        const int L = (int)l;
        const int start = (int)std::ceil(kTaps / 2.0 - l / 2.0) - (L % 2);
        const int m0 = (int)std::floor(-l / 2.0);                    // np.r_[-l//2 : l//2]
        // Hann PERIODIQUE (fftbins=True), puis normalisation L1 du complexe :
        // |sig[n]| = w[n] / l, donc la norme vaut somme(w) / l.
        double wsum = 0.0;
        for (int n = 0; n < L; ++n) wsum += 0.5 - 0.5 * std::cos(2.0 * kPi * n / L);
        const double norm = wsum / l;
        for (int n = 0; n < L; ++n) {
            const double w = 0.5 - 0.5 * std::cos(2.0 * kPi * n / L);
            const double a = (m0 + n) * 2.0 * kPi * freq / fs;
            const int at = start + n;
            if (at < 0 || at >= kTaps) continue;
            k.re[b][at] = (float)(w * std::cos(a) / l / norm);
            k.im[b][at] = (float)(w * std::sin(a) / l / norm);
        }
    }
    make_lowpass(k.lowpass);
    for (int b = 0; b < kCqtBins; ++b) {
        const double freq = 27.5 * std::pow(2.0, b / (double)kBinsPerOct);
        k.sqrtlen[b] = std::sqrt((float)std::ceil(Q * fs / freq));
    }
}

void run_window(const Kernels& k, Work& w, const float* in,
                float* note, float* onset, float* contour)
{
    // --- 1. CQT --------------------------------------------------------------
    int L = kWinSamples;
    float* cur = w.sig[0];
    float* nxt = w.sig[1];
    for (int i = 0; i < L; ++i) {
        const float v = in[i];
        cur[i] = (v == v && v < 1e30f && v > -1e30f) ? v : 0.0f;   // ni NaN ni infini
    }
    const int half = kTaps / 2;
    for (int d = 0; d < kOctaves; ++d) {
        const int hop = kHop >> d;
        reflect_pad(cur, L, half, w.pad);
        // Octave d (d = 0 : la plus haute) : place 8 - d dans la concatenation,
        // dont on retire les 15 premieres cases.
        const int base = (kOctaves - 1 - d) * kBinsPerOct - (kOctaves * kBinsPerOct - kCqtBins);
        for (int t = 0; t < kWinFrames; ++t) {
            const float* x = w.pad + (size_t)t * hop;
            for (int b = 0; b < kBinsPerOct; ++b) {
                const int bin = base + b;
                if (bin < 0) continue;
                const float* kr = k.re[b];
                const float* ki = k.im[b];
                float sr = 0.0f, si = 0.0f;
                for (int n = 0; n < kTaps; ++n) { sr += kr[n] * x[n]; si += ki[n] * x[n]; }
                const float s = k.sqrtlen[bin];
                sr *= s; si *= s;
                w.cqt[t][bin] = std::sqrt(sr * sr + si * si);
            }
        }
        if (d == kOctaves - 1) break;
        // Demi-bande : zeros de 127 points autour, filtre de 256, pas de 2.
        const int Ln = (L + 2 * 127 - kTaps) / 2 + 1;
        for (int i = 0; i < Ln; ++i) {
            const int j0 = 2 * i - 127;
            float s = 0.0f;
            int n0 = (j0 < 0) ? -j0 : 0;
            int n1 = (j0 + kTaps > L) ? L - j0 : kTaps;
            for (int n = n0; n < n1; ++n) s += k.lowpass[n] * cur[j0 + n];
            nxt[i] = s;
        }
        float* t = cur; cur = nxt; nxt = t;
        L = Ln;
    }

    // --- 2. log normalise sur la fenetre, puis normalisation par lot ----------
    float lo = 1e30f, hi = -1e30f;
    for (int t = 0; t < kWinFrames; ++t) {
        for (int b = 0; b < kCqtBins; ++b) {
            const float m = w.cqt[t][b];
            const float v = std::log(m * m + 1e-10f) * 0.4342944622039795f * 10.0f;
            w.cqt[t][b] = v;
            if (v < lo) lo = v;
        }
    }
    for (int t = 0; t < kWinFrames; ++t)
        for (int b = 0; b < kCqtBins; ++b) {
            const float v = w.cqt[t][b] - lo;
            w.cqt[t][b] = v;
            if (v > hi) hi = v;
        }
    for (int t = 0; t < kWinFrames; ++t)
        for (int b = 0; b < kCqtBins; ++b) {
            const float v = (hi == 0.0f) ? 0.0f : w.cqt[t][b] / hi;
            w.cqt[t][b] = v * kBnMul + kBnAdd;
        }

    // --- 3. empilement harmonique --------------------------------------------
    static const int kShift[kHarm] = { -36, 0, 36, 57, 72, 84, 93, 101 };
    for (int c = 0; c < kHarm; ++c) {
        const int s = kShift[c];
        for (int t = 0; t < kWinFrames; ++t) {
            float* row = w.hs[c][t];
            const float* src = w.cqt[t];
            for (int f = 0; f < kContour; ++f) {
                const int j = f + s;
                row[f] = (j >= 0 && j < kCqtBins) ? src[j] : 0.0f;
            }
        }
    }

    // --- 4. le reseau --------------------------------------------------------
    const float* hs = &w.hs[0][0][0];
    // Contour : 3 x 39 (8 -> 8), ReLU, 5 x 5 (8 -> 1), sigmoide.
    conv(hs, kHarm, kWinFrames, kContour, kContour1W, kContour1B, 8, 3, 39, 1, 1, 19,
         &w.c1[0][0][0], kContour);
    relu(&w.c1[0][0][0], sizeof(w.c1) / sizeof(float));
    conv(&w.c1[0][0][0], 8, kWinFrames, kContour, kContour2W, kContour2B, 1, 5, 5, 1, 2, 2,
         contour, kContour);
    for (int i = 0; i < kWinFrames * kContour; ++i) contour[i] = sigmoid(contour[i]);
    // Notes : 7 x 7 pas 3 (1 -> 32), ReLU, 7 x 3 (32 -> 1), sigmoide.
    conv(contour, 1, kWinFrames, kContour, kNote1W, kNote1B, 32, 7, 7, 3, 3, 2,
         &w.n1[0][0][0], kNotes);
    relu(&w.n1[0][0][0], sizeof(w.n1) / sizeof(float));
    conv(&w.n1[0][0][0], 32, kWinFrames, kNotes, kNote2W, kNote2B, 1, 7, 3, 1, 3, 1,
         &w.o1[0][0][0], kNotes);
    for (int i = 0; i < kWinFrames * kNotes; ++i) {
        const float v = sigmoid((&w.o1[0][0][0])[i]);
        (&w.o1[0][0][0])[i] = v;
        note[i] = v;
    }
    // Onsets : 5 x 5 pas 3 sur l'empilement (8 -> 32), ReLU, concatene derriere
    // la note, 3 x 3 (33 -> 1), sigmoide.
    conv(hs, kHarm, kWinFrames, kContour, kOnset1W, kOnset1B, 32, 5, 5, 3, 2, 1,
         &w.o1[1][0][0], kNotes);
    relu(&w.o1[1][0][0], (size_t)32 * kWinFrames * kNotes);
    conv(&w.o1[0][0][0], 33, kWinFrames, kNotes, kOnset2W, kOnset2B, 1, 3, 3, 1, 1, 1,
         onset, kNotes);
    for (int i = 0; i < kWinFrames * kNotes; ++i) onset[i] = sigmoid(onset[i]);
}

}} // namespace cp::bp

