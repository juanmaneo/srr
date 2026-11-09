#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.10"
# dependencies = ["gradio_client>=1.3", "pillow"]
# ///
"""Turn one 2D image into a textured 3D .glb with the public TRELLIS Hugging Face Space.

    uv run tools/trellis/trellis_glb.py input.png out.glb [--seed 0] [--texture-size 1024]

Flow (endpoints of trellis-community/TRELLIS, checked with Client.view_api()):
    /start_session            create the per-session scratch dir on the Space
    /preprocess_image         crop/recenter (uses the alpha channel, no rembg if RGBA)
    /generate_and_extract_glb sparse structure + structured latent + GLB extraction

The request is anonymous by default. If the ZeroGPU quota is exhausted, rerun with
--use-token: the token is then looked up by huggingface_hub itself (HF_TOKEN or
`hf auth login`); this script never reads, prints or stores it.
"""
from __future__ import annotations

import argparse
import shutil
import sys
import time
from concurrent.futures import TimeoutError as FutureTimeout
from pathlib import Path

DEFAULT_SPACE = "trellis-community/TRELLIS"
# Stages of a Space that cannot serve requests (and why), see huggingface_hub.SpaceStage.
BAD_STAGES = {
    "PAUSED": "the Space is paused by its owner (it must be restarted from the Hub UI)",
    "STOPPED": "the Space is stopped",
    "CONFIG_ERROR": "the Space has a configuration error (its owner must fix it)",
    "BUILD_ERROR": "the Space failed to build",
    "RUNTIME_ERROR": "the Space crashed at runtime",
    "NO_APP_FILE": "the Space has no app file",
    "DELETING": "the Space is being deleted",
}


def log(msg: str) -> None:
    print(f"[trellis] {msg}", flush=True)


def die(msg: str, code: int = 1) -> "None":
    print(f"[trellis] ERROR: {msg}", file=sys.stderr, flush=True)
    sys.exit(code)


def as_path(value) -> str:
    """gradio_client returns local file paths, sometimes wrapped in a dict."""
    if isinstance(value, dict):
        value = value.get("video") or value.get("path") or value.get("url")
    if not value:
        raise RuntimeError("the Space returned no file")
    return str(value)


def check_space(space: str, token, timeout: float) -> None:
    """Fail early (or wait while it wakes up) depending on the Space runtime stage."""
    from huggingface_hub import HfApi

    api = HfApi(token=token)
    deadline = time.monotonic() + timeout
    woke = False
    while True:
        try:
            stage = str(api.get_space_runtime(space).stage)
        except Exception as e:  # noqa: BLE001
            first = (str(e).strip().splitlines() or [""])[0]
            die(f"cannot read the runtime of Space '{space}' ({type(e).__name__}: {first}). "
                "Does it exist and is huggingface.co reachable?")
        stage = stage.rsplit(".", 1)[-1].upper()
        if stage == "RUNNING":
            log(f"Space {space}: RUNNING")
            return
        if stage in BAD_STAGES:
            die(f"cannot use Space '{space}': {BAD_STAGES[stage]} (stage={stage}). "
                "Try another one with --space (e.g. a fork of TRELLIS).")
        if stage == "SLEEPING" and not woke:
            import httpx

            log("Space is sleeping, waking it up (this can take a few minutes)...")
            try:
                httpx.get(f"https://{space.replace('/', '-').replace('.', '-').lower()}.hf.space",
                          timeout=30, follow_redirects=True)
            except Exception:  # noqa: BLE001 - waking is best effort, we keep polling
                pass
            woke = True
        else:
            log(f"Space stage is {stage}, waiting...")
        if time.monotonic() > deadline:
            die(f"timeout while waiting for Space '{space}' to become RUNNING (stage={stage}).", 3)
        time.sleep(15)


def run_job(client, label: str, timeout: float, *args, api_name: str):
    """Submit one call, print queue/progress changes, return its result."""
    job = client.submit(*args, api_name=api_name)
    start = time.monotonic()
    last = None
    while True:
        try:
            return job.result(timeout=5)
        except (FutureTimeout, TimeoutError):
            pass
        elapsed = time.monotonic() - start
        if elapsed > timeout:
            job.cancel()
            die(f"timeout after {int(elapsed)} s while waiting for {label} "
                "(queue too long or generation stuck). Raise --timeout or retry later.", 3)
        st = job.status()
        snap = (str(st.code), st.rank, st.queue_size)
        if snap != last:
            where = f"queue position {st.rank + 1}/{st.queue_size}" if st.rank is not None else str(st.code)
            eta = f", eta ~{st.eta:.0f}s" if st.eta else ""
            log(f"{label}: {where}{eta} (elapsed {int(elapsed)}s)")
            last = snap


