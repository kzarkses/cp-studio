"""La reference d'onnxruntime pour les tests de cp_tab : le VRAI modele de
Basic Pitch sur une fenetre d'un signal de test, ecrite dans
tests/fixtures/bp_window.bin.

    python make_fixture.py <chemin de nmp.onnx>

Le signal est fait d'operations EXACTES (des dents de scie a frequence entiere,
des enveloppes rationnelles) : Rust le refait au bit pres sans fichier
d'entree (`test_signal` dans src/tests.rs). Ce qui est garde : une trame sur 4
des trois sorties (43 trames), et la somme de chaque sortie entiere.

Le fichier : "CPBF", u32 trames gardees, puis pour chaque sortie (note,
onset, contour) : f64 somme totale, puis les trames gardees en f32.
"""
import os
import struct
import sys

import numpy as np
import onnxruntime as ort

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(HERE, "..", "tests", "fixtures", "bp_window.bin")
SR = 22050
N = 43844


def test_signal():
    """Trois notes en dents de scie qui se chevauchent (110, 165, 330 Hz)."""
    x = np.zeros(N, dtype=np.float64)
    i = np.arange(N, dtype=np.int64)
    for f, a, b, amp in ((110, 0, 11025, 0.3), (165, 8820, 26460, 0.25), (330, 22050, 41895, 0.2)):
        seg = (i >= a) & (i < b)
        ph = ((i * f) % SR).astype(np.float64) / SR          # exact
        saw = 2.0 * ph - 1.0
        env = 1.0 - (i - a).astype(np.float64) / (b - a)    # decroissance lineaire
        x += np.where(seg, amp * saw * env, 0.0)
    return x.astype(np.float32)


def main(argv):
    if len(argv) < 2:
        print(__doc__)
        return 2
    sess = ort.InferenceSession(argv[1], providers=["CPUExecutionProvider"])
    inp = sess.get_inputs()[0].name
    x = test_signal().reshape(1, N, 1)
    outs = {o.name: v for o, v in zip(sess.get_outputs(), sess.run(None, {inp: x}))}
    # Les noms de tf2onnx ne disent pas qui est qui : la note SONNE (grande
    # masse), l'onset est fait de pics (petite masse) ; le contour a 264 cases.
    two = [k for k, v in outs.items() if v.shape[-1] == 88]
    contour = [v for v in outs.values() if v.shape[-1] == 264][0][0]
    a, b = outs[two[0]][0], outs[two[1]][0]
    note, onset = (a, b) if a.sum() > b.sum() else (b, a)
    keep = list(range(0, 172, 4))
    buf = bytearray(b"CPBF") + struct.pack("<I", len(keep))
    for m in (note, onset, contour):
        buf += struct.pack("<d", float(m.astype(np.float64).sum()))
        buf += np.asarray(m[keep], dtype="<f4").tobytes()
    os.makedirs(os.path.dirname(OUT), exist_ok=True)
    with open(OUT, "wb") as f:
        f.write(buf)
    print("note somme %.4f max %.4f | onset somme %.4f max %.4f | contour somme %.4f"
          % (note.sum(), note.max(), onset.sum(), onset.max(), contour.sum()))
    print("%s : %d octets" % (os.path.normpath(OUT), len(buf)))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
