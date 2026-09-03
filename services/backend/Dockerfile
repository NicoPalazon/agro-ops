FROM rust:1.98.0-bookworm AS dev

ARG LOCAL_UID=1000
ARG LOCAL_GID=1000

RUN groupadd --gid "${LOCAL_GID}" agro \
    && useradd \
        --uid "${LOCAL_UID}" \
        --gid "${LOCAL_GID}" \
        --create-home \
        --shell /bin/bash \
        agro

RUN rustup component add rustfmt

RUN cargo install cargo-watch --version 8.5.3 --locked

ENV CARGO_HOME=/home/agro/.cargo

RUN mkdir -p /home/agro/.cargo \
    && chown -R agro:agro /home/agro

WORKDIR /app

USER agro

CMD ["cargo", "watch", "-x", "run --bin api"]


FROM rust:1.98.0-bookworm AS builder

WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY src ./src

RUN cargo build --locked --release --bins


FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /app/target/release/api /usr/local/bin/api
COPY --from=builder /app/target/release/worker /usr/local/bin/worker
COPY --from=builder /app/target/release/jobs /usr/local/bin/jobs

CMD ["api"]
