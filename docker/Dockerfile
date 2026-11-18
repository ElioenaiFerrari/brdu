# syntax=docker/dockerfile:1.7
# A single image for every lab role: the same binary becomes a validator, a relayer or a tool.
FROM rust:1-slim-bookworm AS build
WORKDIR /src
RUN apt-get update && apt-get install -y --no-install-recommends build-essential pkg-config \
    && rm -rf /var/lib/apt/lists/*
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked -p brdu-nodebin -p brdu-dev -p brdu-explorer \
    && mkdir -p /out && cp target/release/brdu-node target/release/brdu-dev target/release/brdu-explorer /out/

FROM debian:bookworm-slim
RUN useradd --system --uid 10001 --create-home brdu && mkdir -p /data && chown brdu /data
COPY --from=build /out/brdu-node /out/brdu-dev /out/brdu-explorer /usr/local/bin/
USER brdu
WORKDIR /data
ENTRYPOINT ["brdu-node"]
