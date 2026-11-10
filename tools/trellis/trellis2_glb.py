#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.10"
# dependencies = ["gradio_client>=1.3", "pillow", "numpy"]
# ///
"""Turn one 2D image into a textured 3D .glb with the public TRELLIS.2 Hugging Face Space.

    uv run tools/trellis/trellis2_glb.py input.png out.glb [--seed 0] [--resolution 512] [--use-token]

Flow (endpoints of microsoft/TRELLIS.2, checked with Client.view_api()):
    /start_session      create the per-session scratch dir on the Space
    /preprocess_image   crop to a square around the alpha, composite on black
    /image_to_3d        GPU call 1: the shape and the material, returns 48 preview renders (HTML);
                        the latents stay in the *session* of the client, not with us
    /extract_glb        GPU call 2: the GLB (100k+ triangles, WebP PBR textures)

Each GPU call costs ZeroGPU quota, which is scarce, so the clay renders of call 1 are checked
first (the silhouette seen from eight sides): a flat figure (TRELLIS reads a cel drawing as a poster) stops here with exit code 4 and
the second call is never made. `tools/trellis/farm.py` decimates what this returns.
"""
from __future__ import annotations

import argparse
import base64
import io
import re
import shutil
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from trellis_glb import as_path, check_space, die, explain, log, run_job  # noqa: E402

DEFAULT_SPACE = "microsoft/TRELLIS.2"
FLAT = 4  # exit code: the preview shows a billboard, no GLB was extracted
# The 8 views of a preview turn about the figure in 45 degree steps; the modes are, in the
# order of the Space: normal, clay, base colour, three HDRI shaded.
NORMAL, CLAY, SHADED, VIEWS = 0, 1, 3, 8
VIEW_RE = re.compile(r'id="view-m(\d+)-s(\d+)"[^>]*?src="data:image/\w+;base64,([^"]+)"', re.S)


def views(html: str) -> dict[tuple[int, int], "Image.Image"]:
    from PIL import Image

    out = {}
    for mode, step, data in VIEW_RE.findall(html):
        out[(int(mode), int(step))] = Image.open(io.BytesIO(base64.b64decode(data))).convert("RGB")
    return out


def silhouette_box(image) -> tuple[float, float]:
    """Width and height (pixels) of what is not the backdrop of a render: the colour of the
    four corners (black on the Space today), whatever it is."""
    import numpy as np

    pixels = np.asarray(image.convert("RGB")).astype(np.int16)
    corners = np.stack([pixels[0, 0], pixels[0, -1], pixels[-1, 0], pixels[-1, -1]])
    lit = np.abs(pixels - np.median(corners, axis=0)).max(axis=2) > 24
    if not lit.any():
        return 0.0, 0.0
    columns, rows = np.where(lit.any(axis=0))[0], np.where(lit.any(axis=1))[0]
    return float(columns[-1] - columns[0] + 1), float(rows[-1] - rows[0] + 1)


def depth_ratio(widths: list[float], height: float) -> float:
    """Depth of the figure over its height, from the widths of its silhouette seen at `VIEWS`
    angles: a box of width w and depth t seen turned by a looks w|cos a| + t|sin a| wide;
    fit w, t >= 0 and the unknown turn of the figure by least squares. A turn of 90 degrees
    swaps w and t, so the smaller of the two is the depth (a figure is wider than it is deep)."""
    import numpy as np

    steps = np.arange(len(widths)) * (2 * np.pi / len(widths))
    best = None
    for degrees in range(0, 180):
        a = steps - np.radians(degrees)
        basis = np.stack([np.abs(np.cos(a)), np.abs(np.sin(a))], axis=1)
        solution, *_ = np.linalg.lstsq(basis, np.array(widths), rcond=None)
        solution = np.maximum(solution, 0.0)
        error = float(((basis @ solution - widths) ** 2).sum())
        if best is None or error < best[0]:
            best = (error, solution)
    return float(best[1].min()) / max(height, 1.0)


