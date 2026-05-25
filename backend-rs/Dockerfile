FROM rust:bookworm AS chef
# We only pay the installation cost once,
# it will be cached from the second build onwards
RUN cargo install --locked cargo-chef
WORKDIR /app


FROM chef AS planner
COPY . .
RUN cargo chef prepare  --recipe-path recipe.json

FROM chef AS builder

RUN apt-get update && apt-get install -y \
    libopenjp2-7-dev \
    cmake \
    libsqlite3-dev \
    sqlite3 \
    && rm -rf /var/lib/apt/lists/*

COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json

COPY Cargo.toml build.rs meshdb_openapi.json ./
COPY src ./src

RUN cargo build --release


FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y \
    libopenjp2-7 \
    libsqlite3-0 \
    ca-certificates \
    curl \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/loscope /usr/local/bin/loscope

ENV ROCKET_ADDRESS=0.0.0.0
ENV ROCKET_PORT=8000

EXPOSE 8000

CMD ["loscope"]
