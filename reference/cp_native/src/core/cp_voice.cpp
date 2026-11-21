#include "cp_voice.h"
#include "cp_curve.h"
#include "cp_pool.h"

#include <cmath>
#include <cstring>
#include <limits>

namespace cp {

// Seuil sous lequel un gain est force a zero. Les denormaux coutent 100 a 400
// cycles sur un Athlon 64 ou un P4 — c'est le seul piege capable a lui seul de
// rendre le moteur inutilisable sur la cible. On les tue a la source plutot que
// de poser FTZ/DAZ globalement : REAPER regle deja le MXCSR de son fil audio, et
// le modifier depuis un module charge degraderait tous les autres plugins.
static const float kDenormalFloor = 1e-15f;

void Voice::reset() {
  state = kVoiceIdle;
  port = -1;
  clip = -1;
  pos = 0.0;
  start_at = 0;
  stop_at = (std::numeric_limits<frame_t>::max)();
  mode = kPlayOnce;
  rate = 1.0;
  gain = 1.0f;
  gain_target = 1.0f;
  pan = 0.0f;
  loop_start = 0;
  loop_end = 0;
  fade_in_len = 0;
  fade_in_cur = 0;
  fade_in_pos = 0;
  fade_out_len = 0;
  fade_out_pos = 0;
  env_on = false;
  env_a = 0.0f; env_d = 0.0f; env_s = 1.0f; env_r = 0.0f;
  env_ka = 1.0f; env_kd = 1.0f; env_kr = 1.0f;
  env_st = 0;
  env_x = 0.0;
  env_lv = 0.0f;
  env_rl0 = 0.0f;
  env_rxi = 1.0;
  // `envp` (le pointeur) n'est PAS touche : il est pose une fois par l'Engine
  // et reset() est appele a chaque allocation de la voix.
  envp_on = false;
  envp_n = 0;
  envp_act = 0;
  envp_rx = 0;
  envp_i = 0;
  tail_pos = 0.0;
  tail_len = 0;
  tail_i = 0;
  next_voice = -1;
  xfade_len = 0;
  started_at = -1;
  ended_at = -1;
  free_pending = false;
  publish();
}

// Hermite 4 points. Choisi contre le lineaire (audible sur des transitoires) et
// contre un sinc 64 taps (hors budget sur la machine cible). ~10 operations par
// echantillon et par canal, sans table, sans branchement.
static inline float hermite(float xm1, float x0, float x1, float x2, float t) {
  const float c = (x1 - xm1) * 0.5f;
  const float v = x0 - x1;
  const float w = c + v;
  const float a = w + v + (x2 - x0) * 0.5f;
  const float b = w + a;
  return ((((a * t) - b) * t + c) * t + x0);
}

// La courbe d'un etage d'enveloppe : une droite a k = 1, bombee au-dessus
// (k < 1) ou au-dessous (k > 1). Une division, pas d'exponentielle par
// echantillon -- la fonction `shp` du JSFX du kit, mot pour mot.
// La loi est dans cp_curve.h : l'automation d'un clip (cp_lanes.cpp) la suit
// aussi, et une seule ecriture ne peut pas diverger d'elle-meme.
static inline float shp(float x, float k) { return curve_shp(x, k); }

// ---------------------------------------------------------------------------
// L'ENVELOPPE A POINTS -- la regle, ecrite une fois (le miroir Lua est
// `Env.Eval`, CP_Engine/Env.lua : les changer ensemble).
//   * avant le premier point : son gain ; apres le dernier : le sien ;
//   * entre deux points : g0 + (g1 - g0) * shp(x, k), x la progression en
//     temps source dans le segment.
// `cur` est le curseur du segment : la lecture avance, donc on n'avance que
// d'un cran en general ; une boucle ou un saut le fait reculer, une fois. Les
// deux marches sont bornees par n <= kMaxEnvPts : aucune ne peut tourner
// longtemps dans le fil audio.
// ---------------------------------------------------------------------------
static inline float envp_eval(const EnvPt* e, int n, float t, int* cur) {
  int i = *cur;
  if (i < 0 || i >= n) i = 0;
  if (t < e[i].t) {
    while (i > 0 && t < e[i].t) --i;
  } else {
    while (i + 1 < n && t >= e[i + 1].t) ++i;
  }
  *cur = i;
  if (t <= e[0].t) return e[0].g;
  if (i + 1 >= n) return e[n - 1].g;
  const float dt = e[i + 1].t - e[i].t;
  if (dt <= 0.0f) return e[i + 1].g;
  const float x = (t - e[i].t) / dt;
  return e[i].g + (e[i + 1].g - e[i].g) * shp(x, e[i].k);
}

void Voice::env_point(int idx, double t, double g, double c) {
  if (!envp || idx < 0 || idx >= kMaxEnvPts) return;
  EnvPt& p = envp->a[envp_act ^ 1][idx];
  p.t = (float)(t > 0.0 ? t : 0.0);
  p.g = (float)(g < 0.0 ? 0.0 : (g > 4.0 ? 4.0 : g));
  p.k = (float)(c < -1.0 ? -1.0 : (c > 1.0 ? 1.0 : c));   // brute jusqu'a la validation
  ++envp_rx;
}

bool Voice::env_commit(int n) {
  const int rx = envp_rx;
  envp_rx = 0;
  if (n <= 0) { envp_on = false; envp_n = 0; return true; }
  if (!envp || n > kMaxEnvPts || rx != n) return false;
  EnvPt* s = envp->a[envp_act ^ 1];
  for (int i = 1; i < n; ++i) if (s[i].t < s[i - 1].t) return false;
  // La courbe brute devient le facteur de shp, selon le sens du segment. Un
  // pow par point et par validation -- jamais par echantillon.
  for (int i = 0; i < n; ++i) {
    if (i + 1 < n) {
      s[i].k = curve_factor(s[i].k, s[i + 1].g >= s[i].g);
    } else {
      s[i].k = 1.0f;
    }
  }
  envp_act ^= 1;
  envp_n = n;
  envp_i = 0;
  envp_on = true;
  env_on = false;   // exclusives : l'ADSR se tait
  return true;
}

// Une lecture a la position `p`, pour les deux tetes : la vivante et, pendant
// un croisement, l'ancienne. Une seule ecriture du bornage, parce que deux
// copies finiraient par differer d'un echantillon.
static inline void read_at(const sample_t* data, int cnch, double p,
                           bool unity, bool looping,
                           frame_t src_start, frame_t src_end,
                           float* l, float* r) {
  const frame_t i0 = (frame_t)p;
  if (unity) {
    const sample_t* s = data + (size_t)i0 * cnch;
    *l = s[0];
    *r = (cnch > 1) ? s[1] : s[0];
    return;
  }
  const float t = (float)(p - (double)i0);
  frame_t im1 = i0 - 1, i1 = i0 + 1, i2 = i0 + 2;
  // Bornage par repli dans la zone de boucle : jamais de lecture hors du
  // tampon, et pas de branchement imprevisible dans la boucle chaude.
  if (im1 < src_start) im1 = looping ? src_end - 1 : src_start;
  if (i1 >= src_end)   i1  = looping ? src_start : src_end - 1;
  if (i2 >= src_end)   i2  = looping ? src_start : src_end - 1;

  const sample_t* pm1 = data + (size_t)im1 * cnch;
  const sample_t* p0  = data + (size_t)i0  * cnch;
  const sample_t* p1  = data + (size_t)i1  * cnch;
  const sample_t* p2  = data + (size_t)i2  * cnch;

  *l = hermite(pm1[0], p0[0], p1[0], p2[0], t);
  *r = (cnch > 1) ? hermite(pm1[1], p0[1], p1[1], p2[1], t) : *l;
}

// ---------------------------------------------------------------------------
// POURQUOI TOUT L'ETAT MUTABLE DESCEND EN VARIABLE LOCALE
//
// `out` est un `sample_t*` et `pos` un `double` membre : rien n'interdit
// formellement au premier de pointer sur le second. Le compilateur DOIT donc
// relire et reecrire chaque membre a chaque tour de boucle — position, fondus,
// gain, etat : une dizaine d'acces memoire par echantillon, la ou il n'en faut
// aucun. Descendre l'etat en local le lui prouve, et il garde tout en registre.
// C'est la seule optimisation de ce fichier qui se voit sur la machine cible ;
// les autres sont du bruit a cote.
// ---------------------------------------------------------------------------
void Voice::render(const Pool& pool, sample_t* out, int frames, int nch,
                   frame_t block_start, double engine_srate) {
  if (state == kVoiceIdle) return;

  const Clip* c = pool.get(clip);
  if (!c || !c->data || c->frames <= 0) {
    // LA MATIERE A DISPARU SOUS LA VOIX. Un clip decharge pendant qu'on le joue
    // cesse d'etre visible du fil audio des le bloc suivant : la voix n'a plus
    // rien a lire.
    //
    // Elle DOIT mourir ici. Repartir sans rien changer la laissait vivante pour
    // toujours — plus rien ne faisait avancer son fondu, donc elle n'atteignait
    // jamais l'etat eteint, donc son emplacement n'etait jamais rendu. Une
    // fenetre qui recharge ses clips en cours de jeu aurait epuise ses voix en
    // silence, et le symptome se serait manifeste une heure plus tard sous la
    // forme « il n'y a plus de voix ».
    //
    // On ne pose PAS ended_at : l'enchainement est un comportement musical, et
    // ceci est un chemin d'erreur. Le suivant ne doit pas partir sur une
    // disparition.
    state = kVoiceIdle;
    clip = -1;
    tail_len = 0;
    return;
  }

  // Ou commence-t-on dans ce bloc ? C'est ici, et nulle part ailleurs, que se
  // joue l'exactitude a l'echantillon : le rendez-vous n'est pas arrondi au
  // bloc, il est converti en decalage a l'interieur du bloc.
  int off = 0;
  if (state == kVoiceScheduled) {
    if (start_at >= block_start + frames) return;       // pas encore
    off = (int)(start_at > block_start ? (start_at - block_start) : 0);
    state = kVoicePlaying;
    started_at = block_start + off;   // la verite terrain de l'attaque
    fade_in_pos = 0;
  }

  // Coupure datee, elle aussi a l'interieur du bloc.
  int n = frames;
  if (stop_at < block_start + frames) {
    const frame_t rel = stop_at - block_start;
    n = (int)(rel < 0 ? 0 : rel);
    if (n < off) n = off;
  }

  // UN ARRET DATE DEMANDAIT UN FONDU ET N'EN OBTENAIT AUCUN.
  //
  // kCmdVoiceStop pose `stop_at` et `fade_out_len` sans passer la voix en
  // kVoiceStopping — c'est le rendu qui tronque a `stop_at`, et la branche de
  // fondu ci-dessous ne s'execute donc jamais : l'arret est une coupure nette,
  // au milieu de la forme d'onde. Tant que la matiere d'une case mourait
  // d'elle-meme avant la porte, ca ne s'entendait pas. Des qu'elle boucle, ca
  // clique a chaque frontiere de passe.
  //
  // On fait donc COMMENCER le fondu assez tot pour qu'il atteigne zero
  // exactement au rendez-vous, plutot que de le declencher au rendez-vous —
  // ce qui obligerait a rendre au-dela de lui. Le fondu peut avoir commence
  // dans un bloc precedent : `fop` est alors deja avance, et l'expression le
  // dit sans avoir a s'en souvenir. Cout : une comparaison par bloc, aucune
  // par echantillon — cette boucle ne doit rien apprendre de plus.
  if (state == kVoicePlaying && fade_out_len > 0) {
    const frame_t to_stop = stop_at - block_start;
    if (to_stop <= (frame_t)fade_out_len) {
      state = kVoiceStopping;
      const frame_t done = (frame_t)fade_out_len - (to_stop > 0 ? to_stop : 0);
      fade_out_pos = (int)(done > 0 ? done : 0);
    }
  }
  if (n <= off) {
    if (stop_at <= block_start) {
      state = kVoiceIdle;
      clip = -1;
      ended_at = stop_at;
    }
    return;
  }

  const frame_t src_end   = (loop_end > 0 && loop_end <= c->frames) ? loop_end : c->frames;
  const frame_t src_start = (loop_start >= 0 && loop_start < src_end) ? loop_start : 0;
  const frame_t span      = src_end - src_start;
  const int     cnch      = c->nch;
  const sample_t* const data = c->data;

  // Balance, pas panoramique a puissance constante. Un lanceur de clips joue du
  // materiau deja mixe : au centre, il doit passer a l'identique. Une loi a
  // puissance constante mettrait -3 dB sur tout ce qui n'est pas panoramique, ce
  // qui est faux ici et rend le rendu non comparable a la source.
  const float pnorm = (pan < -1.0f ? -1.0f : (pan > 1.0f ? 1.0f : pan));
  const float gl    = (pnorm > 0.0f) ? (1.0f - pnorm) : 1.0f;
  const float gr    = (pnorm < 0.0f) ? (1.0f + pnorm) : 1.0f;

  // Le clip est stocke au taux ou il a ete decode. Si l'appareil change de taux
  // en cours de route, cette matiere ne bouge pas — c'est la LECTURE qui doit
  // s'adapter, sinon la hauteur change. Defaut trouve en changeant de taux
  // d'echantillonnage pendant que le moteur jouait : « le son reprend, mais
  // avec le pitch change ». Le taux de sortie est une entree PAR BLOC, jamais
  // une constante d'initialisation.
  const double sr_ratio = (engine_srate > 1.0 && c->srate > 1.0)
                        ? (c->srate / engine_srate) : 1.0;
  const double step  = rate * sr_ratio;
  const bool   unity = (step > 0.99999999 && step < 1.00000001);
  const bool   looping = (mode == kPlayLoop && span > 0);

  // --- l'etat mutable descend en registre ------------------------------------
  double    p   = pos;
  int       st  = state;
  int       fip = fade_in_pos;
  int       fop = fade_out_pos;
  float     g   = gain;
  const int fil = fade_in_cur;
  const int fol = fade_out_len;
  // L'ancienne tete, si un saut vient d'avoir lieu. `tl` vaut zero le reste du
  // temps, et c'est la seule chose que la boucle chaude a a verifier.
  double    tp  = tail_pos;
  int       ti  = tail_i;
  const int tl  = tail_len;

  // Rampe de gain sur le bloc : un saut de gain sec s'entend, et une rampe par
  // bloc coute une addition par echantillon.
  const float gstep = (n > off) ? (gain_target - g) / (float)(n - off) : 0.0f;

  // L'ENVELOPPE, en registre comme le reste. Les pas sont recalcules par bloc :
  // un bouton tourne pendant que le son tient prend effet au bloc suivant, sans
  // rien demander au fil principal. Les durees sont en temps de SORTIE (le
  // `srate` du moteur), comme dans le JSFX : Tune n'allonge pas l'attaque.
  const bool   eon  = env_on;
  int          est  = env_st;
  double       ex   = env_x;
  float        elv  = env_lv;
  const double ainc = eon ? 1.0 / ((env_a * engine_srate > 1.0) ? env_a * engine_srate : 1.0) : 0.0;
  const double dinc = eon ? 1.0 / ((env_d * engine_srate > 1.0) ? env_d * engine_srate : 1.0) : 0.0;
  const double rxi  = env_rxi;
  const float  esus = env_s, eka = env_ka, ekd = env_kd, ekr = env_kr, rl0 = env_rl0;
  // LA RAMPE DE FIN D'UN ONE-SHOT : 1 tant qu'on est a plus d'un relache de la
  // fin de la region, puis jusqu'a zero au dernier echantillon. En trames
  // SOURCE (le relache en secondes de sortie, multiplie par le pas de lecture),
  // parce que c'est en trames source qu'on mesure la distance a la fin.
  double relk = 0.0;
  if (eon && !looping) {
    const double rl = (double)env_r * engine_srate * step;
    relk = 1.0 / ((rl > 1.0) ? rl : 1.0);
  }
  // L'ENVELOPPE A POINTS, lue a la POSITION de lecture (secondes source = trames
  // du clip / sa frequence) : elle suit la matiere, pas l'horloge. Eteinte, elle
  // ne coute qu'un test par echantillon, toujours predit.
  const bool   pon = envp_on && envp_n > 0 && envp != nullptr;
  const EnvPt* ept = pon ? envp->a[envp_act] : nullptr;
  const int    epn = envp_n;
  int          epi = envp_i;
  const double inv_csr = (c->srate > 1.0) ? 1.0 / c->srate
                                          : 1.0 / ((engine_srate > 1.0) ? engine_srate : 48000.0);

  frame_t ended_here = -1;   // -1 = la voix a survecu a ce bloc

  for (int i = off; i < n; ++i) {
    // --- fin de matiere ------------------------------------------------------
    if (p >= (double)src_end) {
      if (looping) {
        // Repli exact : on retire un nombre entier de spans, la phase
        // fractionnaire est preservee. C'est ce qui evite la derive d'un
        // echantillon par tour, invisible au debut et fatale au bout de dix
        // minutes.
        //
        // ⚠️ UNE SEULE SOUSTRACTION, PUIS UNE REDUCTION EN O(1). C'etait une
        // boucle `while`, et son nombre de tours valait (p - fin) / span : rien
        // dans ce fichier ne borne `p`. Une position posee loin devant — un
        // rendez-vous mal date, une reprise apres un saut de transport, un
        // appelant qui se trompe d'unite — faisait tourner cette boucle des
        // millions de fois DANS LE FIL AUDIO. Ce n'est pas une lenteur : le bloc
        // ne rend jamais, l'appareil decroche, et de l'exterieur ca se voit comme
        // « le moteur audio a plante ».
        //
        // Le cas normal reste une soustraction et ne coute rien de plus. Le cas
        // pathologique coute un `floor` — une fois, et jamais par echantillon.
        p -= (double)span;
        if (p >= (double)src_end) {
          const double d = p - (double)src_start;
          p = (double)src_start + (d - std::floor(d / (double)span) * (double)span);
          // Un `p` non fini ne se reduit pas : la voix s'eteint plutot que de
          // lire n'importe ou. Aucune valeur ne devrait arriver ici, et c'est
          // exactement pour ca qu'on le dit.
          if (!(p >= (double)src_start && p < (double)src_end)) {
            st = kVoiceIdle;
            ended_here = block_start + i;
            break;
          }
        }
      } else {
        st = kVoiceIdle;
        ended_here = block_start + i;
        break;
      }
    }

    // --- lecture interpolee --------------------------------------------------
    float l, r;
    read_at(data, cnch, p, unity, looping, src_start, src_end, &l, &r);
    if (pon) {
      const float ge = envp_eval(ept, epn, (float)(p * inv_csr), &epi);
      l *= ge;
      r *= ge;
    }

    // --- croisement avec l'ancienne tete --------------------------------------
    // Pendant `tl` frames apres un saut, l'ancienne tete continue de lire et
    // descend lineairement pendant que la nouvelle monte. Lineaire et non a
    // puissance constante : sur une matiere coherente (un deplacement dans le
    // meme fichier) la somme vaut exactement la source, ce qu'une loi en racine
    // ne donnerait pas. Si l'ancienne tete sort de la matiere, elle se tait.
    if (ti < tl) {
      float lo = 0.0f, ro = 0.0f;
      if (tp >= (double)src_end) {
        if (looping) tp -= (double)span;
      }
      if (tp >= (double)src_start && tp < (double)src_end)
        read_at(data, cnch, tp, unity, looping, src_start, src_end, &lo, &ro);
      // L'ancienne tete porte le gain de SA position : sur une matiere
      // coherente, la somme reste la source enveloppee. Un curseur a part, qui
      // part du courant -- le croisement dure 5 ms, la marche est courte.
      if (pon) {
        int tc = epi;
        const float gt = envp_eval(ept, epn, (float)(tp * inv_csr), &tc);
        lo *= gt;
        ro *= gt;
      }
      const float w = (float)ti / (float)tl;
      l = l * w + lo * (1.0f - w);
      r = r * w + ro * (1.0f - w);
      tp += step;
      ++ti;
    }

    // --- enveloppes ----------------------------------------------------------
    float env = 1.0f;
    if (fil > 0 && fip < fil) {
      env *= (float)fip / (float)fil;
      ++fip;
    }
    if (st == kVoiceStopping) {
      if (fol > 0 && fop < fol) {
        env *= 1.0f - (float)fop / (float)fol;
        ++fop;
      } else {
        st = kVoiceIdle;
        ended_here = block_start + i;
        break;
      }
    }
    // --- ADSR ------------------------------------------------------------------
    // L'ordre est celui du JSFX : l'etage avance AVANT la lecture de
    // l'echantillon, donc le premier echantillon d'une attaque vaut deja un pas.
    if (eon) {
      if (est == 0) {
        ex += ainc;
        if (ex >= 1.0) { elv = 1.0f; ex = 0.0; est = 1; }
        else elv = shp((float)ex, eka);
      } else if (est == 1) {
        ex += dinc;
        if (ex >= 1.0) { elv = esus; ex = 0.0; est = 2; }
        else elv = 1.0f - (1.0f - esus) * shp((float)ex, ekd);
      } else if (est == 3) {
        ex += rxi;
        if (ex >= 1.0) {
          // Le relache est arrive au bout : la voix est finie, comme la note
          // d'un instrument -- et son emplacement redevient disponible.
          elv = 0.0f;
          st = kVoiceIdle;
          ended_here = block_start + i;
          break;
        }
        elv = rl0 * (1.0f - shp((float)ex, ekr));
      }
      env *= elv;
      if (relk > 0.0) {
        const double rk = ((double)src_end - 1.0 - p) * relk;
        if (rk < 1.0) env *= 1.0f - shp((float)(1.0 - (rk > 0.0 ? rk : 0.0)), ekr);
      }
    }

    g += gstep;
    float gg = g * env;
    if (gg < kDenormalFloor && gg > -kDenormalFloor) gg = 0.0f;

    sample_t* o = out + (size_t)i * nch;
    o[0] += l * gg * gl;
    if (nch > 1) o[1] += r * gg * gr;

    p += step;
  }

  // --- l'etat remonte en memoire, une seule fois -----------------------------
  pos = p;
  fade_in_pos = fip;
  fade_out_pos = fop;
  env_st = est;
  env_x = ex;
  env_lv = elv;
  envp_i = epi;
  if (tl > 0) {
    if (ti >= tl) { tail_len = 0; tail_i = 0; }   // croisement termine
    else          { tail_pos = tp; tail_i = ti; }
  }
  state = st;
  gain = gain_target;
  if (ended_here >= 0) {
    clip = -1;
    ended_at = ended_here;
  }

  // Coupure nette datee : la voix s'eteint exactement au frame demande.
  if (stop_at < block_start + frames && state != kVoiceIdle && fade_out_len == 0) {
    state = kVoiceIdle;
    clip = -1;
    ended_at = stop_at;
  }
}

} // namespace cp

