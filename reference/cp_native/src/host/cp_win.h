// cp_win — une fenetre REAPER qui dessine avec NOTRE rasteriseur.
//
// ============================================================================
// CE QUE CE PROTOTYPE EXISTE POUR REPONDRE
// ============================================================================
// Cinq questions, et aucune ne se repond par la lecture. Elles sont ecrites
// dans ROADMAP_Rendu.md §5, G-D :
//
//   1. `DockWindowAddEx` ancre-t-il proprement une fenetre que nous creons ?
//   2. Le clavier arrive-t-il, via `accelerator` ?
//   3. Le texte par `LICE__GetDC` + GDI est-il REELLEMENT meilleur que
//      `gfx.drawstr`, a 9 px et 11 px — les tailles du theme Constellation ?
//   4. Combien coute une frame, mesuree, contre la meme scene en `gfx` ?
//   5. Que vaut le format de pixel de LICE, mesure et non suppose ?
//
// ⚠ CE N'EST PAS UNE FENETRE D'APPLICATION. Elle ne porte aucun widget, aucun
// etat, aucune disposition. Elle dessine une scene fixe dont chaque element
// existe pour rendre une des cinq reponses VISIBLE. Le jour ou le portage
// commence, elle disparait.
//
// ============================================================================
// LA DECISION QUI REND CE FICHIER POSSIBLE
// ============================================================================
// Le 2026-08-23, Cedric a tranche : on EXIGE la DLL. Pas de double dorsale,
// pas de mode degrade. Une fenetre portee qui ne trouve pas l'extension ne
// s'ouvre pas et l'ecrit. C'est ce qui permet d'ecrire ce code sans prevoir
// partout un repli vers `gfx`.
#ifndef CP_WIN_H
#define CP_WIN_H

