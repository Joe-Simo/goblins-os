# syntax=docker/dockerfile:1.7
# P6 sign-off gate: the CI `rust` job, runnable locally via `docker build` (tar
# context — no iCloud/virtio-fs bind-mount EIO). fmt + clippy(-D warnings) + test.
#   docker build -f os/bootc/gate.Dockerfile -t goblins-os-gate .
FROM docker.io/library/rust:1.88
ENV CARGO_NET_RETRY=10 \
    CARGO_HTTP_TIMEOUT=600
RUN apt-get update \
    && apt-get install -y --no-install-recommends libgtk-4-dev pkg-config \
    && rm -rf /var/lib/apt/lists/*
RUN rustup component add rustfmt clippy
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN cargo fmt --all --check
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo clippy --workspace --features "goblins-os-installer/native-desktop goblins-os-login/native-desktop goblins-os-settings/native-desktop goblins-os-shell/native-desktop goblins-os-launcher/native-desktop goblins-os-control-center/native-desktop goblins-os-ui/native-desktop" -- -D warnings
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo test --workspace --features "goblins-os-installer/native-desktop goblins-os-login/native-desktop goblins-os-settings/native-desktop goblins-os-shell/native-desktop goblins-os-launcher/native-desktop goblins-os-control-center/native-desktop goblins-os-ui/native-desktop"
