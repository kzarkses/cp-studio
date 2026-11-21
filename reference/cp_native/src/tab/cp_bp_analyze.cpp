// cp_bp_analyze.cpp -- voir cp_bp_analyze.h.
#include "cp_bp_analyze.h"
#include "cp_bp_model.h"

#include <algorithm>
#include <cstring>
#include <memory>
#include <thread>

namespace cp { namespace bp {

int window_count(size_t n)
{
    if (n == 0) return 0;
    const size_t frames = n / kHop + 1;
    return (int)((frames + kKeepFrames - 1) / kKeepFrames);
}

void analyze(const float* audio, size_t n, int threads, Posteriors& out,
             std::atomic<int>* done, int* total)
{
    out.frames = (n == 0) ? 0 : (int)(n / kHop + 1);
    const int nwin = window_count(n);
    if (total) *total = nwin;
    out.note.assign((size_t)out.frames * kNotes, 0.0f);
    out.onset.assign((size_t)out.frames * kNotes, 0.0f);
    out.contour.assign((size_t)out.frames * kContour, 0.0f);
    if (nwin == 0) return;

    static Kernels kernels;              // constant apres le premier appel
    static bool kernels_ready = false;
    if (!kernels_ready) { make_kernels(kernels); kernels_ready = true; }

    if (threads <= 0) threads = (int)std::thread::hardware_concurrency();
    if (threads < 1) threads = 1;
    if (threads > nwin) threads = nwin;

    std::atomic<int> next(0);
    auto worker = [&]() {
        std::unique_ptr<Work> w(new Work());
        std::vector<float> in(kWinSamples), note((size_t)kWinFrames * kNotes),
                           onset((size_t)kWinFrames * kNotes),
                           contour((size_t)kWinFrames * kContour);
        for (;;) {
            const int win = next.fetch_add(1);
            if (win >= nwin) break;
            // Fenetre win : echantillons [win x 36352 - 15 x 256, + 43844).
            const long long s0 = (long long)win * kWinHop - (long long)kKeepFrom * kHop;
            for (int i = 0; i < kWinSamples; ++i) {
                const long long j = s0 + i;
                in[i] = (j >= 0 && j < (long long)n) ? audio[j] : 0.0f;
            }
            run_window(kernels, *w, in.data(), note.data(), onset.data(), contour.data());
            for (int j = 0; j < kKeepFrames; ++j) {
                const int g = win * kKeepFrames + j;
                if (g >= out.frames) break;
                const int src = kKeepFrom + j;
                std::memcpy(&out.note[(size_t)g * kNotes], &note[(size_t)src * kNotes],
                            sizeof(float) * kNotes);
                std::memcpy(&out.onset[(size_t)g * kNotes], &onset[(size_t)src * kNotes],
                            sizeof(float) * kNotes);
                std::memcpy(&out.contour[(size_t)g * kContour], &contour[(size_t)src * kContour],
                            sizeof(float) * kContour);
            }
            if (done) done->fetch_add(1);
        }
    };
    std::vector<std::thread> pool;
    for (int t = 1; t < threads; ++t) pool.emplace_back(worker);
    worker();
    for (auto& th : pool) th.join();
}

}} // namespace cp::bp

