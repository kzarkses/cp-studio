#include "cp_engine.h"

#include <cmath>
#include <cstring>
#include <limits>

namespace cp {

Engine::Engine()
    : env_store_(new EnvPts[kMaxVoices]),
      alloc_cursor_(0), srate_(48000.0), clock_(0), blocks_(0), dropped_(0),
      last_tick_(0), clock_external_(false), clock_master_(-1),
      last_master_frames_(0) {
  std::memset(env_store_, 0, sizeof(EnvPts) * kMaxVoices);
  for (int i = 0; i < kMaxVoices; ++i) {
    // Le pointeur d'abord : reset() ne le touche jamais, c'est le seul endroit
    // ou il se pose.
    voices_[i].envp = &env_store_[i];
    voices_[i].reset();
    voices_[i].gen = 1;
    claim_[i].store((claim_t)1, std::memory_order_relaxed); // gen 1, libre
  }
}

Engine::~Engine() {
  delete[] env_store_;
}

void Engine::init(double srate) {
  srate_ = (srate > 1.0) ? srate : 48000.0;
  lanes_.init(srate_);
  clock_.store(0, std::memory_order_release);
  blocks_.store(0, std::memory_order_release);
  dropped_.store(0, std::memory_order_relaxed);
  clock_external_ = false;
  clock_master_ = -1;
  last_master_frames_ = 0;
  alloc_cursor_ = 0;
  for (int i = 0; i < kMaxVoices; ++i) {
    voices_[i].reset();
    voices_[i].gen = 1;
    claim_[i].store((claim_t)1, std::memory_order_relaxed);
  }
  for (int p = 0; p < kMaxPorts; ++p) {
    ports_[p].cmds.clear();
    ports_[p].vcount = 0;
    ports_[p].gain = ports_[p].gain_target = 1.0f;
    ports_[p].last_clock = -1;
    ports_[p].consumed = 0;
    ports_[p].used = false;
  }
}

void Engine::set_srate(double srate) {
  lanes_.set_srate(srate);
  // Le taux est une ENTREE PAR BLOC, pas une constante d'initialisation : REAPER
  // peut changer de peripherique sous nos pieds. Les longueurs deja calculees en
  // frames (fondus) restent valides en frames, ce qui est le comportement
  // attendu — un fondu de 10 ms devient 10 ms au nouveau taux au prochain calcul.
  if (srate > 1.0) srate_ = srate;
}

// ---------------------------------------------------------------------------
// Fil principal
//
// Aucune de ces fonctions n'ecrit dans une structure Voice. Elles manipulent le
// mot de propriete — un entier atomique — et postent une commande. C'est la
// difference entre « benin sur x86 » et « correct ».
// ---------------------------------------------------------------------------

voice_h Engine::voice_alloc(int port) {
  if (port < 0 || port >= kMaxPorts) return kNullVoice;

  // Plafond par port. Ce n'est pas une politique : c'est ce qui garantit que la
  // liste du fil audio ne peut pas deborder. On compte les emplacements
  // POSSEDES, dont l'ensemble contient toujours ceux que le fil audio a deja
  // pris en charge.
  int here = 0;
  for (int i = 0; i < kMaxVoices; ++i) {
    const claim_t c = claim_[i].load(std::memory_order_relaxed);
    if (claim_owned(c) && claim_port(c) == port) ++here;
  }
  if (here >= kMaxPortVoices) return kNullVoice;

  for (int k = 0; k < kMaxVoices; ++k) {
    const int i = (alloc_cursor_ + k) & (kMaxVoices - 1);
    const claim_t cur = claim_[i].load(std::memory_order_acquire);
    if (claim_owned(cur)) continue;   // encore entre les mains du fil audio

    uint16_t gen = (uint16_t)(claim_gen(cur) + 1);
    if (gen == 0) gen = 1;            // 0 est reserve a « vierge »

    // On PREND d'abord, on demande ensuite. Dans cet ordre, un emplacement ne
    // peut jamais etre pris deux fois, meme si la commande se perd.
    claim_[i].store(make_claim(gen, port), std::memory_order_release);

    Cmd c;
    std::memset(&c, 0, sizeof(c));
    c.type = kCmdVoiceAlloc;
    c.voice = make_handle(i, gen);
    c.at = kNow;
    c.u1 = (uint32_t)port;
    if (!post(port, c)) {
      // Anneau plein : on rend l'emplacement tout de suite. Le fil audio ne l'a
      // jamais vu — il n'entre dans sa liste que par cette commande.
      claim_[i].store((claim_t)gen, std::memory_order_release);
      return kNullVoice;
    }

    ports_[port].used = true;
    alloc_cursor_ = i + 1;
    return make_handle(i, gen);
  }
  return kNullVoice;
}

bool Engine::voice_valid(voice_h h) const {
  if (h == kNullVoice) return false;
  const int i = handle_index(h);
  if (i < 0 || i >= kMaxVoices) return false;
  const claim_t c = claim_[i].load(std::memory_order_acquire);
  return claim_owned(c) && claim_gen(c) == handle_gen(h);
}

frame_t Engine::voice_started_at(voice_h h) const {
  if (!voice_valid(h)) return -1;
  return (frame_t)voices_[handle_index(h)].pub_started.load(std::memory_order_relaxed);
}

int Engine::voice_port(voice_h h) const {
  if (h == kNullVoice) return -1;
  const int i = handle_index(h);
  if (i < 0 || i >= kMaxVoices) return -1;
  const claim_t c = claim_[i].load(std::memory_order_acquire);
  if (!claim_owned(c) || claim_gen(c) != handle_gen(h)) return -1;
  return claim_port(c);
}

void Engine::voice_release(voice_h h) {
  if (!voice_valid(h)) return;
  const int i = handle_index(h);
  const claim_t cur = claim_[i].load(std::memory_order_relaxed);
  const int port = claim_port(cur);

  uint16_t ngen = (uint16_t)(claim_gen(cur) + 1);
  if (ngen == 0) ngen = 1;

  // La generation avance ICI : toute commande ulterieure portant l'ancien handle
  // est refusee des la frontiere, sans meme traverser l'anneau. Sans ca, une
  // commande envoyee sur un handle deja libere atteint la voix — bug trouve par
  // le harnais. Dans ce depot les scripts meurent et redemarrent en permanence :
  // c'est un scenario reel, pas une hypothese.
  //
  // L'emplacement reste POSSEDE. Seul le fil audio peut le rendre, et seulement
  // quand il a fini de l'eteindre : c'est ce qui interdit qu'il soit realloue
  // pendant qu'il sonne encore.
  claim_[i].store(make_claim(ngen, port), std::memory_order_release);

  Cmd c;
  std::memset(&c, 0, sizeof(c));
  c.type = kCmdVoiceFree;
  c.voice = h;             // l'ANCIEN handle : c'est celui que la voix reconnait
  c.at = kNow;
  c.u0 = (uint32_t)ngen;   // la generation a installer cote fil audio
  c.a = 0.005;             // 5 ms, pas de clic
  post(port, c);
  // Si l'anneau etait plein, l'emplacement reste possede jusqu'au prochain
  // port_reset. C'est une fuite bornee, comptee par dropped_, et jamais un son
  // qui continue par surprise.
}

bool Engine::post(int port, const Cmd& c) {
  if (port < 0 || port >= kMaxPorts) return false;
  if (!ports_[port].cmds.push(c)) {
    dropped_.fetch_add(1, std::memory_order_relaxed);
    return false;
  }
  return true;
}

bool Engine::voice_query(voice_h h, double* pos_frames, int* state) const {
  if (h == kNullVoice) return false;
  const int i = handle_index(h);
  if (i < 0 || i >= kMaxVoices) return false;
  const claim_t c = claim_[i].load(std::memory_order_acquire);
  if (!claim_owned(c) || claim_gen(c) != handle_gen(h)) return false;
  if (pos_frames) *pos_frames = voices_[i].pub_pos.load(std::memory_order_relaxed);
  if (state) *state = voices_[i].pub_state.load(std::memory_order_relaxed);
  return true;
}

void Engine::panic() {
  for (int p = 0; p < kMaxPorts; ++p) {
    if (!ports_[p].used) continue;
    Cmd c;
    std::memset(&c, 0, sizeof(c));
    c.type = kCmdPanic;
    c.at = kNow;
    post(p, c);
  }
}

int Engine::active_voices() const {
  int n = 0;
  for (int i = 0; i < kMaxVoices; ++i)
    if (voices_[i].pub_state.load(std::memory_order_relaxed) != kVoiceIdle) ++n;
  return n;
}

// LE COMPTAGE SE DEDUIT, IL NE SE TIENT PAS. Faire incrementer le fil audio au
// demarrage d'une voix et decrementer a sa mort aurait demande quatre chemins
// sans faute — demarrage, arret, vol de voix, enchainement — et c'est
// exactement le genre de comptabilite qui finit par mentir une fois. Un
// balayage de 64 voix, au moment ou on veut liberer, repond a la meme question
// sans etat a maintenir.
void Engine::refresh_clip_refs() {
  pool_.clear_refs();
  for (int i = 0; i < kMaxVoices; ++i) {
    if (voices_[i].pub_state.load(std::memory_order_relaxed) == kVoiceIdle) continue;
    pool_.add_ref(voices_[i].pub_clip.load(std::memory_order_relaxed));
  }
}

int Engine::owned_voices() const {
  int n = 0;
  for (int i = 0; i < kMaxVoices; ++i)
    if (claim_owned(claim_[i].load(std::memory_order_relaxed))) ++n;
  return n;
}

int Engine::port_voices(int port) const {
  if (port < 0 || port >= kMaxPorts) return 0;
  int n = 0;
  for (int i = 0; i < kMaxVoices; ++i) {
    const claim_t c = claim_[i].load(std::memory_order_relaxed);
    if (claim_owned(c) && claim_port(c) == port) ++n;
  }
  return n;
}

// ---------------------------------------------------------------------------
// Fil audio
// ---------------------------------------------------------------------------

void Engine::tick(int frames) {
  if (frames <= 0) return;
  clock_external_ = true;
  last_tick_.store(frames, std::memory_order_relaxed);
  const frame_t before = clock_.fetch_add((frame_t)frames, std::memory_order_acq_rel);
  blocks_.fetch_add(1, std::memory_order_acq_rel);

  // LES LANES TRAVAILLENT SUR LE BLOC A VENIR, PAS SUR CELUI QUI VIENT DE
  // PASSER. `before + frames` est le frame du prochain echantillon a produire,
  // donc le debut du bloc que PortSource servira ensuite : les evenements y
  // sont dates a l'avance et tombent a l'offset exact au lieu d'arriver en
  // retard d'un tampon. C'est la meme lecon que le tick lui-meme — appeler
  // avant etiquetterait chaque bloc avec l'heure de sa fin.
  lanes_.tick(before + (frame_t)frames, frames);
}

void Engine::port_reset(int port) {
  if (port < 0 || port >= kMaxPorts) return;
  PortState& ps = ports_[port];
  ps.cmds.clear();
  ps.vcount = 0;
  ps.last_clock = -1;
  ps.consumed = 0;
  ps.gain = ps.gain_target = 1.0f;
  ps.used = false;
  for (int i = 0; i < kMaxVoices; ++i) {
    const claim_t cur = claim_[i].load(std::memory_order_relaxed);
    if (!claim_owned(cur) || claim_port(cur) != port) continue;
    uint16_t g = (uint16_t)(claim_gen(cur) + 1);
    if (g == 0) g = 1;
    voices_[i].reset();
    voices_[i].gen = g;
    claim_[i].store((claim_t)g, std::memory_order_release);   // libre
  }
}

void Engine::port_attach_voice(int port, int index) {
  PortState& ps = ports_[port];
  for (int k = 0; k < ps.vcount; ++k)
    if (ps.vlist[k] == (int16_t)index) return;       // deja la
  if (ps.vcount >= kMaxPortVoices) return;           // borne dure, jamais atteinte
  ps.vlist[ps.vcount++] = (int16_t)index;
}

// ---------------------------------------------------------------------------
// UNE VOIX PROGRAMMEE S'ANNULE, ELLE NE S'ETEINT PAS.
//
// Toute coupure « maintenant » passait la voix en kVoiceStopping quel que soit
// son etat. Une voix encore kVoiceScheduled — une passe armee pour la frontiere
// qui vient — perdait alors sa branche de depart dans render(), et rendait CINQ
// MILLISECONDES DU DEBUT DE SA MATIERE EN FONDU DE SORTIE : un bip, a chaque
// stop du transport, chaque mute, chaque coup de reglette de vitesse. Le
// commentaire de Cells.lua affirmait « une passe pas encore audible s'annule
// sans bruit » ; c'etait l'intention, pas le code. Les trois chemins qui
// coupent (Stop, Free, Panic) passent par ici.
// ---------------------------------------------------------------------------
static inline void stop_now(Voice& v, int fade_frames) {
  if (v.state == kVoiceScheduled) {
    v.state = kVoiceIdle;
    v.clip = -1;
    v.tail_len = 0;
    return;
  }
  // DEJA EN FONDU : on garde celui qui court. Remettre `fade_out_pos` a zero
  // faisait remonter l'enveloppe d'un coup — de 0,3 a 1,0 — pour redescendre
  // ensuite : un clic a l'endroit precis ou on voulait l'eviter.
  if (v.state == kVoiceStopping) return;
  if (v.state != kVoicePlaying) return;
  v.state = kVoiceStopping;
  v.stop_at = (std::numeric_limits<frame_t>::max)();
  v.fade_out_len = fade_frames;
  v.fade_out_pos = 0;
}

static inline double clamp1(double c) { return c < -1.0 ? -1.0 : (c > 1.0 ? 1.0 : c); }

// Le croisement d'un saut de tete : 5 ms, la meme longueur que les coupures.
int Engine::jump_xfade_frames() const { return (int)(0.005 * srate_); }

void Engine::apply(int port, const Cmd& c, frame_t block_start) {
  if (c.type == kCmdPanic) {
    PortState& ps = ports_[port];
    for (int k = 0; k < ps.vcount; ++k) {
      Voice& pv = voices_[ps.vlist[k]];
      if (pv.state == kVoiceIdle) continue;
      stop_now(pv, (int)(0.005 * srate_));
    }
    return;
  }

  if (c.type == kCmdPortGain) {
    const int p = (int)c.u1;
    if (p >= 0 && p < kMaxPorts) ports_[p].gain_target = (float)c.a;
    return;
  }

  const int i = handle_index((voice_h)c.voice);
  if (i < 0 || i >= kMaxVoices) return;
  Voice& v = voices_[i];

  if (c.type == kCmdVoiceAlloc) {
    // La prise de possession. Rien a verifier contre l'ancienne generation :
    // le mot de propriete a deja tranche cote fil principal, et cette commande
    // EST le moment ou l'emplacement change de main. C'est aussi le seul endroit
    // ou une voix est remise a zero pendant que le moteur tourne.
    v.reset();
    v.port = port;
    v.gen = handle_gen((voice_h)c.voice);
    v.publish();
    port_attach_voice(port, i);
    return;
  }

  if (v.gen != handle_gen((voice_h)c.voice)) return; // handle perime : on ignore

  if (c.type == kCmdVoiceFree) {
    // La voix change de generation immediatement : toute commande deja en vol
    // portant l'ancien handle sera refusee juste au-dessus.
    v.gen = (uint16_t)c.u0;
    v.free_pending = true;
    v.next_voice = -1;
    stop_now(v, (int)(c.a * srate_));
    // L'emplacement sera rendu au fil principal quand la voix sera eteinte —
    // voir la phase 3 de render_port. Jamais avant.
    return;
  }

  const frame_t at = (c.at == kNow) ? block_start : c.at;

  switch (c.type) {
    case kCmdVoicePlay: {
      // UN LANCEMENT SUR UNE VOIX QUI SONNE ENCORE N'EST PAS UNE COUPURE SECHE.
      //
      // C'etait un redemarrage brut : la sortie passait de la matiere en cours
      // au nouveau depart sans transition. Meme clip (un navigateur qui relance
      // le fichier qu'on ecoute) : l'ancienne tete croise avec la nouvelle.
      // Autre clip : l'ancienne matiere n'est plus lisible par cette voix, donc
      // on impose au moins un fondu d'entree a la nouvelle — la coupure de
      // l'ancienne reste, et c'est pour ca qu'un appelant qui enchaine des
      // fichiers doit alterner DEUX voix (Audition le fait).
      const bool live = (v.state == kVoicePlaying || v.state == kVoiceStopping);
      const int  nclip = (int)c.u0;
      const int  xf = jump_xfade_frames();
      if (live && nclip == v.clip) {
        v.tail_pos = v.pos;
        v.tail_len = xf;
        v.tail_i = 0;
        v.fade_in_cur = v.fade_in_len;
      } else {
        v.tail_len = 0;
        v.fade_in_cur = (live && v.fade_in_len < xf) ? xf : v.fade_in_len;
      }
      v.clip = nclip;
      v.mode = (int)c.u1;
      v.rate = (c.a > 0.0) ? c.a : 1.0;
      v.gain = v.gain_target = (float)c.b;
      v.pos = (double)v.loop_start;
      v.start_at = at;
      v.stop_at = (std::numeric_limits<frame_t>::max)();
      v.ended_at = -1;
      // L'instant de depart appartient a CE lancement. Il n'etait remis a -1
      // qu'a l'allocation : une voix reutilisee publiait le depart du
      // lancement d'AVANT jusqu'a ce que le nouveau sonne, et quiconque
      // demandait « a-t-elle demarre » obtenait un oui perime.
      v.started_at = -1;
      v.fade_in_pos = 0;
      v.fade_out_pos = 0;
      // Un lancement repart de l'attaque. Les reglages de l'enveloppe restent
      // (ce sont des proprietes), seul son ETAT revient au debut.
      v.env_st = 0;
      v.env_x = 0.0;
      v.env_lv = 0.0f;
      v.envp_i = 0;
      v.state = kVoiceScheduled;
      break;
    }
    case kCmdVoiceEnvPt: {
      // Le tableau EN ATTENTE seulement : rien ne s'entend avant la validation.
      float cv;
      std::memcpy(&cv, &c.u1, sizeof(cv));
      v.env_point((int)c.u0, c.a, c.b, (double)cv);
      break;
    }
    case kCmdVoiceStop: {
      const int fade = (int)(c.a * srate_);
      if (c.at == kNow && fade > 0) {
        // Arret immediat en douceur — ou annulation, si rien n'a encore sonne.
        stop_now(v, fade);
      } else {
        // Arret DATE : c'est le cas musical (fin de boucle, frontiere de
        // mesure). Il doit tomber exactement sur le frame demande. Une voix
        // programmee APRES sa propre coupure n'a rien a jouer : annulee.
        if (v.state == kVoiceScheduled && at <= v.start_at) {
          v.state = kVoiceIdle;
          v.clip = -1;
          break;
        }
        v.stop_at = at;
        v.fade_out_len = fade;
      }
      break;
    }
    case kCmdVoiceSet: {
      switch (c.u0) {
        case kParamRate:      v.rate = (c.a > 0.0) ? c.a : v.rate; break;
        case kParamGain:      v.gain_target = (float)c.a; break;
        case kParamPan:       v.pan = (float)c.a; break;
        case kParamLoopStart: v.loop_start = (frame_t)c.a; break;
        case kParamLoopEnd:   v.loop_end = (frame_t)c.a; break;
        case kParamFadeIn: {
          v.fade_in_len = (int)(c.a * srate_);
          // Un fondu pose sur une voix qui n'a pas encore demarre vaut pour
          // ce lancement-la aussi — sinon il ne compterait qu'au suivant.
          if (v.state == kVoiceScheduled) v.fade_in_cur = v.fade_in_len;
          break;
        }
        case kParamFadeOut:   v.fade_out_len = (int)(c.a * srate_); break;
        // Deplacement de la tete de lecture. Postee APRES un play, elle en
        // devient le point de depart : l'anneau est FIFO, donc l'ordre
        // d'ecriture est l'ordre d'application, dans le meme bloc. Sur une
        // voix qui SONNE, c'est un saut : l'ancienne tete croise avec la
        // nouvelle pendant 5 ms au lieu de disparaitre d'un coup.
        case kParamPos: {
          const double np = (c.a > 0.0) ? c.a : 0.0;
          if (v.state == kVoicePlaying || v.state == kVoiceStopping) {
            v.tail_pos = v.pos;
            v.tail_len = jump_xfade_frames();
            v.tail_i = 0;
          }
          v.pos = np;
          break;
        }
        // La boucle se change A LA VOLEE : un aperçu qu'on met en boucle
        // pendant qu'il sonne ne doit pas repartir du debut.
        case kParamLoop:      v.mode = (c.a != 0.0) ? kPlayLoop : kPlayOnce; break;
        // L'ENVELOPPE. Les courbes arrivent en -1..1, comme celles du pad, et
        // deviennent ici le `k` du JSFX : 8^(-c) pour l'attaque (qui monte),
        // 8^(c) pour ce qui descend. Un pow par reglage, jamais par echantillon.
        // `env` 1 = l'ADSR, 0 = rien : dans les deux cas les points se taisent
        // (les deux enveloppes sont exclusives).
        case kParamEnv:       v.env_on = (c.a != 0.0); v.envp_on = false; break;
        // Valider les points (ABI 3.3). Un lot refuse laisse l'enveloppe
        // d'avant en place : voir Voice::env_commit.
        case kParamEnvPts:    v.env_commit((int)(c.a + 0.5)); break;
        case kParamAttack:    v.env_a = (float)(c.a > 0.0 ? c.a : 0.0); break;
        case kParamDecay:     v.env_d = (float)(c.a > 0.0 ? c.a : 0.0); break;
        case kParamSustain:
          v.env_s = (float)(c.a < 0.0 ? 0.0 : (c.a > 1.0 ? 1.0 : c.a));
          break;
        case kParamRelease:   v.env_r = (float)(c.a > 0.0 ? c.a : 0.0); break;
        case kParamACurve:    v.env_ka = (float)std::pow(8.0, -clamp1(c.a)); break;
        case kParamDCurve:    v.env_kd = (float)std::pow(8.0,  clamp1(c.a)); break;
        case kParamRCurve:    v.env_kr = (float)std::pow(8.0,  clamp1(c.a)); break;
        // RELACHER : le relache part DU NIVEAU COURANT et dure niveau x R --
        // la pente d'un relache de R secondes, comme le JSFX. Une voix pas
        // encore partie n'a rien a relacher : elle s'annule. Sans enveloppe, un
        // relache n'est qu'un arret doux de 5 ms.
        case kParamGate: {
          if (c.a != 0.0) break;
          if (!v.env_on) { stop_now(v, (int)(0.005 * srate_)); break; }
          if (v.state == kVoiceScheduled) { stop_now(v, 0); break; }
          if (v.state != kVoicePlaying || v.env_st == 3) break;
          v.env_st = 3;
          v.env_rl0 = v.env_lv;
          v.env_x = 0.0;
          const double rl = (double)v.env_rl0 * v.env_r * srate_;
          v.env_rxi = 1.0 / ((rl > 1.0) ? rl : 1.0);
          break;
        }
        default: break;
      }
      break;
    }
    case kCmdVoiceQueue: {
      const voice_h nh = (voice_h)c.u0;
      const int ni = handle_index(nh);
      if (ni >= 0 && ni < kMaxVoices && voices_[ni].gen == handle_gen(nh)) {
        v.next_voice = ni;
        v.xfade_len = (int)(c.a * srate_);
      }
      break;
    }
    default: break;
  }
}

void Engine::drain(int port, frame_t block_start) {
  Cmd c;
  // Borne dure : on ne draine jamais indefiniment dans le fil audio. 256 par
  // bloc a 64 echantillons = 192 000 commandes/s, trois ordres de grandeur
  // au-dessus de ce qu'une interface peut produire.
  int guard = kCmdRingCap;
  while (guard-- > 0 && ports_[port].cmds.pop(c)) apply(port, c, block_start);
  // PUIS CE QUE LES LANES ONT DECIDE pour les cases audio de ce port (ABI
  // 3.0). Apres le fil principal : un reglage de voix pose par Lua (une
  // enveloppe, un gain) vaut donc deja pour la passe que la lane lance dans ce
  // bloc.
  // ⚠ Seulement sur une voix DE CE PORT : la liaison vient de Lua, et une
  // voix d'un autre port est rendue par un autre appel, peut-etre sur un autre
  // fil. L'y toucher d'ici serait une course, pas une erreur de routage.
  guard = 64;
  while (guard-- > 0 && lanes_.pop_vcmd(port, c)) {
    const int i = handle_index((voice_h)c.voice);
    if (i < 0 || i >= kMaxVoices || voices_[i].port != port) continue;
    apply(port, c, block_start);
  }
}

frame_t Engine::render_port(int port, sample_t* out, int frames, int nch) {
  if (port < 0 || port >= kMaxPorts || !out || frames <= 0 || nch < 1) return -1;
  PortState& ps = ports_[port];
  ps.used = true;

  // --- ou sommes-nous ? -----------------------------------------------------
  // clock_ est le nombre d'echantillons DEJA delivres. Le premier echantillon
  // de ce bloc porte donc l'index clock_ + ce que ce port a deja consomme dans
  // le bloc courant. Aucun arrondi : c'est ici que se gagne l'exactitude a
  // l'echantillon, et c'est ici que le harnais l'a prise en defaut.
  if (!clock_external_) {
    // Aucun hook materiel disponible : un port fait office d'horloge. Il avance
    // l'horloge du bloc PRECEDENT, en entrant — jamais du bloc courant, sinon
    // tout part un bloc trop tot.
    if (clock_master_ < 0) clock_master_ = port;
    if (port == clock_master_) {
      if (last_master_frames_ > 0) {
        clock_.fetch_add((frame_t)last_master_frames_, std::memory_order_acq_rel);
        blocks_.fetch_add(1, std::memory_order_acq_rel);
      }
      last_master_frames_ = frames;
    }
  }
  const frame_t clk = clock_.load(std::memory_order_acquire);
  if (ps.last_clock != clk) {
    // Le bloc precedent est clos : ce port en a-t-il rendu EXACTEMENT la
    // longueur ? Un port qu'on ne lisait plus (apercu arrete puis relance) ne
    // compte pas -- son ecart n'est pas une lecture fautive.
    if (ps.last_clock >= 0 && ps.consumed > 0) {
      const frame_t el = clk - ps.last_clock;
      if (el > 0 && el < (frame_t)4 * ps.consumed) {
        if ((frame_t)ps.consumed > el) ps.over_pulls.fetch_add(1, std::memory_order_relaxed);
        else if ((frame_t)ps.consumed < el) ps.under_pulls.fetch_add(1, std::memory_order_relaxed);
      }
    }
    ps.last_clock = clk;
    ps.consumed = 0;
  }
  const frame_t block_start = clk + ps.consumed;
  ps.consumed += frames;

  // --- commandes ------------------------------------------------------------
  drain(port, block_start);

  // --- silence de depart ----------------------------------------------------
  std::memset(out, 0, (size_t)frames * nch * sizeof(sample_t));

  // --- phase 1 : les voix vivantes ------------------------------------------
  // Chaque voix est rendue EXACTEMENT une fois par bloc. C'est une invariante,
  // pas une convention : la rendre deux fois la fait avancer a double vitesse,
  // ce qui s'entend comme un decalage et se cherche comme un probleme d'horloge.
  //
  // On ne parcourt que les voix de CE port. Le balayage des 256 emplacements
  // qu'il y avait ici coutait, par bloc et par port, plus de cache que le rendu
  // lui-meme.
  for (int k = 0; k < ps.vcount; ++k) {
    Voice& v = voices_[ps.vlist[k]];
    if (v.state == kVoiceIdle) continue;
    v.render(pool_, out, frames, nch, block_start, srate_);
  }

  // --- phase 2 : enchainements, publication, emplacements rendus -------------
  // Une voix qui s'est eteinte DANS ce bloc donne son frame exact d'extinction a
  // sa suivante. Celle-ci n'a rien produit en phase 1 (son rendez-vous etait
  // encore dans le futur lointain), elle est donc rendue ici, une seule fois.
  // C'est ce qui rend le legato exact a l'echantillon plutot qu'au bloc — la
  // fenetre est de 1,33 ms a 64 echantillons, que Lua ne peut pas tenir.
  for (int k = 0; k < ps.vcount; ) {
    const int i = ps.vlist[k];
    Voice& v = voices_[i];

    if (v.ended_at >= 0) {
      const int ni = v.next_voice;
      v.next_voice = -1;
      const frame_t ended = v.ended_at;
      v.ended_at = -1;
      if (ni >= 0 && ni < kMaxVoices) {
        Voice& nv = voices_[ni];
        if (nv.port == port && nv.clip >= 0 && !nv.free_pending &&
            (nv.state == kVoiceScheduled || nv.state == kVoiceIdle)) {
          // xfade_len avance le depart de la suivante. Un VRAI fondu croise
          // demanderait que la sortante continue de sonner pendant le
          // recouvrement : ce n'est pas fait en v1, et c'est ecrit ici plutot
          // que sous-entendu.
          nv.start_at = ended - (frame_t)v.xfade_len;
          if (nv.start_at < block_start) nv.start_at = block_start;
          nv.state = kVoiceScheduled;
          nv.ended_at = -1;
          nv.render(pool_, out, frames, nch, block_start, srate_);
        }
      }
    }

    v.publish();

    // L'emplacement n'est rendu au fil principal QUE lorsque la voix s'est tue.
    // C'est le point qui ferme la course : tant que ce bit reste pose, aucune
    // allocation ne peut tomber sur cet emplacement.
    if (v.free_pending && v.state == kVoiceIdle) {
      v.port = -1;
      v.clip = -1;
      v.free_pending = false;
      v.publish();
      ps.vlist[k] = ps.vlist[ps.vcount - 1];
      --ps.vcount;
      claim_[i].fetch_and(~kClaimOwned, std::memory_order_release);
      continue;   // l'element deplace occupe maintenant k
    }
    ++k;
  }

  // --- gain du port ---------------------------------------------------------
  if (ps.gain != ps.gain_target || ps.gain != 1.0f) {
    float g = ps.gain;
    const float step = (ps.gain_target - ps.gain) / (float)frames;
    for (int i = 0; i < frames; ++i) {
      g += step;
      sample_t* o = out + (size_t)i * nch;
      for (int ch = 0; ch < nch; ++ch) o[ch] *= g;
    }
    ps.gain = ps.gain_target;
  }

  return block_start;
}

} // namespace cp

