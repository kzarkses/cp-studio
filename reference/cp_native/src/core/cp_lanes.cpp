#include "cp_lanes.h"

#include <cmath>
#include "cp_curve.h"

namespace cp {

// ---------------------------------------------------------------------------
// Lane
// ---------------------------------------------------------------------------
Lane::Lane() { reset(); }

void Lane::reset() {
  // NE TOUCHE PAS a buf[] : les pointeurs appartiennent a Lanes::init, et un
  // reset qui les effacerait ferait lire le fil audio a l'adresse zero.
  port.store(-1, std::memory_order_relaxed);
  channel.store(0, std::memory_order_relaxed);
  bars.store(1.0, std::memory_order_relaxed);
  muted.store(0, std::memory_order_relaxed);
  user_muted.store(0, std::memory_order_relaxed);
  tag.store(0.0, std::memory_order_relaxed);
  phase_off.store(0.0, std::memory_order_relaxed);
  play_from.store(-1.0, std::memory_order_relaxed);
  ts_num.store(0.0, std::memory_order_relaxed);
  loop_a.store(0.0, std::memory_order_relaxed);
  loop_b.store(-1.0, std::memory_order_relaxed);   // pas d'accolade
  nbuf.store(0, std::memory_order_relaxed);
  ncount[0].store(0, std::memory_order_relaxed);
  ncount[1].store(0, std::memory_order_relaxed);
  for (int i = 0; i < 128; ++i) {
    sounding[i] = 0; spass[i] = 0; sstart[i] = 0.0f; send[i] = 0.0;
  }
  // NE TOUCHE PAS a autos[] non plus (meme raison que buf[]) : seulement a ce
  // que le fil audio en a retenu.
  auto_gen.store(0, std::memory_order_relaxed);
  auto_seen = 0xFFFFFFFFu;
  for (int c = 0; c < kMaxAutoCurves; ++c) { auto_last[c] = -1; auto_seg[c] = 0; }
  stop_due = -1.0;
  rec_start = 0.0;
  for (int b = 0; b < 2; ++b) {
    std::memset(&abuf[b], 0, sizeof(LaneAudio));
    abuf[b].port = -1;
    abuf[b].clip = -1;
    abuf[b].v[0] = abuf[b].v[1] = kNullVoice;
    abuf[b].rate = 1.0;
    abuf[b].gain = 1.0;
  }
  apub.store(0, std::memory_order_relaxed);
  au = abuf[0];
  au_run = 0;
  au_cur = 1;          // la premiere passe prend la voix 0
  au_last = 0.0;
  au_expect = 0.0;
  au_fresh = kWaitClock;
  au_prate = 1.0;
  au_pub.store(-1, std::memory_order_relaxed);
  mode.store(kLaneEmpty, std::memory_order_relaxed);
  pending.store(kPendNone, std::memory_order_relaxed);
  pend_target.store(0.0, std::memory_order_relaxed);
  phase.store(0.0, std::memory_order_relaxed);
  len_beats.store(4.0, std::memory_order_relaxed);
  span_a.store(0.0, std::memory_order_relaxed);
  span_len.store(4.0, std::memory_order_relaxed);
  rec_gen.store(0, std::memory_order_relaxed);
}

// ---------------------------------------------------------------------------
// Lanes
// ---------------------------------------------------------------------------
Lanes::Lanes()
    : note_store_(nullptr), auto_store_(nullptr), au_tempo_(120.0), au_prate_(1.0),
      freerun_(1), launch_q_(0.0), audio_run_(0), pub_beat_(0.0),
      pub_active_(0), dropped_(0), srate_(48000.0), free_beat_(0.0),
      prev_active_(0), prev_freerun_(-1), last_pb_(0.0),
      fol_pb_(0.0), fol_ok_(0), anchor_seq_(0), fol_seq_(0), fol_miss_(0),
      fol_err_(0.0), fol_resync_(0), au_cuts_(0), flushes_(0), launches_(0),
      auto_sent_(0) {}

Lanes::~Lanes() {
  delete[] note_store_; note_store_ = nullptr;
  delete[] auto_store_; auto_store_ = nullptr;
}

void Lanes::init(double srate) {
  srate_ = (srate > 1.0) ? srate : 48000.0;
  // LA SEULE allocation de ce module, et elle a lieu ici — hors du fil audio,
  // une fois pour la vie du moteur.
  if (!note_store_) {
    note_store_ = new LaneNote[(size_t)kMaxLanes * 2 * kMaxLaneNotes];
  }
  std::memset(note_store_, 0,
              (size_t)kMaxLanes * 2 * kMaxLaneNotes * sizeof(LaneNote));
  // Les lots d'automation : la meme regle, une allocation a l'init. 2 x 32 lots
  // de 6 Ko -- 390 Ko, la moitie des notes.
  if (!auto_store_) auto_store_ = new AutoSet[(size_t)kMaxLanes * 2];
  std::memset(auto_store_, 0, (size_t)kMaxLanes * 2 * sizeof(AutoSet));
  for (int i = 0; i < kMaxLanes; ++i) {
    lanes_[i].buf[0] = note_store_ + (size_t)(i * 2 + 0) * kMaxLaneNotes;
    lanes_[i].buf[1] = note_store_ + (size_t)(i * 2 + 1) * kMaxLaneNotes;
    lanes_[i].autos[0] = auto_store_ + (size_t)(i * 2 + 0);
    lanes_[i].autos[1] = auto_store_ + (size_t)(i * 2 + 1);
    lanes_[i].reset();
  }
  for (int p = 0; p < kMaxPorts; ++p) { pmidi_[p].count = 0; vout_[p].clear(); }
  cmds_.clear();
  free_beat_ = 0.0;
  prev_active_ = 0;
  prev_freerun_ = -1;
  last_pb_ = 0.0;
  fol_pb_ = 0.0; fol_ok_ = 0; anchor_seq_ = 0; fol_seq_ = 0;
  fol_miss_ = 0; fol_err_ = 0.0;
  pub_beat_.store(0.0, std::memory_order_relaxed);
  pub_active_.store(0, std::memory_order_relaxed);
  dropped_.store(0, std::memory_order_relaxed);
  auto_sent_.store(0, std::memory_order_relaxed);
}

// ---------------------------------------------------------------------------
// Fil principal
// ---------------------------------------------------------------------------
void Lanes::publish_transport(double tempo, double beat, int playing,
                              double ts_num, frame_t at_frame, double rate) {
  // Sequence impaire = ecriture en cours. Le lecteur qui tombe dessus garde sa
  // derniere version coherente plutot que d'attendre : un bloc de retard sur le
  // tempo est sans consequence, un fil audio qui patiente ne l'est pas.
  const uint32_t s = tr_.seq.load(std::memory_order_relaxed);
  tr_.seq.store(s + 1, std::memory_order_release);
  tr_.tempo    = (tempo > 1.0) ? tempo : 120.0;
  tr_.beat     = beat;
  tr_.ts_num   = (ts_num >= 1.0) ? ts_num : 4.0;
  tr_.rate     = (rate > 0.0001) ? rate : 1.0;
  tr_.playing  = playing;
  tr_.at_frame = at_frame;
  tr_.seq.store(s + 2, std::memory_order_release);
}

bool Lanes::post(int lane, int cmd, double arg) {
  LaneCmd c;
  c.lane = lane;
  c.cmd  = cmd;
  c.arg  = arg;
  if (!cmds_.push(c)) {
    dropped_.fetch_add(1, std::memory_order_relaxed);
    return false;
  }
  return true;
}

LaneNote* Lanes::write_buf(int li) {
  if (li < 0 || li >= kMaxLanes) return nullptr;
  const int cur = lanes_[li].nbuf.load(std::memory_order_relaxed);
  return lanes_[li].buf[cur ^ 1];
}

void Lanes::publish_notes(int li, int count) {
  if (li < 0 || li >= kMaxLanes) return;
  if (count < 0) count = 0;
  if (count > kMaxLaneNotes) count = kMaxLaneNotes;
  Lane& L = lanes_[li];
  const int cur  = L.nbuf.load(std::memory_order_relaxed);
  const int next = cur ^ 1;
  L.ncount[next].store(count, std::memory_order_relaxed);
  // Le compte AVANT l'indice : le fil audio ne doit jamais voir un tampon
  // publie dont le compte appartient encore a l'autre.
  L.nbuf.store(next, std::memory_order_release);
}

int Lanes::note_count(int li) const {
  if (li < 0 || li >= kMaxLanes) return 0;
  const Lane& L = lanes_[li];
  const int cur = L.nbuf.load(std::memory_order_acquire);
  return L.ncount[cur].load(std::memory_order_relaxed);
}

// LA CASE AUDIO, cote fil principal. Le tampon non publie est d'abord recopie
// du publie : un appelant qui ne change que la region n'a pas a tout redire,
// et deux champs ne peuvent pas venir de deux versions differentes.
LaneAudio* Lanes::audio_back(int li) {
  if (li < 0 || li >= kMaxLanes) return nullptr;
  Lane& L = lanes_[li];
  const int cur = L.apub.load(std::memory_order_relaxed) & 1;
  L.abuf[cur ^ 1] = L.abuf[cur];
  return &L.abuf[cur ^ 1];
}

void Lanes::audio_publish(int li) {
  if (li < 0 || li >= kMaxLanes) return;
  Lane& L = lanes_[li];
  const int cur = L.apub.load(std::memory_order_relaxed) & 1;
  L.apub.store(cur ^ 1, std::memory_order_release);
}

const LaneNote* Lanes::read_buf(int li) const {
  if (li < 0 || li >= kMaxLanes) return nullptr;
  const Lane& L = lanes_[li];
  return L.buf[L.nbuf.load(std::memory_order_acquire)];
}

// L'AUTOMATION, cote fil principal. Le lot qui dort n'est PAS recopie du
// publie : un appelant redit toujours toutes ses courbes (c'est un clip qu'on
// arme), et un lot recopie garderait les courbes d'un clip precedent qu'on a
// oublie d'effacer.
AutoSet* Lanes::auto_back(int li) {
  if (li < 0 || li >= kMaxLanes) return nullptr;
  Lane& L = lanes_[li];
  return L.autos[(L.auto_gen.load(std::memory_order_relaxed) + 1) & 1];
}

bool Lanes::auto_publish(int li) {
  AutoSet* B = auto_back(li);
  if (!B) return false;
  Lane& L = lanes_[li];
  if (B->n < 0) B->n = 0;
  if (B->n > kMaxAutoCurves) B->n = kMaxAutoCurves;
  for (int c = 0; c < B->n; ++c) {
    AutoCurve& C = B->c[c];
    if (C.n < 0) C.n = 0;
    if (C.n > kMaxAutoPts) C.n = kMaxAutoPts;
    if (C.n > 0 && (C.cc < 0 || C.cc > 31)) return false;
    for (int i = 1; i < C.n; ++i) if (C.p[i].t < C.p[i - 1].t) return false;
  }
  // La courbe brute devient le facteur de shp, selon le sens du segment : la
  // regle de l'enveloppe d'un son (cp_curve.h), un pow par point.
  for (int c = 0; c < B->n; ++c) {
    AutoCurve& C = B->c[c];
    for (int i = 0; i < C.n; ++i) {
      C.p[i].k = (i + 1 < C.n) ? curve_factor(C.p[i].k, C.p[i + 1].v >= C.p[i].v)
                               : 1.0f;
    }
  }
  L.auto_gen.fetch_add(1, std::memory_order_release);
  return true;
}

int Lanes::auto_count(int li) const {
  if (li < 0 || li >= kMaxLanes) return 0;
  const Lane& L = lanes_[li];
  const AutoSet* A = L.autos[L.auto_gen.load(std::memory_order_acquire) & 1];
  return A ? A->n : 0;
}

// ---------------------------------------------------------------------------
// Fil audio — les outils
// ---------------------------------------------------------------------------
// LE PLANCHER EST LE SEIZIEME DE MESURE. Il etait au huitieme ; un son
// decoupe sur une double-croche (la grille par defaut de l'editeur) ne pouvait
// donc pas tourner sur sa propre longueur. A 120 BPM un seizieme de mesure fait
// 125 ms, soit six blocs de 1024 : la porte a de la marge, et sa degenerescence
// (un tampon plus long que la boucle) est deja traitee.
double Lanes::lane_len_beats(int li, double ts_num) const {
  double b = lanes_[li].bars.load(std::memory_order_relaxed);
  if (b < 0.0625) b = 0.0625;
  // LA SIENNE D'ABORD, celle du projet ensuite. Voir `Lane::ts_num` : sans ca,
  // une mesure en 3/4 traversee par le transport raccourcit toutes les boucles
  // du projet, y compris celles qui n'ont rien a voir avec elle.
  const double own = lanes_[li].ts_num.load(std::memory_order_relaxed);
  const double ts  = (own >= 1.0) ? own : ts_num;
  return b * ((ts >= 1.0) ? ts : 4.0);
}

// UNE ACCOLADE QUI NE TIENT PLUS DANS LA CASE NE DOIT PAS LA FAIRE TAIRE.
//
// C'est le seul cas ou cette fonction fait autre chose que recopier deux
// champs, et c'est celui qui arrive : on pose une accolade sur les mesures 3 et
// 4, puis on raccourcit la boucle a une mesure. Refuser, ou rendre une zone
// vide, rendrait la case muette sans qu'aucun geste ne l'ait demande — le pire
// des defauts, celui qu'on attribue au moteur. On RAMENE donc l'accolade dans
// la case, et si ce qu'il en reste n'est plus une duree musicale on revient a
// la case entiere.
//
// Le plancher est la double-croche : en dessous, une boucle n'est plus une
// boucle mais un bourdonnement, et une longueur nulle diviserait par zero deux
// lignes plus loin.
void Lanes::lane_span(int li, double ts_num, double* a, double* len) const {
  const double Lb = lane_len_beats(li, ts_num);
  const Lane& L = lanes_[li];
  double ba = L.loop_a.load(std::memory_order_relaxed);
  double bb = L.loop_b.load(std::memory_order_relaxed);
  if (ba < 0.0) ba = 0.0;
  if (bb > Lb)  bb = Lb;
  if (!(bb > ba + 0.25)) { *a = 0.0; *len = Lb; return; }
  *a = ba;
  *len = bb - ba;
}

// LA FENETRE DE TOLERANCE — une fraction du quantize, et pas une constante.
//
// Elle valait 0,05 beat, en dur. A 120 BPM cela fait 25 ms, et UNE MAIN HUMAINE
// EST EN RETARD DE 40 A 120 ms : le clic qui « visait » la frontiere tombait
// derriere elle et partait une mesure plus tard. Le musicien voyait sa case
// attendre alors qu'il avait joue juste — ou plutot, juste a l'echelle de ce
// qu'un doigt sait faire.
//
// Un huitieme de Q est la meme idee, exprimee dans l'unite qui compte : a
// Q: Bar et 120 BPM, 250 ms ; a Q: Beat, 62 ms ; a Q: 1/4 de beat, 15 ms. La
// tolerance suit donc la finesse demandee — serrer le quantize resserre la
// fenetre, ce qui est exactement ce qu'on veut dire en le serrant. C'est le
// « tolerant trigger sync » de FL, et la fraction est la sienne.
//
// Le plafond de 0,25 beat existe pour le cas ou quelqu'un met Q a 32 mesures :
// une fenetre de seize mesures ferait partir « tout de suite » un lancement
// qu'on voulait a la fin.
static inline double launch_tolerance(double q) {
  double t = q * 0.125;
  if (t > 0.25) t = 0.25;
  return t;
}

// OU TOMBE UN DEPART. Deux reponses, et seulement deux :
//   pas d'horloge -> kWaitClock : il en attend une. Un depart sans temps fort
//                    n'est pas un depart ; on lui donnera une vraie frontiere
//                    des qu'une horloge existera.
//   une horloge   -> la prochaine frontiere de quantize. Une position dans la
//                    fenetre de tolerance APRES une frontiere compte comme
//                    etant DESSUS, pour qu'un lancement arrive en retard
//                    appartienne encore a celle qu'il visait.
//
// Ce que cela ne fait PAS : traiter le premier bloc de l'horloge comme une
// frontiere. Le transport de REAPER demarre la ou le curseur trainait, et la
// grille de mesures existe qu'il roule ou non. Appuyer sur lecture entre deux
// mesures ne fait pas de cet instant une barre de mesure.
double Lanes::launch_target(double pb, bool active) const {
  if (!active) return kWaitClock;
  const double q = launch_q();
  if (q <= 0.001) return pb;
  const double ph = pb - std::floor(pb / q) * q;
  return (ph < launch_tolerance(q)) ? (pb - ph) : (pb - ph + q);
}

// OU COMMENCE UNE PRISE : AU PROCHAIN DEBUT DE PASSE DE CETTE LANE-CI.
//
// Ce n'est PAS la meme question que « ou tombe un depart », et les confondre
// produisait le defaut que Cedric decrit ainsi : « quand j'enregistre au piano,
// a chaque fois c'est le milieu du clip qui est le debut ».
//
// Voici la mecanique exacte. Une prise dure une passe entiere — elle se ferme
// toute seule apres `Lb` — et les notes sont ecrites A LA PHASE OU ELLES ONT
// ETE JOUEES, ce qui est la seule facon d'obtenir une boucle qui revient sur
// elle-meme. Si la prise ne COMMENCE pas a la phase zero, la boucle obtenue est
// la meme musique TOURNEE : ce qu'on a joue en premier revient au milieu.
//
// La grille de quantize ne peut pas repondre a cette question : elle ne connait
// pas la longueur de la case. Q a la mesure avec « Rec: 4 mesures » fait
// commencer la prise sur n'importe laquelle des quatre — donc trois fois sur
// quatre, tournee. A Q: Off elle repond « tout de suite », c'est-a-dire
// n'importe ou.
//
// La tolerance vaut ici ce qu'elle vaut ailleurs : appuyer juste APRES le temps
// fort prend CETTE passe, parce qu'une main est en retard de quarante a cent
// vingt millisecondes et qu'attendre une passe entiere pour ca serait absurde.
//
// L'overdub, lui, garde la grille : entrer en cours de boucle est exactement ce
// qu'on lui demande.
double Lanes::rec_target(int li, double pb, bool active, double ts_num) const {
  if (!active) return kWaitClock;
  double sa, Ls;
  lane_span(li, ts_num, &sa, &Ls);
  if (Ls <= 0.0) return pb;
  const double po = pb + lanes_[li].phase_off.load(std::memory_order_relaxed);
  const double ph = po - std::floor(po / Ls) * Ls;
  return (ph < launch_tolerance(Ls)) ? (pb - ph) : (pb + (Ls - ph));
}

// OU TOMBE UN ARRET. La meme frontiere tant qu'une horloge tourne — un clip
// finit sa mesure — mais MAINTENANT quand il n'y en a pas : rien ne sonne,
// donc il ne reste rien a finir et attendre ne ferait qu'echouer la lane.
double Lanes::stop_target(double pb, bool active) const {
  const double q = launch_q();
  if (!active || q <= 0.001) return pb;
  const double ph = pb - std::floor(pb / q) * q;
  return (ph < launch_tolerance(q)) ? (pb - ph) : (pb - ph + q);
}

void Lanes::emit(int port, frame_t at, unsigned char s, unsigned char d1,
                 unsigned char d2) {
  if (port < 0 || port >= kMaxPorts) return;
  PortMidi& q = pmidi_[port];
  if (q.count >= kPortMidiCap) return;   // sature : on perd, on ne bloque pas
  q.at[q.count] = at;
  q.msg[q.count][0] = s;
  q.msg[q.count][1] = d1;
  q.msg[q.count][2] = d2;
  ++q.count;
}

// Relacher tout ce que cette lane tient. Le seul endroit qui coupe une note de
// lecture, pour qu'aucun chemin ne puisse en oublier une.
void Lanes::flush_lane(int li, frame_t at) {
  Lane& L = lanes_[li];
  const int port = L.port.load(std::memory_order_relaxed);
  const int ch   = L.channel.load(std::memory_order_relaxed) & 0x0F;
  // Une lane qui se tait oublie ce qu'elle a dit de ses courbes : quand elle
  // reprendra, elle redira tout, au premier bloc. Le parametre a pu bouger
  // entre-temps (un autre clip de la colonne, la main de Cedric).
  for (int c = 0; c < kMaxAutoCurves; ++c) L.auto_last[c] = -1;
  bool any = false;
  for (int p = 0; p < 128; ++p) {
    if (L.sounding[p] > 0) {
      emit(port, at, (unsigned char)(0x80 + ch), (unsigned char)p, 0);
      L.sounding[p] = 0;
      any = true;
    }
  }
  if (any) flushes_.fetch_add(1, std::memory_order_relaxed);
}

void Lanes::all_notes_off() {
  for (int i = 0; i < kMaxLanes; ++i) flush_lane(i, 0);
}

// ---------------------------------------------------------------------------
// L'AUTOMATION, cote fil audio
// ---------------------------------------------------------------------------
// La lecture d'une courbe a la position `t` : la regle de `envp_eval`
// (cp_voice.cpp) et d'`Env.Eval` (CP_Engine/Env.lua). Avant le premier point,
// sa valeur ; apres le dernier, la sienne. Le curseur avance d'un cran en
// general ; un tour de boucle le fait reculer une fois. Borne par n <= 64.
static inline float auto_eval(const AutoPt* e, int n, float t, int16_t* cur) {
  int i = *cur;
  if (i < 0 || i >= n) i = 0;
  if (t < e[i].t) {
    while (i > 0 && t < e[i].t) --i;
  } else {
    while (i + 1 < n && t >= e[i + 1].t) ++i;
  }
  *cur = (int16_t)i;
  if (t <= e[0].t) return e[0].v;
  if (i + 1 >= n) return e[n - 1].v;
  const float dt = e[i + 1].t - e[i].t;
  if (dt <= 0.0f) return e[i + 1].v;
  const float x = (t - e[i].t) / dt;
  return e[i].v + (e[i + 1].v - e[i].v) * curve_shp(x, e[i].k);
}

void Lanes::run_auto(int li, int port, int ch, double pos, frame_t at) {
  Lane& L = lanes_[li];
  const uint32_t g = L.auto_gen.load(std::memory_order_acquire);
  const AutoSet* A = L.autos[g & 1];
  if (!A) return;
  if (g != L.auto_seen) {
    // UN LOT NEUF : tout se redit, et les curseurs repartent. La lane a pu
    // recevoir un autre clip, dont la premiere courbe n'a rien a voir.
    L.auto_seen = g;
    for (int c = 0; c < kMaxAutoCurves; ++c) { L.auto_last[c] = -1; L.auto_seg[c] = 0; }
  }
  const int nc = (A->n < kMaxAutoCurves) ? A->n : kMaxAutoCurves;
  for (int c = 0; c < nc; ++c) {
    const AutoCurve& C = A->c[c];
    if (C.n <= 0 || C.cc < 0 || C.cc > 31) continue;
    float v = auto_eval(C.p, C.n, (float)pos, &L.auto_seg[c]);
    if (v < 0.0f) v = 0.0f; else if (v > 1.0f) v = 1.0f;
    const int q = (int)(v * 16383.0f + 0.5f);
    if (q == L.auto_last[c]) continue;
    const unsigned char st = (unsigned char)(0xB0 + (ch & 0x0F));
    emit(port, at, st, (unsigned char)C.cc, (unsigned char)((q >> 7) & 0x7F));
    emit(port, at, st, (unsigned char)(C.cc + 32), (unsigned char)(q & 0x7F));
    L.auto_last[c] = (int16_t)q;
    auto_sent_.fetch_add(1, std::memory_order_relaxed);
  }
}

// ---------------------------------------------------------------------------
// Les commandes. On draine TOUT ce qui a ete ecrit depuis le dernier bloc, et
// dans l'ordre : un geste n'est jamais une commande. Echanger un clip en fait
// deux (arreter cette moitie, lancer l'autre) et une SCENE en fait une paire
// par colonne. Distillees une par bloc, elles pourraient tomber de part et
// d'autre d'une frontiere de quantize — la moitie d'une scene partant une
// mesure avant l'autre n'est pas un quantize, c'est un bug poli.
// ---------------------------------------------------------------------------
// « Elle part a CE beat, et on la veut a CETTE phase. » Le decalage est donc
// la difference, ramenee dans la boucle. Une seule ligne de calcul, mais elle
// doit vivre la ou la frontiere est CHOISIE : c'est le seul endroit du systeme
// qui connaisse `at_beat` avant que le premier echantillon ne sorte.
void Lanes::take_play_from(int li, double at_beat, double ts_num) {
  Lane& L = lanes_[li];
  launches_.fetch_add(1, std::memory_order_relaxed);
  // UN DEPART NEUF, et c'est ici qu'il se sait : cette fonction est appelee
  // partout ou un mode devient « joue » par un lancement. La case audio en
  // part a CE beat, dans ce bloc (`run_audio`) -- et non a la frame Lua
  // suivante, qui la faisait partir de 16 a 33 ms trop tard.
  L.au_fresh = at_beat;
  const double want = L.play_from.load(std::memory_order_relaxed);
  if (want < 0.0) return;
  L.play_from.store(-1.0, std::memory_order_relaxed);
  double sa, Ls;
  lane_span(li, ts_num, &sa, &Ls);
  if (Ls <= 0.0) return;
  // `want` est en coordonnees de CASE — c'est la que Lua a clique. La phase,
  // elle, court dans l'accolade : on retire donc son debut avant de reduire.
  double off = (want - sa) - at_beat;
  off -= std::floor(off / Ls) * Ls;
  L.phase_off.store(off, std::memory_order_relaxed);
}

void Lanes::drain_cmds(double pb, bool active, double ts_num, frame_t at) {
  LaneCmd c;
  while (cmds_.pop(c)) {
    const int li = c.lane;

    if (c.cmd == kLcPanic || c.cmd == kLcClearAll) {
      for (int i = 0; i < kMaxLanes; ++i) {
        Lane& L = lanes_[i];
        if (c.cmd == kLcClearAll) {
          L.mode.store(kLaneEmpty, std::memory_order_relaxed);
        } else {
          const int m = L.mode.load(std::memory_order_relaxed);
          if (m == kLanePlaying || m == kLaneOverdub) {
            L.mode.store(kLaneStopped, std::memory_order_relaxed);
          }
        }
        L.pending.store(kPendNone, std::memory_order_relaxed);
        flush_lane(i, at);
      }
      continue;
    }

    if (li < 0 || li >= kMaxLanes) continue;
    Lane& L = lanes_[li];
    const int  m   = L.mode.load(std::memory_order_relaxed);
    const int  pnd = L.pending.load(std::memory_order_relaxed);

    switch (c.cmd) {
      case kLcRec: {
        // Renvoyer REC pendant qu'il est en file l'annule (bascule).
        if (pnd == kPendRec) { L.pending.store(kPendNone, std::memory_order_relaxed); break; }
        if (!active) {
          // Horloge arretee : rien ne peut etre capture, donc vider ici
          // detruirait la boucle existante pour rien. On ARME : les notes
          // restent, et la capture commence des que l'horloge tourne.
          L.mode.store(kLaneArmed, std::memory_order_relaxed);
          break;
        }
        const double tq = rec_target(li, pb, active, ts_num);
        if (tq < kWaitTest || tq > pb + 0.0005) {
          L.pending.store(kPendRec, std::memory_order_relaxed);
          L.pend_target.store(tq, std::memory_order_relaxed);
        } else {
          flush_lane(li, at);
          L.rec_start = tq;
          L.mode.store(kLaneRec, std::memory_order_relaxed);
          L.rec_gen.fetch_add(1, std::memory_order_relaxed);
        }
        break;
      }

      case kLcStopRec: {
        if (pnd == kPendRec || pnd == kPendOverdub) {
          L.pending.store(kPendNone, std::memory_order_relaxed);
        }
        if (m == kLaneRec || m == kLaneOverdub) {
          const double tq = stop_target(pb, active);
          if (tq > pb + 0.0005) {
            L.pending.store(kPendStopRec, std::memory_order_relaxed);
            L.pend_target.store(tq, std::memory_order_relaxed);
          } else {
            L.mode.store(kLanePlaying, std::memory_order_relaxed);
          }
        } else if (m == kLaneArmed) {
          // annuler un armement : retomber sur ce que la lane tient deja
          L.mode.store(note_count(li) > 0 ? kLanePlaying : kLaneEmpty,
                       std::memory_order_relaxed);
        }
        break;
      }

      case kLcPlay: {
        if (pnd == kPendStop) {
          L.pending.store(kPendNone, std::memory_order_relaxed);  // garder le clip
          break;
        }
        if (m == kLaneStopped) {
          // Sans horloge ceci met en file plutot que de partir : un clip qui
          // « joue » contre un transport arrete est un clip qui ment — il ne
          // sonne rien, et il entrerait la ou le transport tombera plus tard.
          const double tq = launch_target(pb, active);
          if (tq < kWaitTest || tq > pb + 0.0005) {
            L.pending.store(kPendPlay, std::memory_order_relaxed);
            L.pend_target.store(tq, std::memory_order_relaxed);
          } else {
            take_play_from(li, pb, ts_num);
            L.mode.store(kLanePlaying, std::memory_order_relaxed);
          }
        }
        break;
      }

      case kLcStop: {
        if (pnd == kPendPlay || pnd == kPendRec || pnd == kPendOverdub) {
          L.pending.store(kPendNone, std::memory_order_relaxed);
          break;
        }
        if (m == kLanePlaying || m == kLaneOverdub) {
          const double tq = stop_target(pb, active);
          if (tq > pb + 0.0005) {
            L.pending.store(kPendStop, std::memory_order_relaxed);
            L.pend_target.store(tq, std::memory_order_relaxed);
          } else {
            L.mode.store(kLaneStopped, std::memory_order_relaxed);
            flush_lane(li, at);
          }
        }
        break;
      }

      case kLcOverdub: {
        if (pnd == kPendOverdub) { L.pending.store(kPendNone, std::memory_order_relaxed); break; }
        if (m == kLaneOverdub) {
          const double tq = stop_target(pb, active);
          if (tq > pb + 0.0005) {
            L.pending.store(kPendStopRec, std::memory_order_relaxed);
            L.pend_target.store(tq, std::memory_order_relaxed);
          } else {
            L.mode.store(kLanePlaying, std::memory_order_relaxed);
          }
        } else {
          // Entrer : NI vidage NI coupure des notes que la porte tient — une
          // lane qui joue glisse dans l'overdub sans se redeclencher. Entrer
          // est un DEPART, donc sans horloge il attend comme tout depart.
          const double tq = launch_target(pb, active);
          if (tq < kWaitTest || tq > pb + 0.0005) {
            L.pending.store(kPendOverdub, std::memory_order_relaxed);
            L.pend_target.store(tq, std::memory_order_relaxed);
          } else {
            L.mode.store(kLaneOverdub, std::memory_order_relaxed);
          }
        }
        break;
      }

      // Le rendez-vous. Voir kLcPlayAt dans l'en-tete : la date est celle de
      // l'appelant, et elle ne passe pas par la grille de quantize.
      case kLcPlayAt: {
        if (m == kLaneStopped) {
          L.pending.store(kPendPlay, std::memory_order_relaxed);
          L.pend_target.store(c.arg, std::memory_order_relaxed);
        }
        break;
      }

      case kLcStopAt: {
        if (m == kLanePlaying || m == kLaneOverdub) {
          L.pending.store(kPendStop, std::memory_order_relaxed);
          L.pend_target.store(c.arg, std::memory_order_relaxed);
        }
        break;
      }

      case kLcClear: {
        L.mode.store(kLaneEmpty, std::memory_order_relaxed);
        L.pending.store(kPendNone, std::memory_order_relaxed);
        flush_lane(li, at);
        break;
      }

      case kLcSetMode: {
        // Rappel de session : poser l'etat sans passer par une frontiere. Une
        // lane qu'on repose sur « arretee » ne doit rien laisser sonner.
        const int want = (int)c.arg;
        L.pending.store(kPendNone, std::memory_order_relaxed);
        if (want != kLanePlaying && want != kLaneOverdub) flush_lane(li, at);
        L.mode.store(want, std::memory_order_relaxed);
        break;
      }

      default: break;
    }
  }
}

// ---------------------------------------------------------------------------
// Les lancements en file, et l'arret automatique d'une prise apres une boucle.
// Tout ici tombe AVANT la porte, pour qu'une lane qui commence a jouer sur ce
// bloc joue des ce bloc.
// ---------------------------------------------------------------------------
void Lanes::run_pendings(double pb, bool active, double ts_num, frame_t at,
                         double block_beats) {
  if (!active) return;
  for (int li = 0; li < kMaxLanes; ++li) {
    Lane& L = lanes_[li];
    int m = L.mode.load(std::memory_order_relaxed);

    // arme -> enregistre. Avec un quantize, l'armement se convertit en une
    // prise en file a la prochaine frontiere plutot que de partir au premier
    // bloc : le transport qui roule en milieu de mesure ne fait pas de cet
    // instant une barre.
    if (m == kLaneArmed && L.pending.load(std::memory_order_relaxed) == kPendNone) {
      // Le debut de passe, pas la grille : voir rec_target.
      const double tq = rec_target(li, pb, active, ts_num);
      if (tq > pb + 0.0005) {
        L.pending.store(kPendRec, std::memory_order_relaxed);
        L.pend_target.store(tq, std::memory_order_relaxed);
      } else {
        flush_lane(li, at);
        L.rec_start = tq;
        L.mode.store(kLaneRec, std::memory_order_relaxed);
        L.rec_gen.fetch_add(1, std::memory_order_relaxed);
        m = kLaneRec;
      }
    }

    // Un arret pose pour un bloc que la porte n'a pas traite (transport
    // retombe entre-temps) : il est echu, on le resout ici.
    if (L.stop_due >= 0.0 && pb >= L.stop_due - 0.0005) {
      if (m == kLanePlaying || m == kLaneOverdub) {
        L.mode.store(kLaneStopped, std::memory_order_relaxed);
        flush_lane(li, at);
        m = kLaneStopped;
      }
      L.stop_due = -1.0;
    }

    int pnd = L.pending.load(std::memory_order_relaxed);
    if (pnd != kPendNone) {
      // Elle attendait UNE HORLOGE, et en voici une : on lui donne une vraie
      // frontiere maintenant. Pas cet instant — cet instant n'est une
      // frontiere que si le transport a demarre dessus.
      if (L.pend_target.load(std::memory_order_relaxed) < kWaitTest) {
        L.pend_target.store(launch_target(pb, active), std::memory_order_relaxed);
      }
      const double tgt = L.pend_target.load(std::memory_order_relaxed);
      // DEUX HORIZONS, ET ILS NE SONT PAS LE MEME.
      //
      // Ce qui COMMENCE se resout sur le bloc qui CONTIENT sa frontiere, pas
      // sur le premier bloc qui l'a depassee. La porte, elle, reconcilie deja
      // sur la phase de FIN de bloc : lui livrer une lane qui vient seulement
      // de passer a « joue » au bloc SUIVANT lui faisait manquer la frontiere
      // de quelques centaines d'echantillons, et la premiere note du lancement
      // sortait au debut de ce bloc-la. Mesure au harnais : 5,3 ms sur un bloc
      // de 2048, et jusqu'a un bloc entier selon ou tombe la frontiere. Avec le
      // meme horizon que la porte, elle tombe SUR son echantillon.
      //
      // Ce qui S'ARRETE garde l'ancien horizon, et ce n'est pas une omission :
      // un arret resolu sur le bloc qui contient sa frontiere couperait tout ce
      // bloc, donc jusqu'a une taille de tampon AVANT la frontiere. Un arret
      // quantifie promet exactement le contraire — le clip finit sa mesure. En
      // retard vaut mieux qu'en avance, pour lui seul.
      const bool starts = (pnd == kPendPlay || pnd == kPendRec
                           || pnd == kPendOverdub);
      const double horizon = starts ? (pb + block_beats) : pb;
      // UN ARRET DONT LA FRONTIERE TOMBE DANS CE BLOC se fait DANS ce bloc, a
      // sa frontiere : la porte joue jusqu'a elle et pas au-dela (voir
      // Lane::stop_due). Ni en avance -- le clip finit sa mesure -- ni en
      // retard d'un bloc sur la case qui lui succede.
      if (pnd == kPendStop && (m == kLanePlaying || m == kLaneOverdub)
          && tgt > pb + 0.0005 && tgt <= pb + block_beats) {
        L.stop_due = tgt;
        L.pending.store(kPendNone, std::memory_order_relaxed);
      } else if (horizon >= tgt - 0.0005) {
        switch (pnd) {
          case kPendPlay:
            if (m == kLaneStopped) {
              // `tgt`, PAS `pb` : la frontiere est celle que le moteur a
              // choisie, et c'est contre elle que le decalage doit etre exact.
              // Contre `pb` on serait faux de la fraction de beat qui separe le
              // debut du bloc de la frontiere — audible sur une boucle courte.
              take_play_from(li, tgt, ts_num);
              L.mode.store(kLanePlaying, std::memory_order_relaxed);
            }
            break;
          case kPendStop:
            if (m == kLanePlaying || m == kLaneOverdub) {
              L.mode.store(kLaneStopped, std::memory_order_relaxed);
              flush_lane(li, at);
            }
            break;
          case kPendRec:
            flush_lane(li, at);
            L.rec_start = tgt;
            L.mode.store(kLaneRec, std::memory_order_relaxed);
            L.rec_gen.fetch_add(1, std::memory_order_relaxed);
            break;
          case kPendStopRec:
            if (m == kLaneRec || m == kLaneOverdub) {
              L.mode.store(kLanePlaying, std::memory_order_relaxed);
            }
            break;
          case kPendOverdub:
            L.mode.store(kLaneOverdub, std::memory_order_relaxed);
            break;
          default: break;
        }
        L.pending.store(kPendNone, std::memory_order_relaxed);
      }
    }

    // Une prise se ferme d'elle-meme apres une boucle entiere (le REC a un
    // clic). Un stop-rec en file devient sans objet des que la prise s'est
    // fermee toute seule.
    if (L.mode.load(std::memory_order_relaxed) == kLaneRec) {
      const double Lb = lane_len_beats(li, ts_num);
      const double endb = L.rec_start + Lb;
      bool close = (pb - L.rec_start) >= Lb;
      // UNE PRISE AUDIO SE FERME DANS LE BLOC DE SA FIN, et sa matiere part
      // A sa fin (`au_fresh`). Fermee au bloc suivant comme une prise MIDI, la
      // premiere relecture partait jusqu'a un bloc en retard sur la grille --
      // et c'est la boucle qu'on vient de jouer, celle qu'on ecoute le plus.
      // La prise MIDI garde sa regle : Lua la finalise en voyant le mode
      // changer, et un bloc plus tot lui ferait perdre les notes du dernier.
      if (!close && pb + block_beats > endb + 1e-9 && cap_running_on(li)) {
        close = true;
        L.au_fresh = endb;
      }
      if (close) {
        L.mode.store(kLanePlaying, std::memory_order_relaxed);
        if (L.pending.load(std::memory_order_relaxed) == kPendStopRec) {
          L.pending.store(kPendNone, std::memory_order_relaxed);
        }
      }
    }
  }
}

// ---------------------------------------------------------------------------
// LA PORTE. A chaque bloc on reconcilie les notes qui SONNENT contre celles qui
// couvrent la phase courante. C'est le comportement de Live, et il est bien
// choisi : une edition prend effet au bloc suivant, et il n'y a jamais de note
// bloquee — parce qu'une note tenue est tenue par cette table, pas par un
// evenement qu'on aurait pu oublier d'emettre.
//
// Ce que le portage gagne sur le JSFX : les evenements sont dates au frame
// ABSOLU dans le bloc a venir, et non poses a l'offset zero. Une note qui
// commence au tiers du bloc commence au tiers du bloc.
// ---------------------------------------------------------------------------
// Le beat absolu, au plus tot a partir de `pb`, dont la phase dans une boucle
// de longueur Lb vaut `tp`. C'est la conversion « cette note commence a telle
// place dans la boucle » -> « a quel instant, cette fois-ci ».
static inline double phase_hit(double pb, double Lb, double tp) {
  const double p0 = pb - std::floor(pb / Lb) * Lb;
  double d = tp - p0;
  if (d < 0.0) d += Lb;
  return pb + d;
}

// OU DATER UNE TRANSITION DANS LE BLOC — ET CE QU'ON FAIT D'UNE TRANSITION
// DEJA PASSEE.
//
// `phase_hit` rend le PROCHAIN instant ou la phase vaut ce qu'on cherche. En
// regime etabli il tombe dans le bloc a venir, et on date la note a
// l'echantillon : c'est tout le gain de ce moteur sur le JSFX.
//
// Mais il existe des instants ou le point est DEJA PASSE quand on le demande :
// le premier bloc d'un lancement (la lane ne jouait pas, donc rien ne l'avait
// vu passer), un saut de phase, un changement d'accolade. `phase_hit` repart
// alors une boucle ENTIERE plus loin, et borner ce resultat a la FIN du bloc
// datait la note au plus tard possible.
//
// MESURE AU HARNAIS, et c'est elle qui a designe le coupable : sur un bloc de
// 2048, la premiere note d'un lancement quantifie sortait 48 ms apres son
// temps. Le raisonnement, lui, cherchait ailleurs depuis une heure.
//
// Une transition en retard part donc au DEBUT du bloc. C'est la meme regle que
// `drain_midi` applique deja aux evenements qu'elle trouve depasses — « mieux
// vaut une note d'un bloc en retard qu'une note muette » — et ici elle vaut
// mieux vaut un bloc de retard qu'un bloc et demi.
static inline frame_t hit_frame(double d, double bpf, double block_beats,
                                frame_t at, frame_t last) {
  if (bpf <= 0.0 || !(d <= block_beats)) return at;
  const frame_t f = at + (frame_t)(d / bpf + 0.5);
  return (f < at) ? at : ((f > last) ? last : f);
}

// ---------------------------------------------------------------------------
// LA CASE AUDIO DANS LA LANE (ABI 3.0)
// ---------------------------------------------------------------------------
// Ce qui etait `Cells.drive` + `Cells.playAt` en Lua, au meme endroit que la
// porte MIDI et avec la meme horloge. Les regles sont celles de Lua, gardees
// telles quelles -- elles avaient ete reglees a l'oreille le 2026-10-01 :
//
//   * une PASSE joue la region une fois (one-shot) ou la fait tourner (boucle
//     de matiere) ; elle commence a chaque frontiere de la zone de lecture ;
//   * la voix sortante descend en 5 ms APRES la frontiere pendant que
//     l'entrante attaque : un vrai fondu croise, d'ou deux voix par lane ;
//   * une matiere qui boucle entre a sa phase (elle reste calee sur la
//     grille), un one-shot repart toujours du debut de sa region ;
//   * fondu d'entree : rien sur une attaque, 5 ms en cours de matiere ou en
//     relance de boucle, 2 ms pour une tranche qui ne commence pas au zero ;
//   * une entree a moins de 80 ms d'un debut de passe rabat la MATIERE au
//     debut -- c'est l'attaque, et la manger s'entend.
//
// Ce qui change, c'est QUAND : au bloc ou la lane part, s'arrete, se tait ou
// saute, au lieu de la frame Lua suivante.
static const double kAuJumpBeats = 0.03;   // un saut de tete, pas une gigue d'ancre
static const double kAuSnapS     = 0.080;
static const double kAuFadeS     = 0.005;

void Lanes::au_post(int port, uint32_t type, uint32_t voice, frame_t at,
                    double a, double b, uint32_t u0, uint32_t u1) {
  if (port < 0 || port >= kMaxPorts || voice == kNullVoice) return;
  Cmd c;
  std::memset(&c, 0, sizeof(c));
  c.type = type; c.voice = voice; c.at = at;
  c.a = a; c.b = b; c.u0 = u0; c.u1 = u1;
  if (!vout_[port].push(c)) dropped_.fetch_add(1, std::memory_order_relaxed);
}

// Tout ce que la lane fait sonner se tait, MAINTENANT et en douceur. Une voix
// qui n'a pas encore sonne s'annule (stop_now, cp_engine.cpp) ; celle qui
// finit deja son fondu le garde.
void Lanes::au_cut(int li) {
  Lane& L = lanes_[li];
  const LaneAudio& A = L.au;
  if (L.au_run) au_cuts_.fetch_add(1, std::memory_order_relaxed);
  au_post(A.port, kCmdVoiceStop, A.v[0], kNow, kAuFadeS, 0.0, 0, 0);
  au_post(A.port, kCmdVoiceStop, A.v[1], kNow, kAuFadeS, 0.0, 0, 0);
  L.au_run = 0;
}

// Une passe part a `sb` (beat de la ligne de `pb`), sur la voix suivante.
void Lanes::au_start(int li, double sb, double pb, frame_t at, frame_t last,
                     double bpf, double block_beats, double sa, double Ls,
                     double off, bool relaunch) {
  Lane& L = lanes_[li];
  const LaneAudio& A = L.au;
  const double pos = sb + off;
  double ph = pos - std::floor(pos / Ls) * Ls;
  if (Ls - ph < 1e-7) ph = 0.0;
  const frame_t f = hit_frame(sb - pb, bpf, block_beats, at, last);
  const double spb = 60.0 / ((au_tempo_ > 1.0) ? au_tempo_ : 120.0);

  // LA MATIERE. Une boucle entre a sa phase -- EN COORDONNEES DE CASE, accolade
  // comprise, comme la phase que Lua lisait -- et se replie dans sa region.
  const bool snap = !relaunch && ph > 0.0 && ph * spb < kAuSnapS;
  const double phm = sa + (snap ? 0.0 : ph);
  const double reglen = A.f1 - A.f0;
  double offs = A.f0;
  double fin = 0.0;
  if (A.loop && phm > 0.0) {
    double o = std::floor(phm * spb * A.sr * A.rate + 0.5);
    if (reglen > 0.0) o -= std::floor(o / reglen) * reglen;
    offs = A.f0 + o;
    if (o > 0.0) fin = kAuFadeS;
  } else if (A.f0 > 0.0) {
    fin = 0.002;
  }
  if (relaunch && A.loop) fin = kAuFadeS;

  const int nv = L.au_cur ^ 1;
  const uint32_t h = A.v[nv];
  // La sortante descend APRES la frontiere : le moteur fait atteindre zero au
  // rendez-vous, donc on le date de la longueur du fondu.
  if (L.au_run) {
    const frame_t ff = (frame_t)(kAuFadeS * srate_ + 0.5);
    au_post(A.port, kCmdVoiceStop, A.v[L.au_cur], f + ff, kAuFadeS, 0.0, 0, 0);
  }
  // Les proprietes AVANT le lancement (il remet la tete a `loop_start`), la
  // position APRES : l'anneau est FIFO, tout tombe dans le meme bloc.
  au_post(A.port, kCmdVoiceSet, h, kNow, A.f0, 0.0, kParamLoopStart, 0);
  au_post(A.port, kCmdVoiceSet, h, kNow, A.f1, 0.0, kParamLoopEnd, 0);
  au_post(A.port, kCmdVoiceSet, h, kNow, fin, 0.0, kParamFadeIn, 0);
  au_post(A.port, kCmdVoicePlay, h, f, A.rate * au_prate_, A.gain,
          (uint32_t)A.clip, A.loop ? (uint32_t)kPlayLoop : (uint32_t)kPlayOnce);
  if (offs != A.f0) au_post(A.port, kCmdVoiceSet, h, kNow, offs, 0.0, kParamPos, 0);

  L.au_cur = nv;
  L.au_run = 1;
  L.au_last = (ph == 0.0) ? std::floor(pos / Ls + 0.5) * Ls : pos;
  L.au_prate = au_prate_;
}

void Lanes::run_audio(int li, double pb, double ts_num, frame_t at, int frames,
                      double block_beats) {
  Lane& L = lanes_[li];
  // La liaison du bloc. Copiee ici, une fois : la moitie d'une passe ne peut
  // pas lire un clip et l'autre moitie une region d'une autre version.
  L.au = L.abuf[L.apub.load(std::memory_order_acquire) & 1];
  const LaneAudio& A = L.au;
  const double fresh = L.au_fresh;
  L.au_fresh = kWaitClock;      // un depart neuf ne vaut que pour SON bloc

  const int m = L.mode.load(std::memory_order_relaxed);
  // ⚠ LE MUTE MECANIQUE NE COMPTE PAS ICI. La Session le pose sur toute case
  // audio pour que sa note unique n'atteigne pas l'instrument de la colonne ;
  // le lire ici rendrait toutes les cases audio muettes. Seul le mute MUSICAL
  // tait le son (voir Lane::user_muted).
  const bool plays = A.port >= 0 && A.port < kMaxPorts && A.clip >= 0
                     && A.f1 > A.f0 && A.sr > 0.0
                     && (m == kLanePlaying || m == kLaneOverdub)
                     && !L.user_muted.load(std::memory_order_relaxed);
  if (!plays) {
    if (L.au_run) au_cut(li);
    L.au_pub.store(-1, std::memory_order_relaxed);
    return;
  }

  double sa, Ls;
  lane_span(li, ts_num, &sa, &Ls);
  const double off = L.phase_off.load(std::memory_order_relaxed);
  const double bpf = (frames > 0) ? (block_beats / (double)frames) : 0.0;
  const frame_t last = at + (frame_t)(frames > 0 ? frames - 1 : 0);
  const double sdue = L.stop_due;
  double end_b = pb + block_beats;
  if (sdue >= 0.0 && sdue < end_b) end_b = sdue;

  // LA TETE A SAUTE : « pars d'ici » depuis l'editeur, un deplacement du
  // transport, un changement d'horloge. La voix tient une position en frames
  // et finirait sa passe a cote : on la croise vers la nouvelle phase.
  const double pos0 = pb + off;
  if (L.au_run && std::fabs(pos0 - L.au_expect) > kAuJumpBeats) au_cut(li);
  L.au_expect = pos0 + block_beats;

  // LA VITESSE DE LECTURE A BOUGE pendant que ca sonne : la voix suit, sans
  // repartir du debut. Les passes suivantes la prennent a leur lancement.
  if (L.au_run && L.au_prate != au_prate_) {
    au_post(A.port, kCmdVoiceSet, A.v[L.au_cur], kNow, A.rate * au_prate_, 0.0,
            kParamRate, 0);
    L.au_prate = au_prate_;
  }

  // L'ENTREE. Un depart neuf part a SON beat ; une lane qui jouait sans son
  // (demute, rappel de session, transport reparti) entre maintenant, a sa
  // phase. Une passe qui n'a plus que quelques centiemes de beat a vivre
  // attend la suivante plutot que de sonner un eclat.
  if (!L.au_run) {
    double sb = (fresh > kWaitTest && fresh > pb) ? fresh : pb;
    const double pos = sb + off;
    const double ph = pos - std::floor(pos / Ls) * Ls;
    if (Ls - ph <= 0.01 && Ls - ph > 1e-7) sb += Ls - ph;
    if (sb < end_b) au_start(li, sb, pb, at, last, bpf, block_beats, sa, Ls, off, false);
  }

  // LES FRONTIERES DE PASSE du bloc. Deux au plus : une zone plus courte que
  // la moitie d'un bloc n'a que deux voix pour se relayer, et la porte MIDI
  // est deja degeneree a cette echelle (voir `fine` dans run_gate).
  for (int guard = 0; guard < 2 && L.au_run; ++guard) {
    const double k  = std::floor(L.au_last / Ls + 1e-7) + 1.0;
    const double nb = k * Ls - off;
    if (!(nb < end_b - 1e-9)) break;
    au_start(li, (nb < pb) ? pb : nb, pb, at, last, bpf, block_beats, sa, Ls,
             off, true);
  }

  // L'ARRET A SA FRONTIERE (Lane::stop_due) : la passe en cours descend apres
  // elle, pendant que la case qui lui succede attaque.
  if (sdue >= 0.0 && sdue < pb + block_beats && L.au_run) {
    const frame_t sf = hit_frame(sdue - pb, bpf, block_beats, at, last);
    const frame_t ff = (frame_t)(kAuFadeS * srate_ + 0.5);
    au_post(A.port, kCmdVoiceStop, A.v[L.au_cur], sf + ff, kAuFadeS, 0.0, 0, 0);
    L.au_run = 0;
  }
  L.au_pub.store(L.au_run ? L.au_cur : -1, std::memory_order_relaxed);
}

// ---------------------------------------------------------------------------
// LE TIRAGE D'UNE NOTE — sans etat, et c'est ce qui le rend possible ici
// ---------------------------------------------------------------------------
// Une note tiree au sort doit garder sa decision PENDANT TOUTE LA PASSE, sinon
// elle s'allume et s'eteint en cours de note. Ca semble demander une memoire
// par note et par passe — c'est-a-dire de l'etat, dans le fil audio, remis a
// zero par un evenement que ce fil ne connait pas. Ca n'est pas necessaire.
//
// Le tirage ne depend que de (numero de note, numero de passe). Il est donc
// CONSTANT pendant toute la passe et IDENTIQUE pour l'attaque et pour la
// coupure — ce qui est exactement la propriete demandee, obtenue en zero octet
// et sans une seule ecriture.
//
// LE NUMERO DE PASSE SE PREND SUR L'ATTAQUE, PAS SUR L'INSTANT COURANT. Une
// note qui traverse la frontiere de boucle est encore dans la passe ou elle a
// commence ; la dater sur maintenant lui ferait retirer au sort a mi-note, et
// la coupure d'une note qui n'a jamais sonne serait laissee en l'air.
//
// LA GRAINE PAR LANE empeche deux lanes portant les memes notes de tirer a
// l'identique — ce qui s'entendrait tout de suite comme un artefact et non
// comme du hasard. Elle se DEDUIT de l'indice de lane : rien a stocker, rien a
// serialiser, et le harnais peut donc prouver la distribution.
//
// C'est un mixage entier a la Wang/Jenkins : quelques operations sur registres,
// pas de division, pas de table.
static inline unsigned lane_seed(int li) {
  return (unsigned)li * 2654435761u + 0x9E3779B9u;
}

static inline unsigned note_hash(unsigned k, unsigned pass_seed) {
  unsigned h = k * 0x85EBCA6Bu + pass_seed;
  h ^= h >> 15;
  h *= 0xC2B2AE35u;
  h ^= h >> 13;
  h *= 0x27D4EB2Fu;
  h ^= h >> 16;
  return h;
}

void Lanes::run_gate(double pb, bool active, double ts_num, frame_t at,
                     int frames, double block_beats) {
  if (!active) return;
  const double bpf = (frames > 0) ? (block_beats / (double)frames) : 0.0;

  for (int li = 0; li < kMaxLanes; ++li) {
    Lane& L = lanes_[li];
    // La case audio d'abord, et pour TOUTE lane : celle qui vient de s'arreter
    // doit taire sa voix, et elle ne passerait pas le filtre ci-dessous.
    run_audio(li, pb, ts_num, at, frames, block_beats);
    const int m = L.mode.load(std::memory_order_relaxed);
    if (m != kLanePlaying && m != kLaneOverdub) continue;
    // LE OU DES DEUX INTENTIONS, et le moteur n'a pas a savoir laquelle parle.
    const double sdue = L.stop_due;
    if (L.muted.load(std::memory_order_relaxed)
        || L.user_muted.load(std::memory_order_relaxed)) {
      flush_lane(li, at);
      if (sdue >= 0.0) {
        L.mode.store(kLaneStopped, std::memory_order_relaxed);
        L.stop_due = -1.0;
      }
      continue;
    }

    const int port = L.port.load(std::memory_order_relaxed);
    if (port < 0) {
      if (sdue >= 0.0) {
        L.mode.store(kLaneStopped, std::memory_order_relaxed);
        L.stop_due = -1.0;
      }
      continue;
    }
    const int ch = L.channel.load(std::memory_order_relaxed) & 0x0F;

    // LA ZONE DE LECTURE REMPLACE LA LONGUEUR PARTOUT DANS CETTE BOUCLE.
    // Sans accolade elle vaut (0, longueur de la case) et pas un calcul ne
    // change : c'est ce qui rend la fonctionnalite gratuite pour les lanes qui
    // ne s'en servent pas — c'est-a-dire presque toutes, presque toujours.
    double sa, Ls;
    lane_span(li, ts_num, &sa, &Ls);

    // LA PHASE DE REFERENCE EST CELLE DE LA FIN DU BLOC, pas de son debut.
    //
    // Le JSFX lisait la phase du debut et posait tout a l'offset zero : une
    // note tombant au tiers du bloc partait au debut de celui-ci, soit jusqu'a
    // une taille de tampon trop tot. En reconciliant sur la fin du bloc, chaque
    // transition est CONTENUE dans le bloc a venir, et on peut donc la dater a
    // l'echantillon au lieu de l'arrondir a sa frontiere.
    //
    // Degenerescence : un tampon plus long que la boucle elle-meme (une boucle
    // d'un demi-temps a 1024 echantillons) sauterait des notes entieres. Dans
    // ce cas seul on revient a la phase du debut, ce qui est exactement le
    // comportement du JSFX — moins juste, jamais faux.
    const bool  fine = (block_beats < Ls * 0.5);
    const double off  = L.phase_off.load(std::memory_order_relaxed);
    // Un arret dans ce bloc : on reconcilie JUSTE AVANT sa frontiere, pour
    // que rien de la passe suivante ne s'y attaque.
    double end_b = pb + block_beats;
    if (sdue >= 0.0 && sdue - 1e-7 < end_b) end_b = sdue - 1e-7;
    if (end_b < pb) end_b = pb;
    const double pref = (fine ? end_b : pb) + off;
    // LA LIGNE DE TEMPS SUR LAQUELLE LA PHASE COURT — celle du beat du projet
    // DECALEE du decalage de la lane.
    //
    // `phase_hit` repond a « quel est le prochain beat dont la phase vaut
    // ceci ». Lui donner `pb` nu revenait a poser la question sur une ligne qui
    // n'est pas celle de la lane des qu'un « pars d'ici » etait arme : la date
    // sous-bloc de chaque note tombait alors a cote, et comme elle est bornee
    // au bloc, la note partait a son debut ou a sa fin. Une gigue d'un bloc,
    // visible seulement avec un decalage, donc jamais reproduite.
    const double pbo = pb + off;

    const int  nb  = L.nbuf.load(std::memory_order_acquire);
    const int  nev = L.ncount[nb].load(std::memory_order_relaxed);
    const LaneNote* notes = L.buf[nb];

    // Ce que la phase demande. 128 entrees sur la pile : aucune allocation, et
    // la table tient dans deux lignes de cache.
    int16_t       desire[128];
    unsigned char dvel[128];
    // ET DE QUELLE PASSE, sur huit bits — voir Lane::spass.
    unsigned char dpass[128];
    // ET DE QUEL DEBUT — l'identite de la note, voir Lane::sstart.
    float         dstart[128];
    // Depuis combien de beats elle sonne : quand deux notes d'une meme hauteur
    // couvrent la phase, la plus RECENTE gagne. C'etait « la derniere de la
    // liste », donc encore un rang, que la prochaine insertion pouvait changer.
    double        dage[128];
    for (int p = 0; p < 128; ++p) {
      desire[p] = 0; dvel[p] = 0; dpass[p] = 0; dstart[p] = 0.0f; dage[p] = 1e300;
    }

    const double pr = pref - std::floor(pref / Ls) * Ls;
    const unsigned seed = lane_seed(li);
    for (int k = 0; k < nev; ++k) {
      const LaneNote& n = notes[k];
      // Une note qui COMMENCE hors de la zone de lecture est hors du clip :
      // elle reste stockee (raccourcir une boucle, ou poser une accolade, ne
      // doit jamais detruire ce qu'on a ecrit — et rouvrir ramene la musique)
      // mais elle ne sonne pas. Le repli ci-dessous n'existe que pour qu'une
      // note qui TRAVERSE la frontiere garde sa queue ; l'appliquer a un debut
      // hors bornes repliait les mesures 2, 3 et 4 sur la premiere.
      const double ns = (double)n.start - sa;
      if (ns < 0.0 || ns >= Ls) continue;
      double d = pr - ns;
      d -= std::floor(d / Ls) * Ls;
      if (d < (double)n.len) {
        // LA PASSE DE L'ATTAQUE. `pref - d` est l'instant ou cette note a
        // commence : constant tant qu'elle sonne, y compris quand elle traverse
        // la frontiere de boucle. Elle sert DEUX FOIS — au tirage juste en
        // dessous, et a l'identite du desir — et c'est la meme reponse.
        // ⚠️ UNE NOTE AUSSI LONGUE QUE LA ZONE NE TIRE PAS, ET C'EST MESURE.
        //
        // `d` est ramene dans [0, Ls) : pour une note dont la longueur atteint
        // la zone, il ne represente plus « depuis quand elle sonne » mais
        // « depuis la derniere frontiere ». `onset` avance donc d'un Ls a chaque
        // tour alors que la note n'a pas re-attaque, et le tirage est refait au
        // milieu d'elle — elle se fait HACHER a chaque frontiere au lieu de
        // jouer ou de se taire. Le harnais l'a compte : 47 attaques pour
        // 46 coupures sur une note tenue a 50 %.
        //
        // Une note qui couvre toute la boucle ne re-attaque jamais : il n'y a
        // pas de passe pour laquelle se taire, donc elle joue. C'est la seule
        // lecture qui ne produise pas un artefact.
        const double onset = pref - d;
        const long long pass = (long long)std::floor(onset / Ls);
        if (n.prob < 100 && (double)n.len < Ls) {
          const unsigned h = note_hash((unsigned)k,
                                       (unsigned)((unsigned long long)pass) ^ seed);
          if ((h % 100u) >= (unsigned)n.prob) continue;
        }
        const int p = n.pitch & 0x7F;
        if (d >= dage[p]) continue;
        dage[p]   = d;
        desire[p] = (int16_t)(k + 1);
        dvel[p]   = n.vel;
        dpass[p]  = (unsigned char)((unsigned long long)pass & 0xFFull);
        dstart[p] = n.start;
      }
    }

    const frame_t last = at + (frame_t)(frames > 0 ? frames - 1 : 0);

    // ⚠️ TOUTES LES COUPURES AVANT TOUTES LES ATTAQUES, ET C'EST POUR CA QU'IL Y
    // A DEUX BOUCLES.
    //
    // Une seule boucle emettait, POUR CHAQUE HAUTEUR, sa coupure puis son
    // attaque, en parcourant les hauteurs de 0 a 127. Au point de boucle, la
    // note qui finit le clip et celle qui le commence ne sont pas sur la meme
    // hauteur : elles tombent donc dans deux tours differents, et l'ordre des
    // deux messages devient celui des NUMEROS DE NOTE. Si la note entrante est
    // plus grave que la sortante, son Note-On part AVANT le Note-Off de l'autre,
    // au meme echantillon — et tout instrument mono, legato, ou qui traite un
    // note-off orphelin comme « coupe la voix courante » mange la note qui vient
    // de naitre.
    //
    // Le symptome est donc « la note de debut ne se lance pas », et il DEPEND DE
    // L'INTERVALLE : transposer la phrase le fait disparaitre, ce qui le fait
    // passer pour aleatoire. C'est ce que fait tout sequenceur au point de
    // boucle, et c'est la seule facon d'en etre independant.
    //
    // C'est la variante a DEUX notes du mal que `spass` a deja soigne : lui
    // regle la note qui remplit sa passe et se remplace ELLE-MEME, pas deux
    // notes distinctes qui se croisent. `test_lane_two_notes_cross_the_loop`
    // couvre celle-ci.
    //
    // 128 dates de plus sur la pile — aucune allocation, comme les trois tables
    // au-dessus. Elles servent a la SECONDE passe : bornee a `off_frame[p]`,
