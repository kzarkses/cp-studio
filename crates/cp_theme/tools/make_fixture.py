"""Fabrique tests/fixtures/theme.zip : un faux theme de REAPER dont on connait
chaque pixel, pour verifier le lecteur Rust (zip, DEFLATE, PNG) sans dependre
du theme de quiconque.

PNG ecrits a la main (types 0, 2, 3, 4, 6 ; profondeurs 1, 4, 8, 16 ; tRNS ;
Adam7), chaque ligne avec un filtre different (0 a 4), et a cote les pixels
attendus en RGBA droit (expect/<nom>.rgba). Lancer depuis n'importe ou :
    python tools/make_fixture.py
"""

import os
import struct
import zipfile
import zlib

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(HERE, "..", "tests", "fixtures", "theme.zip")


def chunk(kind, body):
    c = struct.pack(">I", len(body)) + kind + body
    return c + struct.pack(">I", zlib.crc32(kind + body) & 0xFFFFFFFF)


def paeth(a, b, c):
    p = a + b - c
    pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
    if pa <= pb and pa <= pc:
        return a
    return b if pb <= pc else c


def filt(rows, bpp):
    out = b""
    prev = bytes(len(rows[0])) if rows else b""
    for y, row in enumerate(rows):
        f = y % 5
        line = bytearray()
        for i, x in enumerate(row):
            a = row[i - bpp] if i >= bpp else 0
            b = prev[i]
            c = prev[i - bpp] if i >= bpp else 0
            pred = [0, a, b, (a + b) // 2, paeth(a, b, c)][f]
            line.append((x - pred) & 0xFF)
        out += bytes([f]) + bytes(line)
        prev = row
    return out


def pack_row(samples, depth):
    """Echantillons entiers vers octets (MSB d'abord sous 8 bits)."""
    if depth == 8:
        return bytes(samples)
    if depth == 16:
        return b"".join(struct.pack(">H", s) for s in samples)
    out, acc, n = bytearray(), 0, 0
    for s in samples:
        acc = (acc << depth) | s
        n += depth
        if n == 8:
            out.append(acc)
            acc, n = 0, 0
    if n:
        out.append(acc << (8 - n))
    return bytes(out)


def png(w, h, depth, color, samples_at, plte=None, trns=None, interlace=0):
    """samples_at(x, y) -> liste d'echantillons du pixel."""
    ch = {0: 1, 2: 3, 3: 1, 4: 2, 6: 4}[color]
    bpp = max(1, ch * depth // 8)
    if interlace:
        passes = [(0, 0, 8, 8), (4, 0, 8, 8), (0, 4, 4, 8), (2, 0, 4, 4),
                  (0, 2, 2, 4), (1, 0, 2, 2), (0, 1, 1, 2)]
    else:
        passes = [(0, 0, 1, 1)]
    raw = b""
    for x0, y0, dx, dy in passes:
        xs = list(range(x0, w, dx))
        ys = list(range(y0, h, dy))
        if not xs or not ys:
            continue
        rows = [pack_row([s for x in xs for s in samples_at(x, y)], depth) for y in ys]
        raw += filt(rows, bpp)
    data = b"\x89PNG\r\n\x1a\n"
    data += chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, depth, color, 0, 0, interlace))
    if plte:
        data += chunk(b"PLTE", b"".join(bytes(c) for c in plte))
    if trns is not None:
        data += chunk(b"tRNS", trns)
    data += chunk(b"IDAT", zlib.compress(raw, 9))
    data += chunk(b"IEND", b"")
    return data


