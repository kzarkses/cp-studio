// cp_bp_analyze.h -- un fichier entier par le reseau, et ce qu'on en garde.
//
// LE DECOUPAGE N'EST PAS CELUI DE BASIC PITCH, ET C'EST VOULU. Basic Pitch
// avance de 36 164 echantillons (141,27 trames) et garde 142 trames par
// fenetre : ses trames ne tombent pas sur une grille reguliere, et il recolle
// les temps avec un decalage empirique (MAGIC_ALIGNMENT_OFFSET). Ici on avance
// de 142 trames EXACTEMENT (36 352 echantillons) et on garde les trames 15..156
// de chaque fenetre : la trame globale g est centree sur l'echantillon g x 256,
// sans correction. Le contexte de part et d'autre d'une trame gardee reste de
// 15 trames au moins, comme chez Basic Pitch.
#pragma once

#include <atomic>
#include <cstddef>
#include <vector>

namespace cp { namespace bp {

constexpr int kKeepFrom   = 15;            // trames gardees : [15, 157)
constexpr int kKeepFrames = 142;
constexpr int kWinHop     = kKeepFrames * 256;   // 36 352 echantillons

struct Posteriors {
    int frames = 0;                 // trame g = echantillon g x 256
    std::vector<float> note;        // frames x 88
    std::vector<float> onset;       // frames x 88
    std::vector<float> contour;     // frames x 264
};

// Analyse `n` echantillons mono a 22 050 Hz. `threads` <= 0 : autant que de
// coeurs. `done` (facultatif) compte les fenetres finies, `total` dit combien
// il y en aura -- pour une barre de progression lue depuis un autre fil.
void analyze(const float* audio, size_t n, int threads, Posteriors& out,
             std::atomic<int>* done = nullptr, int* total = nullptr);

int window_count(size_t n);

}} // namespace cp::bp

