# syntax=docker/dockerfile:1

# ---- build ----
FROM rust:1-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src ./src
# The cache mounts keep the dependency downloads and build artifacts between builds.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked \
        --bin bot --bin analyze_history --bin analyze --bin replay --bin self_play \
 && mkdir /out \
 && cp target/release/bot target/release/analyze_history target/release/analyze \
       target/release/replay target/release/self_play /out/

# ---- run ----
FROM debian:bookworm-slim
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates \
 && rm -rf /var/lib/apt/lists/*
COPY --from=build /out/ /usr/local/bin/
COPY --chmod=755 docker-entrypoint.sh /usr/local/bin/docker-entrypoint.sh

WORKDIR /data
# Feedback reports (and analysis output) are written under /data; mount a volume there to keep them.
ENV HANABI_FEEDBACK_FILE=/data/feedback.jsonl
VOLUME /data

# The entrypoint starts as root, makes /data belong to PUID:PGID (default 10001:10001), drops to that user and
# runs the command you give it, for example `bot index=0,1,2` (there is no default command).
# Credentials and settings come from the environment (HANABI_USERNAME0, HANABI_PASSWORD0, ...).
ENTRYPOINT ["/usr/local/bin/docker-entrypoint.sh"]
