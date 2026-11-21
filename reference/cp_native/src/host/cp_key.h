// CP_Native — cp_key.h : LA TRADUCTION ET LA ROUTE DU CLAVIER, ET RIEN D'AUTRE.
// ---------------------------------------------------------------------------
// Des fonctions PURES, sans Windows et sans etat, pour une raison de fond :
// c'est le seul morceau du clavier qui puisse se PROUVER hors REAPER. Tant
// qu'elles vivaient dans `cp_win.cpp`, entre deux procedures de fenetre, aucun
// harnais ne pouvait les atteindre -- et la faute ci-dessous a survecu a une
// nuit entiere de verifications toutes vertes.
//
// ⚠⚠ LA FAUTE QUI JUSTIFIE CE FICHIER : `WM_CHAR` PASSAIT UN CARACTERE A
// `vk_to_gfx`, QUI ATTEND UN CODE VIRTUEL. Les deux sont des `int` et les deux
// tiennent dans un `WPARAM` : le compilateur n'avait rien a dire. Le resultat
// se lisait comme trois defauts sans rapport entre eux :
//
//   - ESPACE (32) ne correspondait a aucun code virtuel connu de la table, donc
//     `vk_to_gfx` rendait ZERO et `push_key` le jetait. La barre d'espace
//     n'existait plus -- pas << ne faisait rien >>, n'ARRIVAIT PAS ;
//   - 'a' a 'o' et tous les chiffres : jetes pour la meme raison ;
//   - 'p' a '{' (112..123) tombaient PILE dans l'intervalle VK_F1..VK_F12.
//     Taper « s » envoyait donc « f4 ». Une touche qui devient une AUTRE touche
//     est le pire des trois : elle ne se lit pas comme une perte.
//
// Les codes de `gfx.getchar` sont deux mondes qui ne se melangent pas :
//   les touches NOMMEES sont de l'ASCII EMPAQUETE ("up" = 0x7570 = 30064) ;
//   les touches IMPRIMABLES sont leur caractere, tel quel (espace = 32).
// `CP_Toolkit/Keys.lua` compare a ces nombres EXACTS. Un ecart d'une unite fait
// tomber tous les raccourcis de toutes les fenetres a la fois.
#ifndef CP_KEY_H
#define CP_KEY_H

