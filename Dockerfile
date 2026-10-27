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
 && rm -rf /var/lib/apt/lists/* \
 && useradd --system --uid 10001 --home-dir /data --create-home bot
COPY --from=build /out/ /usr/local/bin/

USER bot
WORKDIR /data
# Feedback reports (and analysis output) are written under /data; mount a volume there to keep them.
ENV HANABI_FEEDBACK_FILE=/data/feedback.jsonl
VOLUME /data

# Credentials and settings come from the environment (HANABI_USERNAME0, HANABI_PASSWORD0, ...).
# `index=0,1,2` hosts three accounts in one process.
ENTRYPOINT ["bot"]
CMD ["index=0"]
