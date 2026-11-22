// L'entree MIDI de REAPER (midi_Input, MIDI_eventlist) imitee, compilee
// contre le vrai SDK : le test Rust la lit par ses tables virtuelles
// (tests/midi_abi.rs).

#include "reaper_plugin.h"

#define EXPORT extern "C" __declspec(dllexport)

class FakeList : public MIDI_eventlist {
 public:
  MIDI_event_t ev[16];
  int n = 0;
  void AddItem(MIDI_event_t *evt) {
    if (n < 16) ev[n++] = *evt;
  }
  MIDI_event_t *EnumItems(int *bpos) {
    if (!bpos || *bpos < 0 || *bpos >= n) return 0;
    return &ev[(*bpos)++];
  }
  void DeleteItem(int bpos) {}
  int GetSize() { return n * (int)sizeof(MIDI_event_t); }
  void Empty() { n = 0; }
  virtual ~FakeList() {}
};

class FakeIn : public midi_Input {
 public:
  FakeList list;
  void start() {}
  void stop() {}
  void SwapBufs(unsigned int timestamp) {}
  MIDI_eventlist *GetReadBuf() { return &list; }
};

static FakeIn g_in;

static void add(int off, unsigned char a, unsigned char b, unsigned char c) {
  MIDI_event_t e;
  e.frame_offset = off;
  e.size = 3;
  e.midi_message[0] = a;
  e.midi_message[1] = b;
  e.midi_message[2] = c;
  e.midi_message[3] = 0;
  g_in.list.AddItem(&e);
}

// Le bloc : deux impulsions, Start, une position (SPP 0x0110 = 272), une note.
EXPORT void mi_fill() {
  g_in.list.Empty();
  add(0, 0xF8, 0, 0);
  add(21333, 0xF8, 0, 0);
  add(512000, 0xFA, 0, 0);
  add(600000, 0xF2, 0x10, 0x02);
  add(700000, 0x90, 60, 100);
}

// L'entree 2 existe, les autres non.
EXPORT void *mi_get(int idx) { return idx == 2 ? (void *)&g_in : 0; }
