// La verification de nos tables virtuelles et de nos structures, par le vrai
// compilateur C++ et le vrai SDK de REAPER (voir tests/pcm_abi.rs).
//
// Deux sens :
// - abi_drive : NOTRE source (Rust), appelee par ses methodes C++, comme REAPER
//   l'appelle ;
// - abi_make_source : une source C++, que Rust appelle par sa table.
// Et abi_layout donne les tailles et positions des champs que Rust declare.

#include <windows.h>
#include <cstddef>
#include <cstring>

#include "reaper_plugin.h"

#define EXPORT extern "C" __declspec(dllexport)

EXPORT int abi_layout(long long* out, int n) {
  const long long v[] = {
    (long long)sizeof(PCM_source_transfer_t),
    (long long)offsetof(PCM_source_transfer_t, time_s),
    (long long)offsetof(PCM_source_transfer_t, samplerate),
    (long long)offsetof(PCM_source_transfer_t, nch),
    (long long)offsetof(PCM_source_transfer_t, length),
    (long long)offsetof(PCM_source_transfer_t, samples),
    (long long)offsetof(PCM_source_transfer_t, samples_out),
    (long long)offsetof(PCM_source_transfer_t, midi_events),
    (long long)offsetof(PCM_source_transfer_t, approximate_playback_latency),
    (long long)offsetof(PCM_source_transfer_t, absolute_time_s),
    (long long)offsetof(PCM_source_transfer_t, force_bpm),

    (long long)sizeof(preview_register_t),
    (long long)offsetof(preview_register_t, cs),
    (long long)offsetof(preview_register_t, src),
    (long long)offsetof(preview_register_t, m_out_chan),
    (long long)offsetof(preview_register_t, curpos),
    (long long)offsetof(preview_register_t, loop),
    (long long)offsetof(preview_register_t, volume),
    (long long)offsetof(preview_register_t, peakvol),
    (long long)offsetof(preview_register_t, preview_track),

    (long long)sizeof(audio_hook_register_t),
    (long long)offsetof(audio_hook_register_t, OnAudioBuffer),
    (long long)offsetof(audio_hook_register_t, userdata1),
    (long long)offsetof(audio_hook_register_t, userdata2),
    (long long)offsetof(audio_hook_register_t, input_nch),
    (long long)offsetof(audio_hook_register_t, output_nch),
    (long long)offsetof(audio_hook_register_t, GetBuffer),

    (long long)offsetof(PCM_source_peaktransfer_t, start_time),
    (long long)offsetof(PCM_source_peaktransfer_t, peakrate),
    (long long)offsetof(PCM_source_peaktransfer_t, numpeak_points),
    (long long)offsetof(PCM_source_peaktransfer_t, nchpeaks),
    (long long)offsetof(PCM_source_peaktransfer_t, peaks),
    (long long)offsetof(PCM_source_peaktransfer_t, peaks_out),

    (long long)sizeof(CRITICAL_SECTION),
    (long long)sizeof(ReaSample),

    (long long)sizeof(MIDI_event_t),
    (long long)offsetof(MIDI_event_t, frame_offset),
    (long long)offsetof(MIDI_event_t, size),
    (long long)offsetof(MIDI_event_t, midi_message),
  };
  const int count = (int)(sizeof(v) / sizeof(v[0]));
  for (int i = 0; i < count && i < n; ++i) out[i] = v[i];
  return count;
}

// Notre source, appelee comme REAPER l'appelle. `info` recoit ce que chaque
// methode a rendu ; `buf` recoit le bloc.
EXPORT int abi_drive(PCM_source* s, double* buf, int frames, int nch,
                     double time_s, double srate, double* info,
                     char* type, int typelen) {
  int k = 0;
  info[k++] = s->IsAvailable() ? 1.0 : 0.0;
  info[k++] = (double)s->GetNumChannels();
  info[k++] = s->GetSampleRate();
  info[k++] = s->GetLength();
  info[k++] = s->GetLengthBeats();
  info[k++] = (double)s->GetBitsPerSample();
  info[k++] = s->GetPreferredPosition();
  info[k++] = s->Duplicate() == nullptr ? 1.0 : 0.0;
  info[k++] = s->GetFileName() == nullptr ? 1.0 : 0.0;
  info[k++] = s->SetFileName("x") ? 1.0 : 0.0;
  info[k++] = s->GetSource() == nullptr ? 1.0 : 0.0;
  info[k++] = (double)s->PropertiesWindow(nullptr);
  info[k++] = (double)s->LoadState("x", nullptr);
  info[k++] = (double)s->PeaksBuild_Begin();
  info[k++] = (double)s->PeaksBuild_Run();
  info[k++] = (double)s->Extended(1, nullptr, nullptr, nullptr);

  PCM_source_peaktransfer_t pk;
  std::memset(&pk, 0, sizeof(pk));
  pk.peaks_out = 123;
  s->GetPeakInfo(&pk);
  info[k++] = (double)pk.peaks_out;

  s->SetAvailable(true);
  s->SetSource(nullptr);
  s->Peaks_Clear(false);
  s->PeaksBuild_Finish();
  s->SaveState(nullptr);

  PCM_source_transfer_t t;
  std::memset(&t, 0, sizeof(t));
  t.time_s = time_s;
  t.samplerate = srate;
  t.nch = nch;
  t.length = frames;
  t.samples = buf;
  t.approximate_playback_latency = 99.0;
  t.absolute_time_s = 7.0;
  s->GetSamples(&t);
  info[k++] = (double)t.samples_out;
  info[k++] = t.approximate_playback_latency;
  info[k++] = t.absolute_time_s;

  const char* ty = s->GetType();
  std::strncpy(type, ty ? ty : "", (size_t)typelen);
  if (typelen > 0) type[typelen - 1] = 0;
  return k;
}

