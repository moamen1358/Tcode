# Tessera — multi-stage build. Runs INSIDE the container; the GUI renders on the
# host display via socket forwarding (see run-docker.sh).

# ---- Stage 1: build the release binary -------------------------------------
FROM ubuntu:24.04 AS builder
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
        build-essential pkg-config curl ca-certificates \
        libgtk-4-dev libvte-2.91-gtk4-dev \
    && rm -rf /var/lib/apt/lists/*
# Rust toolchain (matches docs/BUILD_LOG.md). MSRV for gtk4 0.11 is 1.83.
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
        | sh -s -- -y --profile minimal --default-toolchain stable
ENV PATH="/root/.cargo/bin:${PATH}"
WORKDIR /app
COPY . .
RUN cargo build --release -p tessera

# ---- Stage 2: slim runtime image -------------------------------------------
FROM ubuntu:24.04 AS runtime
ENV DEBIAN_FRONTEND=noninteractive
# Runtime libs only: GTK4 + VTE4 runtime, a font so the terminal renders text,
# and Mesa for GL (GTK4's renderer). A shell is already present (/bin/bash, /bin/sh).
RUN apt-get update && apt-get install -y --no-install-recommends \
        libgtk-4-1 libvte-2.91-gtk4-0 \
        fonts-jetbrains-mono fonts-dejavu-core \
        libgl1-mesa-dri libegl1 libgles2 \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/tessera /usr/local/bin/tessera
# Container panes run bash with a directory-showing prompt (native runs use your $SHELL).
COPY docker/tessera-profile.sh /etc/profile.d/zz-tessera.sh
ENV SHELL=/bin/bash
# If a host has no GPU passthrough, set GSK_RENDERER=cairo for software rendering.
ENTRYPOINT ["tessera"]
