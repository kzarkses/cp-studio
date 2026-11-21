#define _CRT_SECURE_NO_WARNINGS
#include "cp_log.h"

#include <windows.h>
#include <cstdio>
#include <cstdarg>
#include <cstring>

namespace cp {
namespace log {

namespace {
FILE*     g_f = nullptr;
long long g_t0 = 0;
long long g_qpf = 0;

double secs() {
  if (!g_qpf) {
    LARGE_INTEGER f;
    QueryPerformanceFrequency(&f);
    g_qpf = f.QuadPart;
  }
  LARGE_INTEGER t;
  QueryPerformanceCounter(&t);
  if (!g_t0) g_t0 = t.QuadPart;
  return (double)(t.QuadPart - g_t0) / (double)g_qpf;
}
} // namespace

// ⚠⚠ QUI NOUS CHARGE ? PARCE QUE CE N'EST PAS TOUJOURS REAPER.
// `build_load_test.cmd` charge la DLL a chaque construction pour verifier
// qu'elle s'initialise. Il ouvrait donc CE journal, en mode << w >> — et
// EFFACAIT la session REAPER qu'on venait de passer a reproduire. Le defaut ne
// se voit pas : le fichier est bien la, il est juste vide de ce qu'on
// cherchait, et on croit que rien n'a ete trace. Ca vient de couter la seule
// trace qu'on avait d'une fenetre reelle.
//
// Le test n'ecrit donc plus dans le meme fichier. On ne le distingue pas par un
// drapeau -- il faudrait que l'appelant pense a le poser -- mais par le nom du
// PROCESSUS, qui ne ment pas et que personne n'a a maintenir.
bool host_is_reaper() {
  char exe[MAX_PATH];
  const DWORD n = GetModuleFileNameA(nullptr, exe, MAX_PATH);
  if (!n || n >= MAX_PATH) return true;   // dans le doute, le journal habituel
  const char* base = exe;
  for (const char* q = exe; *q; ++q) if (*q == '\\' || *q == '/') base = q + 1;
  return _stricmp(base, "reaper.exe") == 0;
}

void open() {
  if (g_f) return;
  char path[MAX_PATH];
  DWORD n = GetEnvironmentVariableA("APPDATA", path, MAX_PATH);
  if (!n || n >= MAX_PATH - 48) return;
  strcat(path, host_is_reaper() ? "\\REAPER\\CP_native.log"
                                : "\\REAPER\\CP_native.test.log");
  // ⚠ ON RECREE A CHAQUE CHARGEMENT. Un journal qui s'allonge d'une session sur
  // l'autre oblige a chercher ou commence la bonne ; celui-ci ne contient que
  // la session en cours, donc sa derniere ligne est toujours la bonne.
  g_f = fopen(path, "w");
  if (!g_f) return;
  secs();
  line("--- CP_Native charge ---");
}

void line(const char* fmt, ...) {
  if (!g_f || !fmt) return;
  std::fprintf(g_f, "[%8.3f] ", secs());
  va_list ap;
  va_start(ap, fmt);
  std::vfprintf(g_f, fmt, ap);
  va_end(ap);
  std::fputc('\n', g_f);
  // ⚠ VIDER, PAS SEULEMENT ECRIRE. Sans ceci, la derniere ligne — celle qui
  // designe le fautif — reste dans le tampon de la bibliotheque C et meurt avec
  // le processus. C'est precisement la ligne qu'on veut lire.
  std::fflush(g_f);
}

void close() {
  if (!g_f) return;
  line("--- CP_Native decharge ---");
  std::fclose(g_f);
  g_f = nullptr;
}

} // namespace log
} // namespace cp

