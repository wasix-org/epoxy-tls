# syntax=docker/dockerfile:1.7
#
# Production image for epoxy-server.
#
# Build context MUST be the workspace root (epoxy-server depends on the
# sibling `wisp` crate via a path dependency), e.g. from the repo root:
#
#   docker build -f server/Dockerfile -t epoxy-server .
#
# Run with a config file mounted into the container:
#
#   docker run --rm -p 4000:4000 \
#     -v "$PWD/server/epoxy.toml:/etc/epoxy/config.toml:ro" \
#     epoxy-server
#
# To override the config path or pass other CLI flags:
#
#   docker run --rm epoxy-server --default-config
#

ARG DEBIAN_RELEASE=bookworm

############################
# Build stage
############################
FROM rust:${DEBIAN_RELEASE} AS builder

ENV DEBIAN_FRONTEND=noninteractive \
    CARGO_TERM_COLOR=always \
    CARGO_INCREMENTAL=0 \
    CARGO_NET_RETRY=10 \
    RUSTUP_MAX_RETRIES=10

RUN apt-get update && apt-get install -y --no-install-recommends \
        pkg-config \
        libssl-dev \
        cmake \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build

# Pin and pre-install the nightly toolchain declared by the workspace before
# pulling in the rest of the source tree so the toolchain layer can be cached.
COPY rust-toolchain.toml ./
RUN rustup show

# Workspace tree. `.dockerignore` at the repo root keeps `target/`, JS build
# output, and other heavy artefacts out of the build context.
# `.git` is required by build.rs (vergen-git2) to embed the commit SHA.
COPY . .

RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/build/target,sharing=locked \
    cargo build --release --locked --package epoxy-server \
    && objcopy --strip-debug --strip-unneeded \
        target/release/epoxy-server /usr/local/bin/epoxy-server

############################
# Runtime stage
############################
FROM debian:${DEBIAN_RELEASE}-slim AS runtime

RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates \
        tini \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --system --gid 10001 epoxy \
    && useradd  --system --uid 10001 --gid epoxy \
                --no-create-home --shell /usr/sbin/nologin epoxy \
    && mkdir -p /etc/epoxy \
    && chown epoxy:epoxy /etc/epoxy

COPY --from=builder /usr/local/bin/epoxy-server /usr/local/bin/epoxy-server

USER epoxy:epoxy
WORKDIR /etc/epoxy
EXPOSE 4000

# tini reaps zombies and forwards SIGTERM cleanly.
ENTRYPOINT ["/usr/bin/tini", "--", "/usr/local/bin/epoxy-server"]
# Positional arg consumed by clap as the config path. Override on `docker run`
# to point at a different location or pass other CLI flags.
CMD ["/etc/epoxy/config.toml"]
