# syntax=docker/dockerfile:1.7
# Uma imagem só para todos os papéis do laboratório: o mesmo binário vira validador, retransmissor ou ferramenta.
FROM rust:1-slim-bookworm AS build
WORKDIR /src
RUN apt-get update && apt-get install -y --no-install-recommends build-essential pkg-config \
    && rm -rf /var/lib/apt/lists/*
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked -p brdu-nodebin -p brdu-dev \
    && mkdir -p /out && cp target/release/brdu-node target/release/brdu-dev /out/

FROM debian:bookworm-slim
RUN useradd --system --uid 10001 --create-home brdu && mkdir -p /data && chown brdu /data
COPY --from=build /out/brdu-node /out/brdu-dev /usr/local/bin/
USER brdu
WORKDIR /data
ENTRYPOINT ["brdu-node"]
