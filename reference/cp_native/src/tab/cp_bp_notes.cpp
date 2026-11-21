// cp_bp_notes.cpp -- voir cp_bp_notes.h.
#include "cp_bp_notes.h"
#include "cp_bp_model.h"

#include <algorithm>
#include <cmath>
#include <queue>

namespace cp { namespace bp {

int min_len_frames(double ms)
{
    if (!(ms >= 0.0)) ms = 0.0;
    return (int)std::nearbyint(ms / 1000.0 * kSr / kHop);   // np.round : au pair
}

namespace {

struct Cell {
    double v;
    int idx;                        // t x 88 + f
    bool operator<(const Cell& o) const
    {
        // priority_queue sort le PLUS GRAND : la plus forte valeur, et a valeur
        // egale la plus petite place (np.argmax rend la premiere).
        if (v != o.v) return v < o.v;
        return idx > o.idx;
    }
};

void bends_for(const Posteriors& p, Note& n)
{
    const int tol = 25, wl = 2 * tol + 1;
    double g[2 * 25 + 1];
    for (int i = 0; i < wl; ++i) {
        const double x = (i - tol) / 5.0;
        g[i] = std::exp(-0.5 * x * x);                     // gaussian(51, std=5)
    }
    const int fi = 3 * (n.pitch - kMidiOffset);
    const int fs = std::max(fi - tol, 0);
    const int fe = std::min(kContour, fi + tol + 1);
    const int gs = std::max(0, tol - fi);
    const int shift = tol - std::max(0, tol - fi);
    n.bends.clear();
    for (int t = n.start; t < n.end; ++t) {
        const float* row = &p.contour[(size_t)t * kContour];
        int best = 0;
        double bv = -1.0;
        for (int j = 0; j < fe - fs; ++j) {
            const double v = (double)row[fs + j] * g[gs + j];
            if (v > bv) { bv = v; best = j; }
        }
        n.bends.push_back(best - shift);
    }
}

} // namespace

void make_notes(const Posteriors& p, const NoteParams& prm, std::vector<Note>& out)
{
    out.clear();
    const int F = p.frames;
    if (F < 3) return;
    const int lo = std::max(0, prm.min_pitch - kMidiOffset);
    const int hi = std::min(kNotes - 1, prm.max_pitch - kMidiOffset);

    // Les deux matrices, bornees en hauteur (constrain_frequency).
    std::vector<float> frames(p.note), onsets(p.onset);
    for (int t = 0; t < F; ++t)
        for (int f = 0; f < kNotes; ++f)
            if (f < lo || f > hi) {
                frames[(size_t)t * kNotes + f] = 0.0f;
                onsets[(size_t)t * kNotes + f] = 0.0f;
            }

    // 1. Onsets deduits des montees (get_infered_onsets, n_diff = 2).
    if (prm.infer_onsets) {
        std::vector<float> diff((size_t)F * kNotes, 0.0f);
        float dmax = 0.0f, omax = 0.0f;
        for (size_t i = 0; i < onsets.size(); ++i) omax = std::max(omax, onsets[i]);
        for (int t = 2; t < F; ++t)
            for (int f = 0; f < kNotes; ++f) {
                const float x = frames[(size_t)t * kNotes + f];
                const float d1 = x - frames[(size_t)(t - 1) * kNotes + f];
                const float d2 = x - frames[(size_t)(t - 2) * kNotes + f];
                float d = std::min(d1, d2);
                if (d < 0.0f) d = 0.0f;
                diff[(size_t)t * kNotes + f] = d;
                dmax = std::max(dmax, d);
            }
        if (dmax > 0.0f)
            for (size_t i = 0; i < onsets.size(); ++i)
                onsets[i] = std::max(onsets[i], omax * diff[i] / dmax);
    }

    std::vector<double> rem(frames.begin(), frames.end());
    auto R = [&](int t, int f) -> double& { return rem[(size_t)t * kNotes + f]; };
    auto clear3 = [&](int t, int f) {
        R(t, f) = 0.0;
        if (f < kNotes - 1) R(t, f + 1) = 0.0;
        if (f > 0) R(t, f - 1) = 0.0;
    };
    auto mean = [&](int a, int b, int f) {
        double s = 0.0;
        for (int t = a; t < b; ++t) s += frames[(size_t)t * kNotes + f];
        return (b > a) ? s / (b - a) : 0.0;
    };

    // 2. Les pics d'onset, du plus tard au plus tot (np.where a l'envers).
    for (int t0 = F - 2; t0 >= 1; --t0) {
        for (int f = kNotes - 1; f >= 0; --f) {
            const float v = onsets[(size_t)t0 * kNotes + f];
            if (!(v >= prm.onset_thresh)) continue;
            if (!(v > onsets[(size_t)(t0 - 1) * kNotes + f] &&
                  v > onsets[(size_t)(t0 + 1) * kNotes + f])) continue;
            if (t0 >= F - 1) continue;
            int i = t0 + 1, k = 0;
            while (i < F - 1 && k < prm.energy_tol) {
                if (R(i, f) < prm.frame_thresh) ++k; else k = 0;
                ++i;
            }
            i -= k;
            if (i - t0 <= prm.min_len) continue;
            for (int t = t0; t < i; ++t) clear3(t, f);
            Note n;
            n.start = t0; n.end = i; n.pitch = f + kMidiOffset;
            n.amp = (float)mean(t0, i, f);
            n.peak = v;
            out.push_back(n);
        }
    }

    // 3. L'astuce melodia : l'energie restante, de la plus forte a la plus faible.
    if (prm.melodia) {
        std::priority_queue<Cell> heap;
        for (int idx = 0; idx < F * kNotes; ++idx)
            if (rem[(size_t)idx] > prm.frame_thresh) heap.push(Cell{ rem[(size_t)idx], idx });
        while (!heap.empty()) {
            const Cell c = heap.top();
            heap.pop();
            if (rem[(size_t)c.idx] != c.v) continue;          // deja effacee
            const int i_mid = c.idx / kNotes, f = c.idx % kNotes;
            R(i_mid, f) = 0.0;
            int i = i_mid + 1, k = 0;
            while (i < F - 1 && k < prm.energy_tol) {
                if (R(i, f) < prm.frame_thresh) ++k; else k = 0;
                clear3(i, f);
                ++i;
            }
            const int i_end = i - 1 - k;
            i = i_mid - 1; k = 0;
            while (i > 0 && k < prm.energy_tol) {
                if (R(i, f) < prm.frame_thresh) ++k; else k = 0;
                clear3(i, f);
                --i;
            }
            const int i_start = i + 1 + k;
            if (i_end - i_start <= prm.min_len) continue;
            Note n;
            n.start = i_start; n.end = i_end; n.pitch = f + kMidiOffset;
            n.amp = (float)mean(i_start, i_end, f);
            n.peak = 0.0f;
            out.push_back(n);
        }
    }

    // 4. Les bends.
    for (Note& n : out) bends_for(p, n);
}

}} // namespace cp::bp