namespace cp {
namespace key {

// Les codes virtuels ecrits en clair : ce fichier n'inclut PAS <windows.h>,
// pour qu'un harnais puisse le compiler sur n'importe quoi. Ils sont figes par
// l'ABI de Windows depuis 1985 et ne bougeront pas.
enum {
  kVkBack = 0x08, kVkTab = 0x09, kVkReturn = 0x0D, kVkEscape = 0x1B,
  kVkSpace = 0x20,
  kVkPrior = 0x21, kVkNext = 0x22, kVkEnd = 0x23, kVkHome = 0x24,
  kVkLeft  = 0x25, kVkUp   = 0x26, kVkRight = 0x27, kVkDown = 0x28,
  kVkInsert = 0x2D, kVkDelete = 0x2E,
  kVkF1 = 0x70, kVkF12 = 0x7B
};

// "up" -> 0x7570. Voir la note du fichier : ce n'est pas une curiosite, c'est
// la convention de `gfx.getchar`.
inline int pack_key(const char* t) {
  int v = 0;
  while (*t) { v = (v << 8) | (unsigned char)*t; ++t; }
  return v;
}

// LES TOUCHES NOMMEES, depuis un code VIRTUEL (`WM_KEYDOWN`).
//
// ⚠ ZERO VEUT DIRE << PAS POUR MOI >>, et c'est ce qui evite le doublon. Les
// touches imprimables arrivent par `WM_CHAR`, qui seul connait la disposition
// du clavier : sur AZERTY, le code virtuel de la touche 'a' est celui du 'q'
// americain. Les traduire ici reviendrait a retaper une table par pays, et a
// faire arriver chaque frappe DEUX fois.
inline int vk_to_gfx(int vk) {
  switch (vk) {
    case kVkUp:     return pack_key("up");
    case kVkDown:   return pack_key("down");
    case kVkLeft:   return pack_key("left");
    case kVkRight:  return pack_key("rght");
    case kVkHome:   return pack_key("home");
    case kVkEnd:    return pack_key("end");
    case kVkPrior:  return pack_key("pgup");
    case kVkNext:   return pack_key("pgdn");
    case kVkInsert: return pack_key("ins");
    case kVkDelete: return pack_key("del");
    default: break;
  }
  if (vk >= kVkF1 && vk <= kVkF12) {
    char buf[8];
    const int n = vk - kVkF1 + 1;
    if (n < 10) { buf[0] = 'f'; buf[1] = (char)('0' + n); buf[2] = 0; }
    else { buf[0] = 'f'; buf[1] = '1'; buf[2] = (char)('0' + n - 10); buf[3] = 0; }
    return pack_key(buf);
  }
  return 0;
}

// LES TOUCHES IMPRIMABLES, depuis un CARACTERE (`WM_CHAR`).
//
// Le caractere EST le code : c'est la convention de `gfx.getchar`, et il n'y a
// donc rien a traduire. Toute la valeur de cette fonction est dans ce qu'elle
// ne fait PAS -- surtout pas passer par `vk_to_gfx`.
//
// ⚠ LA BORNE BASSE EST 1, PAS 8, ET CE N'EST PAS UN DETAIL. Ctrl+A rend 1,
// Ctrl+C rend 3, Ctrl+D rend 4 : trois raccourcis ECRITS dans
// `CP_Engine/Keymaps/editor.lua` (tout selectionner, copier, dupliquer). Une
// borne a 8 les jetait tous les trois en silence, et le defaut se serait lu
// << Ctrl+C ne marche plus dans l'editeur >> : on aurait cherche du cote du
// presse-papiers.
inline int char_to_gfx(unsigned int ch) {
  if (ch == 0) return 0;             // pas une frappe
  if (ch > 0x10FFFF) return 0;       // hors Unicode : ce n'est pas un caractere
  return (int)ch;
}

// ---------------------------------------------------------------------------
// LA ROUTE D'UNE TOUCHE, VUE DU CROCHET WH_GETMESSAGE
// ---------------------------------------------------------------------------
// Le crochet voit chaque message clavier du fil principal AVANT REAPER, et il
// a trois choses possibles a en faire. La decision est ici, PURE, pour la meme
// raison que la table au-dessus : entre deux appels Windows, aucun harnais ne
// pouvait la voir, et c'est exactement la que le correctif precedent s'est
// trompe sans qu'aucune verification ne bronche.
//
//   GARDER     REAPER traite le message tel quel. Pour une fenetre FLOTTANTE
//              ou ANCREE a nous, c'est le bon chemin : l'accelerateur rend -20
//              et REAPER la lui passe, `TranslateMessage` compris.
//   READRESSER la touche visait une fenetre de REAPER alors que le dernier clic
//              etait chez nous : elle part a la fenetre qui revendique.
//   REMETTRE   la DLL donne la touche a la fenetre ELLE-MEME, et REAPER ne la
//              voit jamais.
//
// ⚠⚠ REMETTRE EXISTE POUR UNE SEULE SORTE DE FENETRE : L'HEBERGEE.
// CP_ArrangeHost (CP_ViewFX, CP_ViewSession) fait d'une fenetre flottante un
// ENFANT DIRECT de la fenetre principale, a la place de l'arrangement. REAPER
// ne la connait pas -- ce n'est ni un dialogue de docker ni une fenetre de
// premier rang -- et une touche qui lui est adressee n'arrivait jamais a sa
// procedure : le journal de la session du 2026-10-04 montre la vue hebergee
// pendant une minute, et pas un seul `WM_CHAR` recu par la procedure
// flottante, ni une seule touche readressee. La revendication au clic
// (`take_focus`) ne pouvait rien y faire : elle ne corrige que les touches
// adressees AILLEURS, et celles-ci etaient deja adressees a la bonne fenetre.
// On ne cherche pas a deviner ce que REAPER en fait : on ne le laisse plus
// faire.
//
// ⚠ ET JAMAIS PENDANT UN MENU : ses fleches, son Entree et son Echap sont a
// lui, quelle que soit la fenetre qui avait le clavier quand il s'est ouvert.

// Les messages, ecrits en clair comme les codes virtuels plus haut.
enum {
  kWmKeyDown = 0x0100, kWmKeyUp = 0x0101, kWmChar = 0x0102,
  kWmDeadChar = 0x0103, kWmSysKeyDown = 0x0104, kWmSysKeyUp = 0x0105,
  kWmSysChar = 0x0106, kWmSysDeadChar = 0x0107
};

inline bool is_key_msg(unsigned int msg) {
  return msg >= (unsigned int)kWmKeyDown && msg <= (unsigned int)kWmSysDeadChar;
}

// Ce que le crochet sait du message au moment de decider. Chaque champ est UNE
// question posee a Windows par `cp_win.cpp` ; aucun n'est deduit d'un autre.
struct KeyScene {
  bool menu;          // un menu est ouvert (GUI_INMENUMODE et ses deux freres)
  bool to_ours;       // la cible est une de nos fenetres
  bool to_hosted;     // ... et c'est une fenetre HEBERGEE (voir plus haut)
  bool to_main;       // la racine de la cible est la fenetre principale
  bool to_text;       // la cible est un champ de saisie de REAPER
  bool claim;         // une de nos fenetres revendique le clavier (dernier clic)
  bool claim_hosted;  // ... et c'est une fenetre hebergee
  bool to_claim;      // la cible EST la fenetre qui revendique (ou un enfant)
};

enum { kRouteKeep = 0, kRouteReaddress = 1, kRouteHand = 2 };

// Un masque : READRESSER puis REMETTRE se cumulent quand la fenetre qui
// revendique est hebergee -- REAPER a repris le focus apres le clic, la touche
// vise l'arrangement cache, et la rendre a la fenetre hebergee en la laissant
// a REAPER la perdrait de la meme facon que si elle y avait ete adressee.
//
// ⚠⚠ LE DERNIER CLIC L'EMPORTE SUR LE FOCUS, MEME QUAND LE FOCUS EST CHEZ NOUS
// (2026-10-06). Cedric : « les touches ne fonctionnent plus sur les scripts a
// partir du moment ou un des scripts est dans l'arrangehost -- ni raccourci,
// ni barre de recherche ; Espace devrait jouer l'editeur ». Le journal :
// « 12 touches remises par le crochet a FX Constellation, 12 rendues a REAPER
// (12 avec une action) », et pas une readressee. Apres un clic dans l'editeur
// ancre, REAPER rend le focus a ce qui occupe la place de l'arrangeur -- la VUE
// HEBERGEE, puisque l'hote le lui a donne en la posant. La table regardait
// « adressee a une de nos fenetres » AVANT la revendication : la touche partait
// a la vue, qui ne la prenait pas, puis aux raccourcis de REAPER. Espace
// lancait le transport au lieu de l'editeur. Une revendication qui n'est pas
// la cible readresse donc, que la cible soit a REAPER OU A NOUS.
inline int route_key(const KeyScene& s) {
  if (s.menu) return kRouteKeep;
  if (s.claim && !s.to_claim && s.to_main && !s.to_text)
    return kRouteReaddress | (s.claim_hosted ? kRouteHand : 0);
  if (s.to_ours) return s.to_hosted ? kRouteHand : kRouteKeep;
  return kRouteKeep;
}

// Une touche REMISE : qu'en tire la fenetre ?
//   kHandKey     un appui -- la touche NOMMEE, ou `TranslateMessage` sur une
//                copie, qui poste le `WM_CHAR` avec la disposition, les touches
//                mortes et AltGr, exactement comme la boucle de REAPER l'aurait
//                fait ;
//   kHandChar    un caractere : le code tel quel (`char_to_gfx`) ;
//   kHandSwallow un relachement ou une touche morte : rien a pousser, mais le
//                message est AVALE quand meme. REAPER verrait sinon un
//                relachement dont il n'a jamais vu l'appui.
enum { kHandSwallow = 0, kHandKey = 1, kHandChar = 2 };

inline int hand_kind(unsigned int msg) {
  switch (msg) {
    case kWmKeyDown: case kWmSysKeyDown: return kHandKey;
    case kWmChar:    case kWmSysChar:    return kHandChar;
    default: return kHandSwallow;
  }
}

// ---------------------------------------------------------------------------
// LA TOUCHE QU'UNE VUE HEBERGEE N'A PAS PRISE (2026-10-04, nuit)
// ---------------------------------------------------------------------------
// Cedric, apres la remise par le crochet : « les raccourcis clavier ne
// fonctionnent toujours pas lorsqu'un CP_View est actif ». Le journal de la
// session disait pourtant que la remise MARCHAIT : `touche remise a la fenetre
// HEBERGEE "FX Constellation"`. Les touches arrivaient donc -- et c'est
// exactement le defaut :
//
//   * REMETTRE, c'est changer le message en WM_NULL : REAPER ne voit plus RIEN
//     de ce qu'on tape dans une vue hebergee ;
//   * or FX Constellation n'a AUCUN raccourci a lui (un seul test de touche
//     dans tout le produit : Echap). Espace, Ctrl+Z, Ctrl+S, R... arrivaient a
//     une fenetre qui n'en fait rien, et mouraient la ;
//   * et la vue occupe la place de l'arrangeur : il n'y a plus d'arrangeur ou
//     cliquer pour rendre le clavier a REAPER. Une fenetre flottante a la meme
//     regle et personne ne la remarque, parce qu'on clique a cote.
//
// La touche appartient d'abord a la fenetre : c'est elle qui sait si Espace
// lance sa grille ou rien du tout. Mais celle qu'elle n'a PAS prise doit
// repartir chez REAPER, comme si l'arrangeur l'avait eue. Seul Lua sait si une
// touche a servi (`Core.ConsumeChar`) ; il le dit APRES sa frame, et la DLL la
// rend alors a la table de raccourcis de la section Main
// (`kbd_translateAccelerator`, cp_win.cpp `pass_key`).
//
// ⚠ IL FAUT POUR CELA SE SOUVENIR DE CE QUE LA TOUCHE ETAIT. La file de Lua ne
// porte que le code de `gfx.getchar` ; REAPER, lui, lie des codes VIRTUELS et
// des modificateurs. Chaque case de la file garde donc son ORIGINE : l'appui
// (WM_KEYDOWN / WM_SYSKEYDOWN) et, pour une imprimable, le caractere que
// `TranslateMessage` en a fait. Le caractere arrive AVANT l'appui suivant --
// il est poste, et Windows rend les messages postes avant l'entree -- donc
// « l'appui en attente » le designe sans ambiguite.

struct KeyOrigin {
  unsigned int       down_msg;   // WM_KEYDOWN ou WM_SYSKEYDOWN, 0 si inconnu
  unsigned long long down_wp;    // le code virtuel
  long long          down_lp;    // lParam : balayage, repetition, Alt
  unsigned int       char_msg;   // WM_CHAR ou WM_SYSCHAR, 0 pour une touche nommee
  unsigned long long char_wp;
  long long          char_lp;
};

// Zero partout : l'origine est INCONNUE. C'est le cas de toute touche arrivee
// par une procedure de fenetre (flottante, ancree) -- et une origine inconnue
// ne se rend jamais.
inline KeyOrigin no_origin() {
  KeyOrigin o = { 0u, 0ull, 0ll, 0u, 0ull, 0ll };
  return o;
}

inline bool has_origin(const KeyOrigin& o) {
  return o.down_msg != 0 || o.char_msg != 0;
}

// Un appui remis par le crochet. Il devient « l'appui en attente » : la touche
// NOMMEE qu'il produit l'emporte telle quelle, le caractere qu'il fera naitre
// le reprendra (`origin_of_char`).
inline KeyOrigin origin_of_down(unsigned int msg, unsigned long long wp,
                                long long lp) {
  KeyOrigin o = no_origin();
  o.down_msg = msg; o.down_wp = wp; o.down_lp = lp;
  return o;
}

// Un caractere remis : l'appui qui l'a fait naitre, plus lui. Sans appui en
// attente (un caractere poste par quelqu'un d'autre), il garde le caractere
// seul -- REAPER sait aussi lier un caractere.
inline KeyOrigin origin_of_char(const KeyOrigin& pending, unsigned int msg,
                                unsigned long long wp, long long lp) {
  KeyOrigin o = pending;
  o.char_msg = msg; o.char_wp = wp; o.char_lp = lp;
  return o;
}

// Ce que la DLL sait au moment ou Lua dit « je ne l'ai pas prise ».
struct PassScene {
  bool hosted;   // la fenetre est HEBERGEE maintenant (style vivant, `is_hosted`)
  bool menu;     // un menu est ouvert
  int  code;     // le code `gfx.getchar` que Lua a depile
};

// Quoi rendre a REAPER, dans l'ordre : l'appui d'abord, puis le caractere si
// l'appui n'etait lie a rien. C'est l'ordre de la boucle de REAPER elle-meme :
// un appui traite comme raccourci n'est jamais traduit, et sinon son caractere
// passe a son tour par les raccourcis.
enum { kPassDown = 1, kPassChar = 2 };

inline int pass_plan(const PassScene& s, const KeyOrigin& o) {
  // ⚠ SEULE UNE VUE HEBERGEE REND SES TOUCHES. Une fenetre flottante ou
  // ancree garde le contrat d'aout -- « la fenetre qui a le clavier le garde,
  // REAPER saute ses raccourcis » (`hwnd_info` type 1) -- et l'arrangeur reste
  // a un clic. La vue hebergee, elle, EST la place de l'arrangeur.
  if (!s.hosted || s.menu) return 0;
  // ⚠ ECHAP EST AU TOOLKIT : il ferme un menu, lache un champ, puis la
  // fenetre. Le contrat de REAPER le met lui aussi a part (-666 : « with the
  // exception of ESC »).
  if (s.code <= 0 || s.code == kVkEscape) return 0;
  int p = 0;
  if (o.down_msg == (unsigned int)kWmKeyDown ||
      o.down_msg == (unsigned int)kWmSysKeyDown) p |= kPassDown;
  if (o.char_msg == (unsigned int)kWmChar ||
      o.char_msg == (unsigned int)kWmSysChar) p |= kPassChar;
  return p;
}

}  // namespace key
}  // namespace cp

#endif  // CP_KEY_H

