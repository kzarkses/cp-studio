// L'etireur de REAPER (IReaperPitchShift) imite, compile contre le vrai SDK :
// chaque methode note son numero et ses arguments. Le test Rust l'appelle
// par sa table virtuelle et verifie que chaque appel tombe sur la bonne
// methode (tests/pitch_abi.rs).

#include <string.h>
#include "reaper_plugin.h"

#define EXPORT extern "C" __declspec(dllexport)

static double g_log[256];
static int g_n = 0;

static void note(double a, double b = 0.0) {
  if (g_n + 2 <= 256) {
    g_log[g_n++] = a;
    g_log[g_n++] = b;
  }
}

class Fake : public IReaperPitchShift {
 public:
  ReaSample buf[4096];
  int fed = 0;
  virtual ~Fake() { note(100, 0); }
  virtual void set_srate(double srate) { note(1, srate); }
  virtual void set_nch(int nch) { note(2, nch); }
  virtual void set_shift(double shift) { note(3, shift); }
  virtual void set_formant_shift(double shift) { note(4, shift); }
  virtual void set_tempo(double tempo) { note(5, tempo); }
  virtual void Reset() { note(6); }
  virtual ReaSample *GetBuffer(int size) {
    note(7, size);
    return buf;
  }
  virtual void BufferDone(int input_filled) {
    note(8, input_filled);
    fed = input_filled;
  }
  virtual void FlushSamples() { note(9); }
  virtual bool IsReset() {
    note(10);
    return true;
  }
  virtual int GetSamples(int requested_output, ReaSample *buffer) {
    note(11, requested_output);
    // Rend ce qui a ete donne, tel quel (deux canaux).
    int n = fed < requested_output ? fed : requested_output;
    memcpy(buffer, buf, sizeof(ReaSample) * n * 2);
    return n;
  }
  virtual void SetQualityParameter(int parm) { note(12, parm); }
  virtual int Extended(int call, void *parm1, void *parm2, void *parm3) {
    note(13, call);
    return 42;
  }
};

EXPORT IReaperPitchShift *ps_fake_new() {
  g_n = 0;
  return new Fake();
}

EXPORT int ps_fake_log(double *out, int n) {
  int k = g_n < n ? g_n : n;
  memcpy(out, g_log, sizeof(double) * k);
  return k;
}