namespace cp {
namespace win {

// L'instance du module, recue par le point d'entree. Une classe de fenetre
// Win32 s'enregistre POUR une instance : sans elle, `CreateWindowEx` echoue
// avec un code que personne ne pense a lire.
void set_instance(void* hinstance);

// Ouvre la fenetre, ou la remet au premier plan si elle existe deja.
// `dock` : 0 = fenetre flottante, 1 = tentative d'ancrage dans un docker.
//
// ⚠ LES DEUX NE SE CREENT PAS PAREIL. Un docker attend une fenetre ENFANT ; une
// fenetre flottante est de niveau superieur. On ne peut pas changer d'avis
// apres coup sans detruire et recreer, donc le choix se fait a l'ouverture.
// Le mode flottant est le defaut parce qu'il ne depend d'aucune API non
// documentee : si l'ancrage rate, on veut quand meme voir des pixels.
bool open(int dock);

// --- plusieurs fenetres ----------------------------------------------------
// ⚠ UNE POIGNEE, PAS UN POINTEUR. Lua ne doit jamais tenir une adresse : une
// bascule ancre <-> flottant recree le HWND, et un redemarrage de script
// rouvrirait sur une fenetre liberee. La poignee est un indice de case, verifie
// a chaque usage - au pire elle designe une fenetre fermee, et l'appel ne fait
// rien.
//
// `title` est ce que REAPER affiche sur l'onglet du docker ; `ident` est la cle
// sous laquelle il MEMORISE la position ancree. Deux fenetres qui partagent un
// `ident` se disputent la meme case et perdent leur disposition.
// ⚠ LA GEOMETRIE EST FACULTATIVE, ET ZERO VEUT DIRE << LAISSE WINDOWS
// DECIDER >>. `gfx.init` en porte une ; la perdre faisait naitre toutes les
// fenetres a 900x520, et un script qui restaure sa disposition sauvegardee la
// voyait ignoree sans un mot.
int  open_named(int dock, const char* title, const char* ident,
                int w = 0, int h = 0, int x = 0, int y = 0);
bool select_win(int handle);
int  current_win();
// Les frames posees dans ce HWND, -1 s'il n'est pas une de nos fenetres.
// Un AUTRE script s'en sert pour savoir qu'une fenetre a peint (ABI 3.6).
int  presented(void* hwnd);
void close_all();

void close();
bool is_open();
// La fenetre courante est-elle MONTREE ? Faux pour un onglet de docker
// inactif, un docker ferme, une fenetre dont la racine est reduite. Lua s'en
// sert pour ne plus dessiner ce que personne ne voit (ABI 3.4).
bool shown();
// La fenetre courante devient « sans clavier » : un clic ne lui donne ni le
// focus ni la revendication, et REAPER garde ses raccourcis (ABI 3.5).
void set_keyless(bool on);

// Une ligne d'etat lisible depuis Lua. Elle porte les REPONSES MESUREES :
// format de pixel, unite du row span, retournement, taille, cout de frame.
// C'est la sortie principale du prototype — plus que la fenetre elle-meme.
const char* diag();

// Appele au dechargement de l'extension, avant toute destruction.
void shutdown();

// ---------------------------------------------------------------------------
// LE DESSIN PILOTE DEPUIS LUA — c'est ici que l'architecture (b) commence
// ---------------------------------------------------------------------------
// Le modele : Lua mene la frame. Une boucle `defer` appelle `begin_frame`,
// emet ses ordres de dessin, puis `end_frame`. Le `WM_PAINT` de la fenetre ne
// fait plus que recopier la bitmap deja composee.
//
// ⚠ C'EST LE CONTRAIRE DE `gfx`, ET C'EST TOUT L'INTERET. Sous `gfx`, un bouton
// etiquete coute QUATORZE traversees Lua->C : une pose de couleur, trois
// dalles, quatre disques, une pose de couleur, un contour, une pose de couleur,
// deux ecritures de champ, une chaine. Ici, `draw_round_rect` en coute UNE et
// fait le meme travail — remplissage ET contour, geometrie identique.
//
// ⚠ LA FENETRE NE DESSINE JAMAIS RIEN D'ELLE-MEME. Avant le premier
// `begin_frame`, la liste est vide et le compositeur peint le fond par defaut ;
// la scene de test du prototype a ete retiree le 2026-09-30, avec les deux
// raccourcis clavier qu'elle gardait pour elle.
bool begin_frame(int* w, int* h);
void end_frame();

// Coordonnees EXACTES : le rendu arrondit les bords apres translation.
void draw_rect(double x, double y, double w, double h, unsigned int argb, int alpha);
void draw_round_rect(double x, double y, double w, double h, int radius,
                     unsigned int fill, int fill_a,
                     unsigned int stroke, int stroke_a);
void draw_text(double x, double y, const char* str, unsigned int argb, int size);

// Du texte dont le NOMBRE est evalue par la DLL a chaque composition. `tmpl`
// est un gabarit de printf a UNE conversion numerique ; tout le reste est
// refuse ici meme (un `%s` lirait un pointeur pris sur la pile, un `%n`
// ECRIRAIT en memoire, et ces chaines viennent de scripts).
//
// LA BOITE EST DONNEE, PAS MESUREE. `w`/`h` sont la place reservee par la mise
// en page : le calcul d endommagement doit connaitre la zone AVANT de savoir ce
// qu on va y ecrire, et Lua la connait puisque c est lui qui met en page.
//
// C ETAIT LA DERNIERE EXCEPTION. Un cadran montrait son anneau a 120 Hz et son
// << -6.0 dB >> a 30 : deux cadences dans le meme objet, ce qui se voit PLUS que
// 30 partout. On en avait conclu une frontiere du procede ; il n y en avait pas.
void draw_text_val(double x, double y, const char* tmpl, double value,
                   int w, int h, unsigned int argb, int size);

// L'entree. `buttons` : bit 0 = gauche, bit 1 = droit.
// ⚠ LE POINTEUR EST LU AU MOMENT DE L'APPEL, et quand il n'est pas sur la
// fenetre (ni tenu par un bouton) on rend -32768 sur les deux axes : rien n'est
// survole, et une boucle Lua au repos le reste. `gfx` rendait la vraie position
// hors fenetre ; la difference est voulue, et `show_menu` la rattrape.
void get_mouse(int* x, int* y, int* buttons);

// Le tick de REAPER en cours a-t-il ete demande par nous (voir `tick_boost`
// dans cp_win.cpp) ? Lu par `Core.Run` : une fenetre ne travaille sur un tick
// force que si elle a une entree, un mouvement ou un geste a servir.
bool tick_boosted();
// Depile une touche, ou 0 si la file est vide.
int pop_key();
// Rend a REAPER (section Main) la derniere touche depilee, que Lua n'a pas
// prise -- seulement pour une vue HEBERGEE et une touche remise par le
// crochet. `code` est celui que Lua a depile. 1 action lancee, 0 aucune liee,
// negatif rien a rendre. Voir `pass_key` dans cp_win.cpp.
int pass_key(int code);

// --- le decoupage ---------------------------------------------------------
// ⚠ IL S'INTERSECTE, IL NE SE REMPLACE PAS. Un panneau dans un panneau ne peut
// qu'etre plus PETIT que son parent : c'est ce qui rend `overflow: hidden`
// composable. `clip_reset` revient a la surface entiere.
bool clip_push(int x, int y, int w, int h);
void clip_pop();
void clip_reset();

// --- les formes ------------------------------------------------------------
void draw_circle(double cx, double cy, int radius, unsigned int argb, int alpha);
void draw_ring(double cx, double cy, double ri, double ro,
               double a0, double a1, unsigned int argb, int alpha);
void draw_line(double x0, double y0, double x1, double y1,
               double width, unsigned int argb, int alpha);

// Une polyligne s'accumule cote C++ puis se dessine EN UNE FOIS. C'est ce qui
// la rend une forme et non N segments : voir `draw_polyline` dans cp_raster2.
void poly_begin();
void poly_point(double x, double y);
void poly_draw(double width, unsigned int argb, int alpha);

// La forme d onde : meme accumulateur que la polyligne, meme raison (l ABI de
// ReaScript ne passe pas de tableau). `wave_point` empile un couple (min, max)
// dans [-1, 1] ; `wave_draw` consomme.
//
// UN ORDRE POUR TOUTE LA FORME. Le toolkit en emettait pres de 10 000, cuits
// dans une image hors ecran -- ce qui marche en regime etabli et s ecroule des
// qu on zoome ou qu on defile, parce que le tampon se recuit alors a chaque
// frame. En un ordre, une portee de deplacement peut la faire defiler a la
// cadence de l ecran sans que Lua repasse.
void wave_begin();
void wave_point(double lo, double hi);
void wave_draw(double x, double y, double w, double h,
               unsigned int argb, int alpha);

// La mesure du texte. Sans elle aucune mise en page n'est possible : c'est la
// premiere chose que `Layout` demandera.
bool measure_text(const char* str, int size, int* w, int* h);

// --- les liaisons ----------------------------------------------------------
// Elles s'attachent au DERNIER ordre enregistre. C'est la forme la plus simple
// qui marche depuis Lua : on dessine, puis on dit de quoi ce qu'on vient de
// dessiner depend. Aucun identifiant a inventer, aucune table a tenir.
bool bind_hover(int x, int y, int w, int h, unsigned int col_hover,
                int alpha_hover = 0);
// ⚠ `step` > 0 : la souris suit un ESCALIER, pas une droite. C'est ce qui
// deplafonne un geste MAGNETISE -- le cas par defaut du piano roll et de la vue
// d'onde -- et la condition (une grille uniforme en pixels) se verifie chez
// l'appelant. Voir `Bind::step`.
bool bind_mouse(int axis, int arg, double k, double c, double lo, double hi,
                double step = 0.0, double qphase = 0.0);

// L'index du prochain ordre. Sert a marquer le DEBUT d'une plage : on marque,
// on dessine plusieurs formes, puis on lie la plage entiere.
int  mark();
bool bind_rotate(int from, double cx, double cy, int axis,
                 double k, double c, double lo, double hi);
// La meme rotation, d'un angle borne(k*forme(shape, freq*t+phase)+c) : le point
// vif d'un cadran module par un LFO. Formes 0..4 seulement, comme bind_shape.
bool bind_rotate_shape(int from, double cx, double cy, int shape,
                       double freq, double phase,
                       double k, double c, double lo, double hi);

// ---------------------------------------------------------------------------
// LES PORTEES - une liaison sur une PLAGE d ordres
// ---------------------------------------------------------------------------
// TOUT CE QUI PRECEDE PATCHE UN ARGUMENT D UN ORDRE, et c est structurellement
// du petit a petit : il faut cabler chaque widget a la main, chaque cablage peut
// etre a moitie fait, et rien ne suit ni le texte ni la mise en page. Le
// symptome se voit - un potard a 120 Hz avec son etiquette figee a 30 - et il
// est PIRE que 30 partout, parce que l oeil compare deux choses cote a cote.
//
// Une portee lie une plage par ce que ses ordres ont en commun. Elle se pose au
// niveau du CONTENEUR : on n entoure pas la moitie d un widget.

// La teinte : accumuler les paires, puis fermer la portee. Meme procede que la
// polyligne, et pour la meme raison - l ABI de ReaScript ne passe pas de
// tableau. `bind_tint` CONSOMME les paires accumulees.
//
// UNE SUBSTITUTION, PAS UN DELTA. Le theme clair descend au survol, et trois
// themes livres ont des accents qui changent de teinte et pas de luminance : un
// delta unique serait faux pour la moitie du depot, et faux d une facon qu on ne
// verrait qu en changeant de theme.
void tint_pair(unsigned int from_col, unsigned int to_col);
bool bind_tint(int from, int x, int y, int w, int h, int inv);

// La translation. `arg` 0 = dx, 1 = dy ; `src` est un `cp::list::Src`.
// C EST CE QUI DEPLAFONNE LE DEFILEMENT, ET DONC TOUTES LES LISTES D UN COUP.
// Une liste qui defile ne se RECALCULE pas : elle se DEPLACE. Le texte ne change
// pas, seule sa position change - et une position se translate sans savoir ce
// qu on translate.
// ⚠ `mod` REBOUCLE LA PLAGE, et il vaut zero pour tout ce qui ne reboucle pas.
// Une affine bornee COLLE au bord ; un point de lecture, lui, revient a zero a
// la fin de la passe. Sans lui, une tete de lecture faite de plusieurs ordres
// restait au bord droit jusqu'a la passe suivante de Lua -- alors que la meme,
// en UN ordre, rebouclait depuis toujours par `bind_time`.
bool bind_move(int from, int arg, int src, int slot,
               double k, double c, double lo, double hi, double mod = 0.0,
               double step = 0.0, double qphase = 0.0);

// La visibilite. `inv` = 1 : cache quand la souris est DANS le rectangle.
// ELLE NE SAUTE JAMAIS UN ORDRE D ETAT (decoupe, cible, dimension) : en sauter
// un laisserait tout ce qui suit dessiner au mauvais endroit, et le defaut
// apparaitrait loin de sa cause.
bool bind_show(int from, int x, int y, int w, int h, int inv);
// La meme, avec ses portes (ABI 11) : `gate` 1 = l'horloge de la fenetre dans
// [t0, t1[, 2 = aucun bouton tenu, 4 = sans rectangle. Voir `Bind::gate`.
bool bind_show_gate(int from, int x, int y, int w, int h, int inv, int gate,
                    double t0, double t1);

// --- ce qui bouge sans la souris ------------------------------------------
// ⚠ ELLES N'ONT DE SENS QUE SI LE FIL DE REJEU TOURNE. Sans lui, rien ne
// reveille la fenetre entre deux frames Lua, et une liaison de temps n'avance
// qu'a 30 Hz — elle ne serait pas FAUSSE, elle serait inutile. La ligne d'etat
// dit « rejeu AUCUN » dans ce cas.
bool bind_time(int arg, double k, double c, double lo, double hi,
               double mod = 0.0);
bool bind_osc(int arg, double freq, double phase, double amp, double offset,
              double lo, double hi);
bool bind_value(int slot, int arg, double k, double c, double lo, double hi);

// ⚠ `tau_ms > 0` FAIT UN VU-METRE : attaque instantanee, descente
// exponentielle calculee a chaque composition. C'est ce qui rend un metre
// fluide a 120 Hz alors que sa DONNEE n'arrive qu'a 30 : ce qu'on percoit comme
// fluide est la descente, et elle est une fonction du temps, pas de la donnee.
// `mode` 0 = METRE (attaque instantanee, descente exponentielle vers zero),
// 1 = GLISSE (passe-bas symetrique vers la cible). Les confondre donne un
// lissage asymetrique : un defilement sauterait en descendant et ramperait vers
// le haut de la liste en remontant, parce que << vers zero >> n est pas << vers
// la valeur >>.
void set_value(int slot, double v, double tau_ms, int mode = 0);

// L'horloge de la fenetre courante, en secondes depuis son ouverture -- la
// meme que `kSrcTime` lit. C'est l'ANCRE d'une liaison de temps : sans elle,
// l'appelant ne peut pas poser la constante de son affine.
double now();

// La FORME d'un LFO evaluee par la DLL : `shapeValue(shape, freq*t + phase)`,
// puis l'affine bornee habituelle. `phase` est en CYCLES, et la source rend
// 0..1. Formes : 0 sinus, 1 triangle, 2 rampe montante, 3 rampe descendante,
// 4 carre. Voir cp_list.h pour pourquoi 5, 6 et 7 n'y sont pas.
bool bind_shape(int arg, int shape, double freq, double phase,
                double k, double c, double lo, double hi);

// ===========================================================================
// CE QUE `gfx` FOURNIT ET QU'IL FAUT REFOURNIR
// ===========================================================================
// Remplacer `gfx` ne veut pas dire remplacer son DESSIN : ca veut dire
// remplacer tout ce dont soixante-six scripts se servent. Le dessin en etait la
// moitie visible ; celle-ci est la moitie qui casse tout si elle manque.

// --- le dessin hors ecran --------------------------------------------------
// Cuit (ou retrouve) une police et rend son indice d'atlas, a passer ensuite
// comme argument `size` de `draw_text` et `measure_text`.
// `flags` : bit 0 = gras, bit 1 = italique.
int  set_font(const char* face, int size, int flags);

// La taille de la zone cliente, SANS ouvrir de frame.
//
// ⚠ C'EST TOUTE LA RAISON DE SON EXISTENCE. `begin_frame` la rend aussi, mais
// la demander par la ouvrirait une frame que le chemin de saut de `Core.Run` ne
// fermerait jamais : les ordres s'accumuleraient d'une frame sur l'autre
// jusqu'a saturer l'arene, et le symptome serait un dessin qui se degrade
// lentement sans qu'aucune erreur ne soit levee.
void client_size(int* w, int* h);

void set_dest(int img);
// Charge un PNG dans la case `img`. Rend false quand le fichier n'existe pas,
// ne se decode pas, ou que REAPER n'expose pas `LICE_LoadPNG`.
bool load_img(int img, const char* path);
// Le fournisseur de pixels donne a `cp::list::replay` (voir `ImgFn`). `ctx`
// est la fenetre dont la frame se rejoue : les PNG sont a elle.
const unsigned int* img_provider(void* ctx, int id, int* w, int* h);
void set_img_dim(int img, int w, int h);
void get_img_dim(int img, int* w, int* h);

// LES CASES VIDEO DE LA FENETRE COURANTE (ABI 4.1, cp_vsink.h). Une image
// `img` liee a une source (1 camera, 2 REAPER par le robinet, 3 image fixe) a
// la taille w x h : Lua la blitte comme une icone, un autre fil la remplit.
// ⚠ Ne jamais dimensionner (`set_img_dim`) une case liee : l'image cuite
// passerait devant la video, le blit lisant `imgs` avant le fournisseur.
// `video_bind` rend 0 ou un code negatif (-4 : pas de fenetre).
int  video_bind(int img, int source, int w, int h);
void video_unbind(int img);
// Rend false si `img` n'est pas une case de la fenetre courante. `out` :
// w, h, rx, ry, rw, rh, src_w, src_h, seq, source.
bool video_info(int img, double* out10);
// Charge une image fixe (chemin UTF-8) dans une case liee a la source 3.
// Rend un ticket (cp::cam::snap_status) ou un code negatif.
int  video_still(int img, const char* path_utf8);

void blit_img(int src, double sx, double sy, double sw, double sh,
              double dx, double dy, double dw, double dh,
              int alpha, int mode);
void mul_add(int x, int y, int w, int h,
             double mr, double mg, double mb, double ma,
             double ar, double ag, double ab, double aa);

// --- les formes que le toolkit reclamait ------------------------------------
void draw_triangle(double x1, double y1, double x2, double y2,
                   double x3, double y3, unsigned int col, int alpha);
void draw_grad_rect(int x, int y, int w, int h,
                    double r, double g, double b, double a,
                    double drdx, double dgdx, double dbdx, double dadx,
                    double drdy, double dgdy, double dbdy, double dady);
void draw_radial(double cx, double cy, double radius, unsigned int col,
                 int alpha, double falloff);
void draw_glow(double x, double y, double w, double h, double radius,
               double spread, unsigned int col, int alpha);

// --- l'entree ---------------------------------------------------------------
// ⚠ `mouse_cap` DOIT PORTER LES MODIFICATEURS, ET ILS NE VIENNENT PAS DES
// MESSAGES. Sans le focus clavier, `WM_MOUSEMOVE` ne dit rien de Ctrl/Shift ;
// le toolkit avait deja dû les relire a l'OS (`_mouse_cap_with_os_mods`).
// On les relit ici une bonne fois, avec les memes bits que `gfx` :
//   1 = gauche, 2 = droite, 4 = Ctrl, 8 = Shift, 16 = Alt, 64 = milieu
int  mouse_cap();
// Les boutons APPUYES depuis le dernier appel, et le nombre d'appuis du bouton
// gauche. ⚠ ELLE VIDE CE QU'ELLE REND. C'est ce qui rend un clic
// impossible a perdre quelle que soit la cadence de l'appelant -- `mouse_cap`,
// lui, ne dit que l'instant present.
// `dbl` (facultatif) : bit i = l'appui gauche i etait un double-clic, au sens
// de Windows et a l'instant de l'appui. Voir `note_left_press`.
int  mouse_latch(int* n, unsigned* dbl = nullptr);
// Le dernier appui et le dernier relachement GAUCHES (ABI 4.3) : la position
// du MESSAGE et un numero qui avance a chacun. Voir `note_release`.
void mouse_at(int* px, int* py, unsigned* pseq, int* ux, int* uy, unsigned* useq);
void get_wheel(double* wheel, double* hwheel);

// Dock : `-1` demande l'etat, sinon l'impose. Rend l'etat courant.
int  dock_query(int* x, int* y, int* w, int* h);
void dock_set(int dock);

// Un menu contextuel au format de `gfx.showmenu` : "a|b|>sous|x|<y|c", avec
// `#` pour grise et `!` pour coche. Rend l'indice 1-base, 0 si rien.
int  show_menu(const char* spec, int x, int y);

// La forme du pointeur. `which` est un identifiant de ressource Windows,
// `name` un curseur de REAPER (le dossier `Cursors` du chemin de ressources) ;
// le nom gagne quand il resout, l'identifiant est le repli.
void set_cursor(int which, const char* name);
void client_to_screen(int* x, int* y);
void screen_to_client(int* x, int* y);
bool pop_dropped_file(int index, char* out, int out_sz);
void clear_dropped_files();

// ⚠ VIDE L'ACCUMULATEUR. Deux appels dans la meme frame rendent la seconde fois
// zero : c'est un DEPLACEMENT depuis la derniere lecture, pas une position.
void mouse_delta(double* dx, double* dy);

} // namespace win
} // namespace cp

#endif // CP_WIN_H