def explain(e: Exception, space: str) -> str:
    text = f"{type(e).__name__}: {e}"
    low = text.lower()
    if "quota" in low or "zerogpu" in low or "gpu task aborted" in low or "429" in low or "too many requests" in low:
        return ("ZeroGPU quota exceeded or rate limited (the message below says when it resets). "
                "Wait, or rerun with --use-token to use your Hugging Face account quota.\n  " + text)
    if "queue is full" in low or "queue_full" in low:
        return "the Space queue is full, try again in a few minutes.\n  " + text
    if "sleeping" in low or "paused" in low or "building" in low or "not running" in low:
        return f"Space '{space}' is not running (sleeping/paused/building).\n  " + text
    if "timed out" in low or "timeout" in low:
        return "network timeout talking to the Space.\n  " + text
    return text


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("image", type=Path, help="input image (RGBA PNG with transparent background is best)")
    ap.add_argument("output", type=Path, help="output .glb path")
    ap.add_argument("--space", default=DEFAULT_SPACE, help=f"Space id (default {DEFAULT_SPACE})")
    ap.add_argument("--seed", type=int, default=0)
    ap.add_argument("--texture-size", type=int, default=1024, choices=(512, 1024, 1536, 2048))
    ap.add_argument("--simplify", type=float, default=0.95,
                    help="fraction of triangles removed, 0.90-0.98 (default 0.95; 0.98 = lightest mesh)")
    ap.add_argument("--ss-steps", type=int, default=12, help="sparse-structure sampling steps (1-50)")
    ap.add_argument("--ss-guidance", type=float, default=7.5, help="sparse-structure guidance (0-10)")
    ap.add_argument("--slat-steps", type=int, default=12, help="structured-latent sampling steps (1-50)")
    ap.add_argument("--slat-guidance", type=float, default=3.0, help="structured-latent guidance (0-10)")
    ap.add_argument("--timeout", type=float, default=1800, help="max seconds to wait for the generation (default 1800)")
    ap.add_argument("--use-token", action="store_true",
                    help="authenticate with the locally stored Hugging Face token (default: anonymous)")
    ap.add_argument("--no-video", action="store_true", help="do not save the preview video next to the .glb")
    a = ap.parse_args()

    if not a.image.is_file():
        die(f"input image not found: {a.image}")
    if not 0.9 <= a.simplify <= 0.98:
        die("--simplify must be between 0.90 and 0.98 (range accepted by the Space)")

    from PIL import Image

    with Image.open(a.image) as im:
        log(f"input {a.image}: {im.size[0]}x{im.size[1]} {im.mode}")
        if "A" not in im.getbands():
            log("WARNING: no alpha channel; the Space will try to remove the background itself (rembg)")

    # token=False: strictly anonymous. token=None: huggingface_hub's standard lookup.
    token = None if a.use_token else False
    log("auth: " + ("Hugging Face account (standard token lookup)" if a.use_token else "anonymous"))

    from gradio_client import Client, handle_file

    check_space(a.space, token, a.timeout)
    t0 = time.monotonic()
    try:
        client = Client(a.space, token=token, verbose=False)
        client.predict(api_name="/start_session")

        log("preprocessing image (/preprocess_image)...")
        pre = as_path(run_job(client, "preprocess", a.timeout, handle_file(str(a.image)),
                              api_name="/preprocess_image"))

        log(f"generating (/generate_and_extract_glb): seed={a.seed} simplify={a.simplify} "
            f"texture={a.texture_size} ss={a.ss_steps}x{a.ss_guidance} slat={a.slat_steps}x{a.slat_guidance}")
        t1 = time.monotonic()
        video, glb, _dl = run_job(
            client, "generation", a.timeout,
            handle_file(pre), [], a.seed, a.ss_guidance, a.ss_steps, a.slat_guidance, a.slat_steps,
            "stochastic", a.simplify, a.texture_size,
            api_name="/generate_and_extract_glb",
        )
    except SystemExit:
        raise
    except KeyboardInterrupt:
        die("interrupted", 130)
    except Exception as e:  # noqa: BLE001
        die(explain(e, a.space), 2)

    a.output.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(as_path(glb), a.output)
    shutil.copyfile(pre, a.output.with_name(a.output.stem + "_preprocessed.png"))
    if not a.no_video:
        try:
            shutil.copyfile(as_path(video), a.output.with_suffix(".mp4"))
        except Exception as e:  # noqa: BLE001
            log(f"no preview video saved ({e})")
    log(f"done in {time.monotonic() - t0:.0f}s (generation call {time.monotonic() - t1:.0f}s) -> "
        f"{a.output} ({a.output.stat().st_size / 1e6:.2f} MB)")


if __name__ == "__main__":
    main()
