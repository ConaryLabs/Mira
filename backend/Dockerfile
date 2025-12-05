# backend/Dockerfile
# Multi-stage build for Mira backend

# Stage 1: Chef - compute recipe for caching
FROM rust:latest AS chef
RUN cargo install cargo-chef
WORKDIR /app

# Stage 2: Planner - create dependency recipe
FROM chef AS planner
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo chef prepare --recipe-path recipe.json

# Stage 3: Builder - build dependencies then application
FROM chef AS builder

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    libsqlite3-dev \
    && rm -rf /var/lib/apt/lists/*

# Build dependencies first (cached layer)
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json

# Build application
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY migrations ./migrations
COPY .sqlx ./.sqlx

# Enable SQLx offline mode
ENV SQLX_OFFLINE=true

RUN cargo build --release --bin mira-backend

# Install sqlx-cli for migrations
RUN cargo install sqlx-cli --no-default-features --features sqlite

# Stage 4: Runtime (must match build image's glibc version)
FROM debian:trixie-slim AS runtime

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3t64 \
    libsqlite3-0 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy binary, migrations, and sqlx-cli
COPY --from=builder /app/target/release/mira-backend /app/mira-backend
COPY --from=builder /app/migrations /app/migrations
COPY --from=builder /usr/local/cargo/bin/sqlx /usr/local/bin/sqlx

# Create data directory with proper permissions
RUN mkdir -p /data && chmod 777 /data

# Create startup script that runs migrations then starts the app
RUN printf '#!/bin/bash\nset -e\necho "Running database migrations..."\ncd /app && sqlx database create || true\nsqlx migrate run\necho "Starting Mira backend..."\nexec ./mira-backend\n' > /app/start.sh && chmod +x /app/start.sh

# Environment defaults
ENV MIRA_PORT=3001
ENV MIRA_ENV=production
ENV DATABASE_URL=sqlite:///data/mira.db
ENV RUST_LOG=info

EXPOSE 3001

CMD ["/app/start.sh"]
