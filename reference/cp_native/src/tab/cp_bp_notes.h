// cp_bp_notes.h -- des matrices du reseau aux notes.
//
// Le decodage de Basic Pitch (note_creation.py, output_to_notes_polyphonic +
// get_pitch_bends), porte a la lettre, avec les memes valeurs par defaut :
//   1. les onsets sont completes par les MONTEES franches de la matrice des
//      notes (infer_onsets) ;
//   2. chaque pic d'onset au-dessus du seuil ouvre une note, du plus tard au
//      plus tot, qui dure tant que la matrice des notes reste au-dessus du
//      seuil de trame (11 trames de tolerance) ; ce qu'elle couvre est retire
//      de l'energie restante, avec les deux demi-tons voisins ;
//   3. << l'astuce melodia >> : ce qui reste d'energie au-dessus du seuil
//      devient des notes SANS onset, du plus fort au plus faible -- c'est ce qui
//      rattrape un legato (hammer-on, pull-off) dont l'attaque ne se voit pas.
//      Le plus fort se cherche dans un tas plutot qu'en rebalayant la matrice :
//      meme choix (valeur, puis la plus petite place), sans le cout quadratique.
//   4. le bend de chaque note : la case de contour la plus forte, a +-25 tiers
//      de demi-ton, ponderee par une gaussienne (ecart-type 5), trame par trame.
//
// Ajout a Basic Pitch : `peak`, la force de l'onset qui a ouvert la note (0
// pour une note de l'astuce melodia). Lua s'en sert pour dire << lie >>.
#pragma once

#include "cp_bp_analyze.h"

#include <vector>

namespace cp { namespace bp {

struct NoteParams {
    float onset_thresh = 0.5f;      // DEFAULT_ONSET_THRESHOLD
    float frame_thresh = 0.3f;      // DEFAULT_FRAME_THRESHOLD
    int   min_len      = 11;        // trames (127,7 ms)
    int   energy_tol   = 11;
    bool  infer_onsets = true;
    bool  melodia      = true;
    int   min_pitch    = 21;        // MIDI, bornes comprises
    int   max_pitch    = 108;
};

struct Note {
    int   start = 0, end = 0;       // trames, [start, end)
    int   pitch = 0;                // MIDI
    float amp = 0.0f;               // moyenne de la matrice des notes, 0..1
    float peak = 0.0f;              // force de l'onset (0 : note melodia)
    std::vector<int> bends;         // tiers de demi-ton, une valeur par trame
};

int min_len_frames(double ms);      // np.round(ms / 1000 x 22050 / 256)

void make_notes(const Posteriors& p, const NoteParams& prm, std::vector<Note>& out);

}} // namespace cp::bp

