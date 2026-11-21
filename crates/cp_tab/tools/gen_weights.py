"""Extrait les poids du reseau de Basic Pitch dans src/bp_weights.bin.

Basic Pitch (Spotify, ICASSP 2022, licence Apache-2.0) :
    https://github.com/spotify/basic-pitch
    basic_pitch/saved_models/icassp_2022/nmp.onnx

UN OUTIL A USAGE UNIQUE, PAS UN HARNAIS : le fichier genere EST la source, on
ne le regenere que si le modele change. Il demande `onnx` et `numpy` (un venv
a part suffit : rien d'autre dans le depot ne les exige).

    python gen_weights.py <chemin de nmp.onnx>

Le fichier : "CPBW", u32 version (1), puis des f32 little-endian dans l'ordre
de ROLES : bn_mul, bn_add, puis pour chaque convolution ses poids
[sortie][entree][temps][frequence] et ses biais. La normalisation par lot de
chaque couche y est deja fondue par la conversion TF -> ONNX. Les noyaux du CQT
et le filtre passe-bas ne sont PAS extraits : `model.rs` les CALCULE (les
formules de nnAudio et de scipy.signal.firwin2).
"""
import os
import struct
import sys

import numpy as np
import onnx
from onnx import numpy_helper

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(HERE, "..", "src", "bp_weights.bin")

# role -> forme du noyau (sortie, entree, hauteur = temps, largeur = frequence)
ROLES = [
    ("contour1", [8, 8, 3, 39]),
    ("contour2", [1, 8, 5, 5]),
    ("note1", [32, 1, 7, 7]),
    ("note2", [1, 32, 7, 3]),
    ("onset1", [32, 8, 5, 5]),
    ("onset2", [1, 33, 3, 3]),
]


def main(argv):
    if len(argv) < 2:
        print(__doc__)
        return 2
    model = onnx.load(argv[1])
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
                attrs = dict((a.name, onnx.helper.get_attribute_value(a)) for a in n.attribute)
                found[role] = (w, inits[n.input[2]], attrs)
    missing = [r for r, _ in ROLES if r not in found]
    if missing:
        raise SystemExit("convolutions introuvables : %s" % missing)
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
    out = bytearray(b"CPBW") + struct.pack("<I", 1)
    out += np.array([bn_mul, bn_add], dtype="<f4").tobytes()
    total = 2
    for role, shape in ROLES:
        w, b, attrs = found[role]
        print("%-9s %-12s pas %s marges %s" % (role, "x".join(map(str, shape)),
                                              attrs.get("strides"), attrs.get("pads")))
        out += np.asarray(w, dtype="<f4").ravel().tobytes()
        out += np.asarray(b, dtype="<f4").ravel().tobytes()
        total += w.size + b.size
    with open(OUT, "wb") as f:
        f.write(out)
    print("%s : %d poids, %d octets" % (os.path.normpath(OUT), total, len(out)))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