def main():
    files = {}
    expect = {}

    def add(name, w, h, data, rgba_at):
        files["Img/" + name + ".png"] = data
        expect[name] = bytes(v for y in range(h) for x in range(w) for v in rgba_at(x, y))

    # RGBA 8 bits, a l'echelle 1 et 2.
    f = lambda x, y: [(x * 40 + y * 7) % 256, (y * 60 + 3) % 256, (x * y * 13) % 256, (x * 50 + y * 30) % 256]
    add("a", 5, 3, png(5, 3, 8, 6, f), f)
    files["Img/200/a.png"] = png(10, 6, 8, 6, lambda x, y: f(x // 2, y // 2))
    expect["a@2"] = bytes(v for y in range(6) for x in range(10) for v in f(x // 2, y // 2))

    # Palette sur 4 bits, avec transparence par entree.
    plte = [(i * 16, 255 - i * 16, (i * 37) % 256) for i in range(16)]
    alpha = bytes((i * 17) % 256 for i in range(10))  # les 6 dernieres restent opaques
    idx = lambda x, y: (x * 3 + y * 5) % 16
    add("pal4", 7, 5, png(7, 5, 4, 3, lambda x, y: [idx(x, y)], plte=plte, trns=alpha),
        lambda x, y: list(plte[idx(x, y)]) + [alpha[idx(x, y)] if idx(x, y) < len(alpha) else 255])

    # Gris sur 1 bit.
    g1 = lambda x, y: (x + y) % 2
    add("gray1", 9, 2, png(9, 2, 1, 0, lambda x, y: [g1(x, y)]),
        lambda x, y: [g1(x, y) * 255] * 3 + [255])

    # RGB 16 bits, une couleur transparente.
    c16 = lambda x, y: [x * 20000 + 1, y * 30000 + 2, (x + y) * 9000 + 3]
    key = c16(1, 1)
    add("rgb16", 3, 3, png(3, 3, 16, 2, c16, trns=struct.pack(">HHH", *key)),
        lambda x, y: [v >> 8 for v in c16(x, y)] + [0 if c16(x, y) == key else 255])

    # Gris et alpha, 8 bits.
    ga = lambda x, y: [(x * 70) % 256, (y * 80 + 10) % 256]
    add("ga8", 4, 4, png(4, 4, 8, 4, ga), lambda x, y: [ga(x, y)[0]] * 3 + [ga(x, y)[1]])

    # RGBA entrelace (Adam7), taille qui ne tombe pas juste.
    fi = lambda x, y: [(x * 23) % 256, (y * 29) % 256, (x * y) % 256, 255 - (x + y) * 5]
    add("inter", 11, 9, png(11, 9, 8, 6, fi, interlace=1), fi)

    # Une image a bord rose : 8x6, bords gauche 2, haut 1, droit 3, bas 2
    # (dans l'image interieure de 6x4).
    P = [255, 0, 255, 255]
    def nine(x, y):
        w, h = 8, 6
        if y == 0:
            return P if x <= 2 or x == w - 1 else [0, 0, 0, 0]
        if x == 0:
            return P if y <= 1 or y == h - 1 else [0, 0, 0, 0]
        if y == h - 1:
            return P if x >= w - 1 - 3 else [0, 0, 0, 0]
        if x == w - 1:
            return P if y >= h - 1 - 2 else [0, 0, 0, 0]
        return [x * 30, y * 40, 100, 255]
    add("pink", 8, 6, png(8, 6, 8, 6, nine), nine)

    theme_ini = (
        "[color theme]\r\n"
        "col_main_bg2=4079166\r\n"
        "trans_font=E9FFFFFF000000000000000000000000BC0200000000000003020102526F626F746F00\r\n"
        "[REAPER]\r\n"
        "ui_img=Img\r\n"
    )
    big = ("".join("ligne %d : le transport de CP Studio %s\n" % (i, "x" * (i % 17)) for i in range(4000))).encode()
    with zipfile.ZipFile(OUT, "w") as z:
        z.writestr("Theme.ReaperTheme", theme_ini, compress_type=zipfile.ZIP_DEFLATED)
        z.writestr("big.txt", big, compress_type=zipfile.ZIP_DEFLATED)
        z.writestr("tiny.txt", b"hello hello hello", compress_type=zipfile.ZIP_DEFLATED)
        z.writestr("stored.bin", bytes(range(256)), compress_type=zipfile.ZIP_STORED)
        # DEFLATE niveau 0 : des blocs « stockes » (plus de 64 Ko : deux blocs).
        z.writestr("level0.txt", big[:70000], compress_type=zipfile.ZIP_DEFLATED, compresslevel=0)
        for n, d in files.items():
            z.writestr(n, d, compress_type=zipfile.ZIP_DEFLATED)
        for n, d in expect.items():
            z.writestr("expect/" + n + ".rgba", d, compress_type=zipfile.ZIP_DEFLATED)
    print("ecrit", OUT, os.path.getsize(OUT), "octets")


if __name__ == "__main__":
    main()
