// Bouchon des lanes pour l'oracle.
//
// cp_lanes.cpp a ete recousu de lectures partielles et sa fin manque : il ne
// compile pas. L'Engine n'appelle que ces quatre fonctions (plus `pop_vcmd`,
// en ligne dans cp_lanes.h, qui ne lit que des anneaux construits par leur
// propre constructeur). Aucune lane n'est active dans les tests de l'oracle :
// le vrai code n'y poserait aucune commande de voix, et ce bouchon ne change
// donc rien au son compare.
//
// Les lanes seront portees (et comparees) a part, quand cp_lanes.cpp aura ete
// reconstitue en entier.
#include "../cp_native/src/core/cp_lanes.h"

namespace cp {

Lane::Lane() {}
Lanes::Lanes() {}
Lanes::~Lanes() {}
void Lanes::init(double srate) { set_srate(srate); }
void Lanes::tick(frame_t, int) {}

}  // namespace cp
