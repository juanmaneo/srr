# syntax=docker/dockerfile:1
# Shattered Realm: Rampage in Docker. Two uses, two images:
#
# * The playable game, for a machine with Docker but no Rust (the default target). It compiles
#   the web build and the LAN server from this copy of the project and serves them on port 8080:
#
#       docker build -t srr .
#       docker run --rm -p 8080:8080 srr
#
#   then open http://<host-ip>:8080 on every machine. The address the server prints is the
#   container's; use the host machine's LAN address instead (`ip addr` / `ipconfig`).
#
# * The `toolchain` stage, holding only the Rust toolchain and the WASM / coverage tools.
#   `cargo xtask docker <task>` builds it (`docker build --target toolchain`), mounts the project
#   at /work and runs `cargo xtask <task>` inside it, so build outputs (dist/) land on the host.
#   The container runs the command as the owner of /work, so those outputs belong to the host
#   user rather than to root.
ARG RUST_VERSION=1.88
ARG DEBIAN_RELEASE=bookworm

# ---------------------------------------------------------------------------------------------
# Toolchain
# ---------------------------------------------------------------------------------------------
FROM rust:${RUST_VERSION}-${DEBIAN_RELEASE} AS toolchain

# Must match the `wasm-bindgen` crate version in Cargo.lock (`cargo tree -i wasm-bindgen`).
ARG WASM_BINDGEN_VERSION=0.2.129

# The native build of the game (run by `cargo test --workspace`) plays the music through ALSA.
RUN apt-get update \
 && apt-get install -y --no-install-recommends libasound2-dev pkg-config \
 && rm -rf /var/lib/apt/lists/*

# The base image already makes $CARGO_HOME and $RUSTUP_HOME world-writable; the files these
# installs add (crate registry, package cache lock) must be too, for the non-root user below.
RUN rustup target add wasm32-unknown-unknown \
 && rustup component add llvm-tools-preview \
 && cargo install --locked wasm-bindgen-cli --version ${WASM_BINDGEN_VERSION} \
 && cargo install --locked cargo-llvm-cov \
 && chmod -R a+rwX ${CARGO_HOME}

# Runs the command as the owner of /work (the project, bind-mounted by `cargo xtask docker`),
# so that target/ and dist/ are created with the host user's uid/gid instead of root's. Stays
# root when the mount shows no owner (Docker Desktop, rootless engines, no mount at all).
COPY --chmod=755 <<'EOF' /usr/local/bin/srr-entrypoint
#!/bin/sh
set -eu
uid=$(stat -c %u /work)
gid=$(stat -c %g /work)
if [ "$(id -u)" != 0 ] || [ "$uid" = 0 ]; then
    exec "$@"
fi
# The crate cache volume may still belong to root, from a container that ran before this fix.
if [ "$(stat -c %u "$CARGO_HOME/registry")" != "$uid" ]; then
    chown -R "$uid:$gid" "$CARGO_HOME/registry"
fi
# $HOME is /root; the user has no entry in /etc/passwd, so point it somewhere writable.
exec setpriv --reuid="$uid" --regid="$gid" --clear-groups --inh-caps=-all env HOME=/tmp "$@"
EOF
# Git on Windows checks this Dockerfile out with CRLF line endings (`core.autocrlf=true`).
# Docker drops the CR of its instructions but keeps the ones inside a heredoc, so the script
# would start with `#!/bin/sh<CR>`, a shell that does not exist, and every `cargo xtask docker`
# would fail with "exec /usr/local/bin/srr-entrypoint: no such file or directory".
RUN sed -i 's/\r$//' /usr/local/bin/srr-entrypoint

WORKDIR /work
# `cargo xtask docker serve` runs the LAN server on this port.
EXPOSE 8080
ENTRYPOINT ["srr-entrypoint"]
CMD ["cargo", "xtask", "help"]

# ---------------------------------------------------------------------------------------------
# Build: the web build (dist/) and the LAN server, from a copy of the project
# ---------------------------------------------------------------------------------------------
FROM toolchain AS build

COPY . .
# The cache mounts keep the downloaded crates and the compiled dependencies between builds, so
# editing a source file does not recompile Bevy. They vanish after the step: `dist/` lives
# outside them, and the server binary is copied out.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/work/target \
    cargo xtask dist \
 && cargo build --release --package srr-server \
 && cp target/release/srr-server /usr/local/bin/srr-server

# ---------------------------------------------------------------------------------------------
# Runtime: the game and its server, nothing else
# ---------------------------------------------------------------------------------------------
FROM debian:${DEBIAN_RELEASE}-slim

# As PID 1 the server would ignore Ctrl+C (no signal handler); `tini` forwards it properly.
RUN apt-get update \
 && apt-get install -y --no-install-recommends tini \
 && rm -rf /var/lib/apt/lists/*

WORKDIR /srr
COPY --from=build /usr/local/bin/srr-server /usr/local/bin/srr-server
COPY --from=build /work/dist dist

EXPOSE 8080
ENTRYPOINT ["tini", "--"]
CMD ["srr-server", "--dir", "dist", "--port", "8080"]
