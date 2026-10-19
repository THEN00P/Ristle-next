"""Generates the 512 artwork JPEGs (600x600, warm gradients with grain and
shapes, so decoding costs about what real covers cost). Deterministic."""
import sys, pathlib
import numpy as np
from PIL import Image, ImageDraw, ImageFilter

out = pathlib.Path(sys.argv[1]); out.mkdir(parents=True, exist_ok=True)
rng = np.random.default_rng(7)
yy, xx = np.mgrid[0:600, 0:600] / 600.0
for n in range(512):
    c1 = rng.integers(20, 255, 3); c2 = rng.integers(0, 200, 3)
    a = rng.uniform(0, 6.28)
    t = (np.cos(a) * xx + np.sin(a) * yy); t = (t - t.min()) / (t.max() - t.min())
    img = (c1[None, None, :] * (1 - t[..., None]) + c2[None, None, :] * t[..., None])
    img += rng.normal(0, 14, img.shape)
    im = Image.fromarray(np.clip(img, 0, 255).astype(np.uint8))
    d = ImageDraw.Draw(im)
    for _ in range(int(rng.integers(3, 9))):
        x, y, r = rng.integers(0, 600, 3) // np.array([1, 1, 3])
        col = tuple(int(v) for v in rng.integers(0, 255, 3))
        d.ellipse([x - r, y - r, x + r, y + r], outline=col, width=int(rng.integers(2, 14)))
    im = im.filter(ImageFilter.GaussianBlur(0.6))
    im.save(out / f"{n:03d}.jpg", quality=88)
print("ok", len(list(out.glob("*.jpg"))))
