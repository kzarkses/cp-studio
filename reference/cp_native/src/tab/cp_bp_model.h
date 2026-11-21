// cp_bp_model.h -- le reseau de Basic Pitch, une fenetre a la fois.
//
// Basic Pitch (Spotify, ICASSP 2022, Apache-2.0) : un CQT a 3 cases par
// demi-ton, empile sur huit harmoniques, puis six convolutions. Il rend trois
// matrices par fenetre de 2 s : NOTE (88 hauteurs, la note sonne), ONSET (88,
// la note COMMENCE) et CONTOUR (264 cases d'un tiers de demi-ton, la hauteur
// fine -- c'est la que se lisent les bends).
//
// CE FICHIER REJOUE LE GRAPHE ONNX DU MODELE, A LA LETTRE. Les poids sont dans
// cp_bp_weights.h (genere) ; le CQT, son filtre et ses longueurs sont CALCULES
// ici, avec les formules de nnAudio (CQT2010v2) et de scipy.signal.firwin2 que
// le modele a vues a l'entrainement. Un CQT a peine different, et le reseau
// voit autre chose que ce qu'il a appris : tab_test confronte donc les noyaux
// calcules a ceux du modele (par leurs sommes) et la sortie entiere a celle
// d'onnxruntime sur un signal de reference (tests/fixtures).
//
// Aucun fil, aucun etat global : `Work` est le brouillon d'UN fil, et
// `run_window` ne lit que `Kernels` (constant apres `make_kernels`).
#pragma once

namespace cp { namespace bp {

constexpr int kSr         = 22050;   // le modele ne connait que ce taux
constexpr int kHop        = 256;     // une trame = 256 echantillons (11,6 ms)
constexpr int kWinSamples = 43844;   // une fenetre : 2 s moins une trame
constexpr int kWinFrames  = 172;
constexpr int kNotes      = 88;      // A0 (MIDI 21) .. C8 (MIDI 108)
constexpr int kMidiOffset = 21;
constexpr int kContour    = 264;     // 88 x 3
constexpr int kCqtBins    = 309;     // 103 demi-tons x 3 (jusqu'a Nyquist)
constexpr int kOctaves    = 9;
constexpr int kBinsPerOct = 36;
constexpr int kTaps       = 256;     // longueur des noyaux du CQT et du filtre
constexpr int kHarm       = 8;       // harmoniques 0.5, 1, 2 .. 7

struct Kernels {
    float re[kBinsPerOct][kTaps];    // l'octave du haut ; les autres par
    float im[kBinsPerOct][kTaps];    // sous-echantillonnage du signal
    float lowpass[kTaps];            // le filtre de demi-bande (firwin2)
    float sqrtlen[kCqtBins];         // la normalisation << librosa >>
};

// Calcule les noyaux. A faire une fois ; le resultat est en lecture seule.
void make_kernels(Kernels& k);

// Le brouillon d'un fil. Gros (environ 8 Mo) : un par fil, jamais sur la pile.
struct Work {
    float sig[2][kWinSamples + 2 * kTaps];          // deux octaves en alternance
    float pad[kWinSamples + 2 * kTaps];             // signal reflechi
    float cqt[kWinFrames][kCqtBins];                // module, puis log normalise
    float hs[kHarm][kWinFrames][kContour];          // empilement harmonique
    float c1[8][kWinFrames][kContour];              // contour, 1re couche
    float n1[32][kWinFrames][kNotes];               // notes, 1re couche
    float o1[33][kWinFrames][kNotes];               // [0] = note, [1..32] = onset
};

// Une fenetre de kWinSamples echantillons (taux kSr, mono) ->
//   note[kWinFrames][kNotes], onset[kWinFrames][kNotes],
//   contour[kWinFrames][kContour], toutes dans 0..1.
void run_window(const Kernels& k, Work& w, const float* in,
                float* note, float* onset, float* contour);

}} // namespace cp::bp

