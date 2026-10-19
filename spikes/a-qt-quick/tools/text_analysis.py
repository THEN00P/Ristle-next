"""Scenario 5: text sharpness from duplicated-frame crops.

For each line of Segoe UI text (12/13/14 px, dark band and white band) in the
Qt specimen (three render types) and the native references (DirectWrite
ClearType, DirectWrite grayscale, GDI ClearType) it measures:

  acutance   mean |horizontal luminance gradient| over ink pixels, divided
             by the band's text/background contrast (higher = crisper edges)
  solid      share of ink pixels within 25 % of full text contrast (stems
             that reach full ink rather than grey smear)
  chroma     mean colour fringing |R-G|+|G-B| over ink pixels (ClearType > 0)
  width      ink width of the line in px (hinting/advance differences)

and writes a ×4 nearest-neighbour comparison sheet.
Usage: text_analysis.py <tag>"""
import json, sys
import numpy as np
from PIL import Image
from spikelib import RUNS, ROOT

QT_COLS = ["QtRendering", "NativeRendering", "CurveRendering"]


def lines(img, x0, x1, y0, y1, bg):
    """Text lines (y_start, y_end, x_start, x_end) in a region, by ink rows."""
    reg = img[y0:y1, x0:x1].astype(int)
    ink = np.abs(reg - bg).sum(axis=2) > 24
    rows = ink.any(axis=1)
    out, start = [], None
    for y, v in enumerate(list(rows) + [False]):
        if v and start is None:
            start = y
        elif not v and start is not None:
            cols = np.where(ink[start:y].any(axis=0))[0]
            out.append((y0 + start, y0 + y, x0 + cols[0], x0 + cols[-1] + 1))
            start = None
    return out


def metrics(img, box, bg, fg):
    y0, y1, x0, x1 = box
    reg = img[y0:y1, x0:x1].astype(float)
    lum = reg @ [0.2126, 0.7152, 0.0722]
    bgl, fgl = np.dot(bg, [0.2126, 0.7152, 0.0722]), np.dot(fg, [0.2126, 0.7152, 0.0722])
    contrast = abs(fgl - bgl)
    t = np.clip((lum - bgl) / (fgl - bgl), 0, 1)       # 0 background .. 1 full ink
    ink = t > 0.1
    gx = np.abs(np.diff(lum, axis=1)) / contrast
    chroma = (np.abs(reg[..., 0] - reg[..., 1]) + np.abs(reg[..., 1] - reg[..., 2]))
    return {"acutance": round(float(gx[ink[:, 1:]].mean()), 3),
            "solid": round(float((t[ink] > 0.75).mean()), 3),
            "chroma": round(float(chroma[ink].mean()), 2),
            "width": int(x1 - x0), "height": int(y1 - y0)}


def analyse(tag):
    k = json.load(open(RUNS / f"{tag}_text_qt" / "meta.json"))["scale"]
    P = lambda v: round(v * k)
    sources = {"qt": RUNS / f"{tag}_text_qt" / "dupcap_snap1200.png"}
    for m in ("cleartype", "grayscale", "gdi"):
        sources[m] = RUNS / f"{tag}_text_native_{m}" / "dupcap_snap1200.png"
    imgs = {n: np.array(Image.open(p).convert("RGB")) for n, p in sources.items()}
    bands = [((0x1b, 0x17, 0x12), (0xf5, 0xf0, 0xe8), 0), ((255, 255, 255), (0, 0, 0), 360)]
    results, crops = [], []
    for bg, fg, yb in bands:
        for name, img in imgs.items():
            for col in range(3):
                x0, x1 = P(24 + col * 410), P(24 + col * 410 + 400)
                ls = lines(img, x0, x1, P(yb + 10), P(yb + 200), np.array(bg))
                if name == "qt":
                    ls = ls[1:]  # skip the render-type label
                label = f"Qt {QT_COLS[col]}" if name == "qt" else {"cleartype": "DirectWrite ClearType",
                        "grayscale": "DirectWrite grayscale", "gdi": "GDI ClearType"}[name]
                if name != "qt" and col > 0:
                    continue  # the reference draws the same thing three times
                for size, box in zip((12, 13, 14), ls[:3]):
                    r = {"renderer": label, "band": "dark" if yb == 0 else "white", "px": size, **metrics(img, box, bg, fg)}
                    results.append(r)
                    if size == 13:
                        y0, y1, xa, xb = box
                        crops.append((label, r["band"], img[y0 - 2:y1 + 2, xa - 2:xa + P(170)]))
    sheet(crops, ROOT / "results" / f"text_{tag}.png")
    with open(ROOT / "results" / "text.jsonl", "a") as f:
        for r in results:
            print(json.dumps({"tag": tag, "scale": k, **r}), file=f)
    return results


def sheet(crops, path, zoom=4):
    h = sum(c[2].shape[0] * zoom + 6 for c in crops)
    w = max(c[2].shape[1] for c in crops) * zoom
    out = Image.new("RGB", (w, h), (128, 128, 128))
    y = 0
    for _, _, c in crops:
        im = Image.fromarray(c).resize((c.shape[1] * zoom, c.shape[0] * zoom), Image.NEAREST)
        out.paste(im, (0, y))
        y += im.height + 6
    out.save(path)
    with open(str(path) + ".txt", "w") as f:
        f.write("\n".join(f"{l} ({b} band)" for l, b, _ in crops) + "\n")


if __name__ == "__main__":
    for r in analyse(sys.argv[1]):
        print(r)
