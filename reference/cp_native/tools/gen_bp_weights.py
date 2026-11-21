"""Genere src/tab/cp_bp_weights.h depuis le modele ONNX de Basic Pitch.

Basic Pitch (Spotify, ICASSP 2022, licence Apache-2.0) :
    https://github.com/spotify/basic-pitch
    basic_pitch/saved_models/icassp_2022/nmp.onnx

UN OUTIL A USAGE UNIQUE, PAS UN HARNAIS. L'en-tete genere EST la source : on ne
le regenere que si le modele change. Il demande `onnx` et `numpy`, que le reste
de l'outillage n'exige pas -- c'est pour ca qu'il ne s'appelle pas *_test.py
(check.py le lancerait, et il manquerait sur une machine neuve).

    python gen_bp_weights.py <chemin de nmp.onnx> [sortie]

Ce qui est extrait : les six convolutions du reseau (la normalisation par lot y
est deja fondue par la conversion TF -> ONNX) et la normalisation par lot qui
suit le CQT (un produit et une somme). Ce qui ne l'est PAS : les noyaux du CQT,
le filtre passe-bas et les longueurs, que `cp_bp_model.cpp` CALCULE (les
formules de nnAudio et de scipy.signal.firwin2) -- 18 700 nombres de moins dans
le depot, et `tab_test` confronte le calcul aux constantes du modele par leurs
sommes (voir kRefCqt dans src/test/tab_test.cpp).
"""
import os
import sys

import numpy as np
import onnx
from onnx import numpy_helper

HERE = os.path.dirname(os.path.abspath(__file__))
DEFAULT_OUT = os.path.join(HERE, "..", "src", "tab", "cp_bp_weights.h")

# role -> forme du noyau (sortie, entree, hauteur = temps, largeur = frequence)
ROLES = [
    ("kContour1", [8, 8, 3, 39]),
    ("kContour2", [1, 8, 5, 5]),
    ("kNote1", [32, 1, 7, 7]),
    ("kNote2", [1, 32, 7, 3]),
    ("kOnset1", [32, 8, 5, 5]),
    ("kOnset2", [1, 33, 3, 3]),
]


def fmt(v):
    # %.9g fait l'aller-retour exact d'un float32.
    s = "%.9g" % float(v)
    if "e" not in s and "." not in s and "n" not in s:
        s += ".0"
    return s + "f"


def emit(out, name, arr):
    flat = np.asarray(arr, dtype=np.float32).ravel()
    out.append("static const float %s[%d] = {" % (name, flat.size))
    line = "    "
    for i, v in enumerate(flat):
        piece = fmt(v) + ","
        if len(line) + len(piece) + 1 > 100:
            out.append(line.rstrip())
            line = "    "
        line += piece + " "
    out.append(line.rstrip())
    out.append("};")


def main(argv):
    if len(argv) < 2:
        print(__doc__)
        return 2
    src = argv[1]
    dst = argv[2] if len(argv) > 2 else DEFAULT_OUT
    model = onnx.load(src)
    g = model.graph
    inits = {i.name: numpy_helper.to_array(i) for i in g.initializer}

    found = {}
    for n in g.node:
        if n.op_type != "Conv":
            continue
        w = inits.get(n.input[1])
        if w is None:
            continue
        for role, shape in ROLES:
            if list(w.shape) == shape:
                if role in found:
                    raise SystemExit("deux convolutions de forme %s" % shape)
                found[role] = (w, inits[n.input[2]], dict(
                    (a.name, onnx.helper.get_attribute_value(a)) for a in n.attribute))
    missing = [r for r, _ in ROLES if r not in found]
    if missing:
        raise SystemExit("convolutions introuvables : %s" % missing)

    # La normalisation par lot du CQT : Mul puis Add par un scalaire, juste
    # apres la mise a l'echelle logarithmique.
    bn_mul = bn_add = None
    nodes = list(g.node)
    for i, n in enumerate(nodes):
        if n.op_type == "Mul" and n.input[1] in inits and inits[n.input[1]].shape == (1,):
            nxt = nodes[i + 1] if i + 1 < len(nodes) else None
            if nxt is not None and nxt.op_type == "Add" and nxt.input[1] in inits:
                bn_mul = float(inits[n.input[1]][0])
                bn_add = float(inits[nxt.input[1]][0])
    if bn_mul is None:
        raise SystemExit("normalisation du CQT introuvable")

    out = []
    out.append("// GENERE par CP_Native/tools/gen_bp_weights.py -- ne pas editer a la main.")
    out.append("//")
    out.append("// Les poids du reseau de Basic Pitch (Spotify, ICASSP 2022), extraits de")
    out.append("// basic_pitch/saved_models/icassp_2022/nmp.onnx. Licence Apache-2.0 :")
    out.append("// https://github.com/spotify/basic-pitch/blob/main/LICENSE")
    out.append("//")
    out.append("// Disposition ONNX : [sortie][entree][temps][frequence], biais par sortie.")
    out.append("// La normalisation par lot de chaque couche y est deja fondue.")
    out.append("#pragma once")
    out.append("")
    out.append("namespace cp { namespace bp {")
    out.append("")
    out.append("// Normalisation par lot apres le CQT logarithmique : x * kBnMul + kBnAdd.")
    out.append("static const float kBnMul = %s;" % fmt(bn_mul))
    out.append("static const float kBnAdd = %s;" % fmt(bn_add))
    total = 0
    for role, shape in ROLES:
        w, b, attrs = found[role]
        out.append("")
        out.append("// %s : %s, pas %s, marges %s" % (
            role, "x".join(str(s) for s in shape), attrs.get("strides"), attrs.get("pads")))
        emit(out, role + "W", w)
        emit(out, role + "B", b)
        total += w.size + b.size
    out.append("")
    out.append("}} // namespace cp::bp")
    out.append("")
    with open(dst, "w", newline="\n") as f:
        f.write("\n".join(out))
    print("%s : %d poids" % (os.path.normpath(dst), total + 2))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))

