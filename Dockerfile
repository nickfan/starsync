ARG RUST_VERSION=1
ARG DEBIAN_VERSION=bookworm
ARG CARGO_CHEF_VERSION=0.1.77
ARG BASE_IMAGE_REGISTRY=

FROM ${BASE_IMAGE_REGISTRY}rust:${RUST_VERSION}-${DEBIAN_VERSION} AS chef

ARG CARGO_CHEF_VERSION

WORKDIR /app

RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    cargo install cargo-chef --version "${CARGO_CHEF_VERSION}" --locked

FROM chef AS planner

COPY Cargo.toml Cargo.lock ./
COPY src ./src

RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS cacher

COPY --from=planner /app/recipe.json recipe.json

RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    cargo chef cook --release --locked --recipe-path recipe.json

FROM chef AS builder

COPY --from=cacher /app/target /app/target
COPY Cargo.toml Cargo.lock ./
COPY src ./src

RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    cargo build --release --locked \
    && mkdir -p /out \
    && cp /app/target/release/starsync /out/starsync \
    && if command -v strip >/dev/null 2>&1; then strip /out/starsync; fi

FROM ${BASE_IMAGE_REGISTRY}debian:${DEBIAN_VERSION}-slim AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --create-home --home-dir /home/starsync starsync \
    && mkdir -p /data /state \
    && chown -R starsync:starsync /data /state /home/starsync

COPY --from=builder /out/starsync /usr/local/bin/starsync

ENV STARSYNC_DATA_DIR=/data \
    STARSYNC_STATE_DIR=/state \
    STARSYNC_BIND=0.0.0.0:8989

USER starsync

VOLUME ["/data", "/state"]
EXPOSE 8989

ENTRYPOINT ["starsync"]
CMD ["serve"]
