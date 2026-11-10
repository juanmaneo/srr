"""Volume cues for a flat sprite: shade it as if it were inflated, before it goes to TRELLIS.

TRELLIS reads a flat cel drawing as a poster and returns a thin standee. A drawing that is
lit like a rounded object (darker edges, a light from the upper left) is read as a solid
figure far more often. The height of the "balloon" is the square root of the distance to the
silhouette, so every limb, strand of hair and fold is a tube of its own.
"""
from __future__ import annotations

import numpy as np
from PIL import Image
from scipy.ndimage import distance_transform_edt, gaussian_filter

LIGHT = np.array([-0.45, -0.55, 0.70])  # towards the light: from the upper left, in front of the figure
LIGHT = LIGHT / np.linalg.norm(LIGHT)


def inflate(image: Image.Image, strength: float = 0.55, relief: float = 6.0) -> Image.Image:
    """`image` (RGBA) shaded as an inflated figure; `strength` 0 leaves it as drawn."""
    rgba = np.asarray(image.convert("RGBA")).astype(np.float32)
    mask = rgba[..., 3] > 128
    height = np.sqrt(distance_transform_edt(mask)) * relief
    height = gaussian_filter(height, 1.5)
    gy, gx = np.gradient(height)
    normal = np.stack([-gx, -gy, np.ones_like(height)], axis=-1)
    normal /= np.linalg.norm(normal, axis=-1, keepdims=True)
    lambert = np.clip(normal @ LIGHT, 0.0, 1.0)
    # 0.45 (facing away) .. 1.15 (facing the light), 1 meaning "as drawn" on a flat interior.
    shade = 0.45 + 0.7 * lambert
    shade = 1.0 + strength * (shade - 1.0)
    rgba[..., :3] = np.clip(rgba[..., :3] * shade[..., None], 0, 255)
    return Image.fromarray(rgba.astype(np.uint8), "RGBA")
