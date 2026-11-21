// cp_bp_io.h -- les trois fichiers que se passent Lua et cp_transcribe.exe.
//
// POURQUOI UN EXECUTABLE ET PAS LA DLL. L'analyse dure des secondes : dans la
// DLL, elle tiendrait un fil de REAPER, et tout appel neuf demanderait une
// montee de l'ABI (donc une DLL reconstruite, REAPER ferme, ou toute la suite
// repasse en `gfx`). Un processus a part ne bloque rien, ne peut pas faire
// tomber REAPER, et Lua le lance sans attendre (ExecProcess, -2) puis lit son
// fichier de progression a chaque frame.
//
// 1. L'AUDIO (Lua -> exe) : "CPA1", u32 taux (22050), u32 n, n float32 mono.
// 2. LE CACHE (exe -> exe) : "CPB1", u32 trames, puis note, onset, contour en
//    float32. Il permet de redecoder avec d'autres seuils sans relancer le
//    reseau : c'est ce que fait le curseur de sensibilite.
// 3. LES NOTES (exe -> Lua), texte :
//        CPTAB 1
//        frames <F> hop 256 sr 22050
//        <debut> <fin> <hauteur> <amp> <pic> <nb> <b1> .. <bnb>
//    debut et fin en trames ; bends en tiers de demi-ton, un par trame.
// Chaque fichier s'ecrit sous `<nom>.part` puis se RENOMME : Lua ne lit jamais
// un fichier a moitie ecrit (la regle de la video, ROADMAP_Video.md).
#pragma once

#include "cp_bp_analyze.h"
#include "cp_bp_notes.h"

#include <string>
#include <vector>

namespace cp { namespace bp {

bool read_audio(const std::string& path, std::vector<float>& out, std::string& err);
bool write_audio(const std::string& path, const float* x, size_t n);   // les harnais
bool write_cache(const std::string& path, const Posteriors& p);
bool read_cache(const std::string& path, Posteriors& p, std::string& err);
bool write_notes(const std::string& path, const Posteriors& p, const std::vector<Note>& notes);
// Ecrit `text` sous path.part puis renomme (remplace l'ancien).
bool write_atomic(const std::string& path, const void* data, size_t n);

}} // namespace cp::bp