def contact_sheet(images: dict, mode: int, path: Path) -> None:
    from PIL import Image

    tiles = [images[(mode, step)].resize((256, 256)) for step in range(VIEWS) if (mode, step) in images]
    if not tiles:
        return
    sheet = Image.new("RGB", (256 * 4, 256 * ((len(tiles) + 3) // 4)))
    for i, tile in enumerate(tiles):
        sheet.paste(tile, ((i % 4) * 256, (i // 4) * 256))
    sheet.save(path, quality=88)


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("image", type=Path, help="input image (RGBA PNG with transparent background is best)")
    ap.add_argument("output", type=Path, help="output .glb path")
    ap.add_argument("--space", default=DEFAULT_SPACE)
    ap.add_argument("--seed", type=int, default=0)
    ap.add_argument("--resolution", default="512", choices=("512", "1024", "1536"),
                    help="512 is the cheapest in GPU seconds (the shape decision does not depend on it)")
    ap.add_argument("--texture-size", type=int, default=1024, choices=(1024, 2048, 3072, 4096))
    ap.add_argument("--decimation", type=int, default=100000, help="face target of the Space (100k-500k)")
    ap.add_argument("--min-depth", type=float, default=0.09,
                    help="depth/height under which the preview counts as a billboard (default 0.09)")
    ap.add_argument("--preview", type=Path, help="prefix: write <prefix>_clay.jpg and <prefix>_shaded.jpg")
    ap.add_argument("--timeout", type=float, default=1800)
    ap.add_argument("--use-token", action="store_true",
                    help="authenticate with the locally stored Hugging Face token (default: anonymous)")
    a = ap.parse_args()
    if not a.image.is_file():
        die(f"input image not found: {a.image}")

    token = None if a.use_token else False
    log("auth: " + ("Hugging Face account (standard token lookup)" if a.use_token else "anonymous"))
    from gradio_client import Client, handle_file

    check_space(a.space, token, a.timeout)
    t0 = time.monotonic()
    try:
        client = Client(a.space, token=token, verbose=False)
        client.predict(api_name="/start_session")
        log("preprocessing image (/preprocess_image)...")
        pre = as_path(run_job(client, "preprocess", a.timeout, handle_file(str(a.image)), api_name="/preprocess_image"))

        log(f"shape and material (/image_to_3d): seed={a.seed} resolution={a.resolution}")
        t1 = time.monotonic()
        html = run_job(client, "image_to_3d", a.timeout, handle_file(pre), a.seed, a.resolution,
                       7.5, 0.7, 12, 5.0, 7.5, 0.5, 12, 3.0, 1.0, 0.0, 12, 3.0, api_name="/image_to_3d")
        log(f"image_to_3d took {time.monotonic() - t1:.0f}s")
        # Reading the preview must never cost the call it came from: on any surprise, extract anyway.
        try:
            shots = views(html if isinstance(html, str) else str(html))
            if a.preview:
                a.preview.parent.mkdir(parents=True, exist_ok=True)
                contact_sheet(shots, CLAY, a.preview.with_name(a.preview.name + "_clay.jpg"))
                contact_sheet(shots, SHADED, a.preview.with_name(a.preview.name + "_shaded.jpg"))
            # The normal map renders have no dark side, so no part of the figure is lost in the shadow.
            boxes = [silhouette_box(shots[(NORMAL, step)]) for step in range(VIEWS) if (NORMAL, step) in shots]
            ratio = depth_ratio([w for w, _ in boxes], sorted(h for _, h in boxes)[VIEWS // 2]) if len(boxes) == VIEWS else None
            if ratio is None:
                log(f"WARNING: {len(boxes)} views found instead of {VIEWS}, extracting anyway")
        except Exception as e:  # noqa: BLE001
            ratio = None
            log(f"WARNING: cannot read the preview ({type(e).__name__}: {e}), extracting anyway")
        if ratio is not None:
            log(f"preview depth/height = {ratio:.3f} (billboards are under {a.min_depth})")
            if ratio < a.min_depth:
                log("FLAT: not extracting the GLB (saves a GPU call)")
                sys.exit(FLAT)

        log(f"extracting (/extract_glb): decimation={a.decimation} texture={a.texture_size}")
        t2 = time.monotonic()
        glb, _download = run_job(client, "extract_glb", a.timeout, a.decimation, a.texture_size, api_name="/extract_glb")
    except SystemExit:
        raise
    except KeyboardInterrupt:
        die("interrupted", 130)
    except Exception as e:  # noqa: BLE001
        die(explain(e, a.space), 2)

    a.output.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(as_path(glb), a.output)
    shutil.copyfile(pre, a.output.with_name(a.output.stem + "_preprocessed.png"))
    log(f"done in {time.monotonic() - t0:.0f}s (extract call {time.monotonic() - t2:.0f}s) -> "
        f"{a.output} ({a.output.stat().st_size / 1e6:.2f} MB)")


if __name__ == "__main__":
    main()