// Une source C++ au comportement connu, et une methode virtuelle de plus : si
// Rust la trouve a la case 25, la table de PCM_source a bien 25 cases.
static int g_destroyed = 0;

class ProbeSource : public PCM_source {
 public:
  ~ProbeSource() override { ++g_destroyed; }
  PCM_source* Duplicate() override { return nullptr; }
  bool IsAvailable() override { return true; }
  const char* GetType() override { return "PROBE"; }
  bool SetFileName(const char*) override { return false; }
  int GetNumChannels() override { return 3; }
  double GetSampleRate() override { return 44100.0; }
  double GetLength() override { return 2.5; }
  int GetBitsPerSample() override { return 24; }
  int PropertiesWindow(HWND) override { return 0; }
  // Une frame de moins que demande : Rust doit le voir dans samples_out.
  void GetSamples(PCM_source_transfer_t* b) override {
    for (int i = 0; i < b->length; ++i)
      for (int c = 0; c < b->nch; ++c)
        b->samples[i * b->nch + c] = b->time_s + i * 0.001 + c * 10.0 + b->samplerate * 1e-6;
    b->samples_out = b->length - 1;
  }
  void GetPeakInfo(PCM_source_peaktransfer_t*) override {}
  void SaveState(ProjectStateContext*) override {}
  int LoadState(const char*, ProjectStateContext*) override { return -1; }
  void Peaks_Clear(bool) override {}
  int PeaksBuild_Begin() override { return 0; }
  int PeaksBuild_Run() override { return 0; }
  void PeaksBuild_Finish() override {}
  virtual int Extra() { return 77; }
};

EXPORT PCM_source* abi_make_source() { return new ProbeSource(); }
EXPORT void abi_destroy_source(PCM_source* p) { delete p; }
EXPORT int abi_destroyed() { return g_destroyed; }

// Une liste MIDI comme celle de REAPER : notre source y ajoute ses messages
// par la table virtuelle de MIDI_eventlist.
class ProbeMidiList : public MIDI_eventlist {
 public:
  MIDI_event_t ev[64];
  int n = 0;
  void AddItem(MIDI_event_t* e) override {
    if (n < 64) ev[n++] = *e;
  }
  MIDI_event_t* EnumItems(int* bpos) override {
    int i = bpos ? *bpos : 0;
    if (i < 0 || i >= n) return nullptr;
    if (bpos) *bpos = i + 1;
    return &ev[i];
  }
  void DeleteItem(int) override {}
  int GetSize() override { return n * (int)sizeof(MIDI_event_t); }
  void Empty() override { n = 0; }
  ~ProbeMidiList() override {}
};

// GetSamples avec une liste MIDI : rend le nombre de messages, et pour chacun
// (frame, taille, octet 0, octet 1, octet 2) dans `out`.
EXPORT int abi_drive_midi(PCM_source* s, double* buf, int frames, int nch,
                          int* out, int cap) {
  ProbeMidiList list;
  PCM_source_transfer_t t;
  std::memset(&t, 0, sizeof(t));
  t.time_s = 0.0;
  t.samplerate = 48000.0;
  t.nch = nch;
  t.length = frames;
  t.samples = buf;
  t.midi_events = &list;
  s->GetSamples(&t);
  int k = 0;
  for (int i = 0; i < list.n && k + 5 <= cap; ++i) {
    out[k++] = list.ev[i].frame_offset;
    out[k++] = list.ev[i].size;
    out[k++] = list.ev[i].midi_message[0];
    out[k++] = list.ev[i].midi_message[1];
    out[k++] = list.ev[i].midi_message[2];
  }
  return list.n;
}
