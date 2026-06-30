ARG RUST_VERSION=1

FROM rust:${RUST_VERSION}-bookworm AS builder

WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY src ./src

RUN cargo install --path . --locked

FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --create-home --home-dir /home/starsync starsync \
    && mkdir -p /data /state \
    && chown -R starsync:starsync /data /state /home/starsync

COPY --from=builder /usr/local/cargo/bin/starsync /usr/local/bin/starsync

ENV STARSYNC_DATA_DIR=/data \
    STARSYNC_STATE_DIR=/state \
    STARSYNC_BIND=0.0.0.0:8989

USER starsync

VOLUME ["/data", "/state"]
EXPOSE 8989

ENTRYPOINT ["starsync"]
CMD ["serve"]
