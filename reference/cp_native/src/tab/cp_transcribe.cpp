// cp_transcribe.cpp -- l'executable que CP Guitar Tab lance.
//
//   cp_transcribe run     <audio.cpa> <cache.cpb> <notes.txt> [reglages]
//   cp_transcribe analyze <audio.cpa> <cache.cpb>             [--threads N] [--progress f]
//   cp_transcribe notes   <cache.cpb> <notes.txt>             [reglages]
//   cp_transcribe window  <audio.cpa> <sortie.f32>   (une fenetre brute : confrontation ONNX)
//   cp_transcribe kernels <sortie.f32>                (les noyaux du CQT : idem)
//
// Reglages : --onset 0.5  --frame 0.3  --minlen-ms 127.7  --minpitch 21
//            --maxpitch 108  --no-melodia  --no-infer  --threads N  --progress f
//
// Lua ne lit ni la sortie ni le code de retour (il lance sans attendre) : une
// erreur s'ecrit dans `<notes ou cache>.err`, et la progression dans le fichier
// --progress (<fenetres finies> <total>), reecrit a chaque fenetre.
#include "cp_bp_analyze.h"
#include "cp_bp_io.h"
#include "cp_bp_model.h"
#include "cp_bp_notes.h"

#include <atomic>
#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <memory>
#include <string>
#include <thread>
#include <vector>

using namespace cp::bp;

namespace {

struct Args {
    std::vector<std::string> pos;
    NoteParams prm;
    double minlen_ms = 127.7;
    int threads = 0;
    std::string progress;
};

bool parse(int argc, char** argv, Args& a)
{
    for (int i = 2; i < argc; ++i) {
        const std::string s = argv[i];
        auto val = [&](double& d) {
            if (i + 1 >= argc) return false;
            d = std::atof(argv[++i]);
            return true;
        };
        double d = 0.0;
        if (s == "--onset") { if (!val(d)) return false; a.prm.onset_thresh = (float)d; }
        else if (s == "--frame") { if (!val(d)) return false; a.prm.frame_thresh = (float)d; }
        else if (s == "--minlen-ms") { if (!val(d)) return false; a.minlen_ms = d; }
        else if (s == "--minpitch") { if (!val(d)) return false; a.prm.min_pitch = (int)d; }
        else if (s == "--maxpitch") { if (!val(d)) return false; a.prm.max_pitch = (int)d; }
        else if (s == "--threads") { if (!val(d)) return false; a.threads = (int)d; }
        else if (s == "--progress") { if (i + 1 >= argc) return false; a.progress = argv[++i]; }
        else if (s == "--no-melodia") a.prm.melodia = false;
        else if (s == "--no-infer") a.prm.infer_onsets = false;
        else if (s.size() > 1 && s[0] == '-' && s[1] == '-') return false;
        else a.pos.push_back(s);
    }
    // Des seuils hors de 0..1 ne veulent rien dire : on borne, on ne refuse pas.
    auto clamp01 = [](float& v) { if (!(v >= 0.0f)) v = 0.0f; if (v > 1.0f) v = 1.0f; };
    clamp01(a.prm.onset_thresh);
    clamp01(a.prm.frame_thresh);
    a.prm.min_len = min_len_frames(a.minlen_ms);
    return true;
}

void fail(const std::string& where, const std::string& msg)
{
    std::fprintf(stderr, "cp_transcribe: %s\n", msg.c_str());
    if (!where.empty()) write_atomic(where + ".err", msg.data(), msg.size());
}

void put_progress(const std::string& path, int done, int total)
{
    if (path.empty()) return;
    char s[64];
    const int n = std::snprintf(s, sizeof s, "%d %d\n", done, total);
    write_atomic(path, s, (size_t)n);
}

bool do_analyze(const std::string& audio, const std::string& cache, const Args& a, Posteriors& p)
{
    std::vector<float> x;
    std::string err;
    if (!read_audio(audio, x, err)) { fail(cache, err); return false; }
    std::atomic<int> done(0);
    int total = window_count(x.size());
    put_progress(a.progress, 0, total);
    std::thread th([&]() { analyze(x.data(), x.size(), a.threads, p, &done); });
    int shown = -1;
    for (;;) {
        const int d = done.load();
        if (d != shown) { put_progress(a.progress, d, total); shown = d; }
        if (d >= total) break;
        std::this_thread::sleep_for(std::chrono::milliseconds(40));
    }
    th.join();
    put_progress(a.progress, total, total);
    if (!write_cache(cache, p)) { fail(cache, "cannot write " + cache); return false; }
    return true;
}

bool do_notes(const Posteriors& p, const std::string& out, const Args& a)
{
    std::vector<Note> notes;
    make_notes(p, a.prm, notes);
    if (!write_notes(out, p, notes)) { fail(out, "cannot write " + out); return false; }
    return true;
}

int usage()
{
    std::fprintf(stderr,
        "usage: cp_transcribe run <audio> <cache> <notes> [options]\n"
        "       cp_transcribe analyze <audio> <cache> [--threads N] [--progress f]\n"
        "       cp_transcribe notes <cache> <notes> [options]\n"
        "options: --onset T --frame T --minlen-ms MS --minpitch P --maxpitch P\n"
        "         --no-melodia --no-infer --threads N --progress FILE\n");
    return 1;
}

} // namespace

int main(int argc, char** argv)
{
    if (argc < 2) return usage();
    const std::string cmd = argv[1];
    Args a;
    if (!parse(argc, argv, a)) return usage();

    if (cmd == "run" && a.pos.size() == 3) {
        Posteriors p;
        if (!do_analyze(a.pos[0], a.pos[1], a, p)) return 2;
        return do_notes(p, a.pos[2], a) ? 0 : 2;
    }
    if (cmd == "analyze" && a.pos.size() == 2) {
        Posteriors p;
        return do_analyze(a.pos[0], a.pos[1], a, p) ? 0 : 2;
    }
    if (cmd == "notes" && a.pos.size() == 2) {
        Posteriors p;
        std::string err;
        if (!read_cache(a.pos[0], p, err)) { fail(a.pos[1], err); return 2; }
        return do_notes(p, a.pos[1], a) ? 0 : 2;
    }
    if (cmd == "window" && a.pos.size() == 2) {
        std::vector<float> x;
        std::string err;
        if (!read_audio(a.pos[0], x, err)) { fail("", err); return 2; }
        x.resize(kWinSamples, 0.0f);
        std::unique_ptr<Kernels> k(new Kernels());
        std::unique_ptr<Work> w(new Work());
        make_kernels(*k);
        std::vector<float> out((size_t)kWinFrames * (kNotes * 2 + kContour));
        float* note = out.data();
        float* onset = note + kWinFrames * kNotes;
        float* contour = onset + kWinFrames * kNotes;
        run_window(*k, *w, x.data(), note, onset, contour);
        return write_atomic(a.pos[1], out.data(), out.size() * 4) ? 0 : 2;
    }
    if (cmd == "kernels" && a.pos.size() == 1) {
        std::unique_ptr<Kernels> k(new Kernels());
        make_kernels(*k);
        return write_atomic(a.pos[0], k.get(), sizeof(Kernels)) ? 0 : 2;
    }
    return usage();
}

