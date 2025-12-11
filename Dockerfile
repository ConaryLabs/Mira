# Mira Power Suit - MCP Server for Claude Code
# Multi-stage build for minimal final image

# Build stage - Rust 1.85+ required for edition 2024
FROM rust:latest AS builder

WORKDIR /app

# Copy manifests first for dependency caching
COPY Cargo.toml Cargo.lock ./

# Create dummy src to build dependencies
RUN mkdir -p src && \
    echo "fn main() {}" > src/main.rs && \
    echo "pub fn lib() {}" > src/lib.rs

# Build dependencies (this layer is cached)
ENV SQLX_OFFLINE=true
RUN cargo build --release && rm -rf src

# Copy actual source code
COPY src ./src
COPY .sqlx ./.sqlx

# Build the actual binary
RUN touch src/main.rs src/lib.rs && cargo build --release

# Runtime stage - minimal image
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y \
    ca-certificates \
    sqlite3 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy binary from builder
COPY --from=builder /app/target/release/mira /usr/local/bin/mira

# Copy migrations and seed file
COPY migrations ./migrations
COPY seed_mira_guidelines.sql ./

# Create data directory
RUN mkdir -p /app/data

# Set default environment
ENV DATABASE_URL="sqlite:///app/data/mira.db"

# The MCP server runs over stdio, so we just exec the binary
# Claude Code will connect via stdin/stdout
ENTRYPOINT ["/usr/local/bin/mira"]
