# Mira Deployment Guide

This guide covers deploying Mira in development and production environments.

## Table of Contents

1. [Prerequisites](#prerequisites)
2. [Quick Start (Development)](#quick-start-development)
3. [Production Deployment](#production-deployment)
4. [Docker Deployment](#docker-deployment)
5. [Service Management](#service-management)
6. [Configuration Reference](#configuration-reference)
7. [Nginx Configuration](#nginx-configuration)
8. [SSL/TLS Setup](#ssltls-setup)
9. [Monitoring & Logging](#monitoring--logging)
10. [Troubleshooting](#troubleshooting)
11. [Backup & Recovery](#backup--recovery)

---

## Prerequisites

### Required Software

| Component | Minimum Version | Purpose |
|-----------|----------------|---------|
| Rust | 1.91+ | Backend compilation |
| Node.js | 18+ | Frontend build |
| SQLite | 3.35+ | Database |
| Qdrant | 1.12+ | Vector database |

### API Keys

- **OpenAI API Key**: Required for GPT-5.1 (LLM) and text-embedding-3-large
  - Get from: https://platform.openai.com/api-keys

### System Requirements

| Resource | Development | Production |
|----------|-------------|------------|
| CPU | 2 cores | 4+ cores |
| RAM | 4 GB | 8+ GB |
| Disk | 10 GB | 50+ GB |
| Network | Localhost | Outbound HTTPS |

---

## Quick Start (Development)

> **Just want to run Mira?** Skip to [Docker Deployment](#docker-deployment) for a simpler setup that doesn't require Rust or Node.js.

### 1. Clone and Setup

```bash
git clone <repository-url> mira
cd mira
```

### 2. Start Qdrant

```bash
cd backend

# Option A: Binary (download from github.com/qdrant/qdrant/releases)
./bin/qdrant --config-path ./config/config.yaml

# Option B: Docker
docker run -d -p 6333:6333 -p 6334:6334 qdrant/qdrant:latest
```

### 3. Configure Environment

```bash
cd backend
cp .env.example .env

# Edit .env and add your OpenAI API key
# OPENAI_API_KEY=your-key-here
```

### 4. Build and Run Backend

```bash
cd backend
cargo build --release
./target/release/mira-backend
```

### 5. Build and Run Frontend

```bash
cd frontend
npm install
npm run dev    # Development (hot reload)
# or
npm run build  # Production build
npm run preview
```

### 6. Access Mira

- Frontend: http://localhost:5173 (dev) or http://localhost:4173 (preview)
- Backend WebSocket: ws://localhost:3001/ws
- Qdrant: http://localhost:6333

---

## Production Deployment

### Automated Installation (Ubuntu 24.04)

```bash
sudo ./install.sh
```

This script:
1. Installs system dependencies (Rust, Node.js, SQLite, Nginx)
2. Downloads and configures Qdrant
3. Builds backend and frontend
4. Sets up systemd services
5. Configures Nginx reverse proxy
6. Creates initial user account

### Manual Installation

#### Step 1: Install Dependencies

```bash
# Ubuntu/Debian
sudo apt update
sudo apt install -y build-essential curl git libgit2-dev sqlite3 \
    libsqlite3-dev libssl-dev pkg-config nginx

# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env

# Install Node.js 20
curl -fsSL https://deb.nodesource.com/setup_20.x | sudo bash -
sudo apt install -y nodejs
```

#### Step 2: Download Qdrant

```bash
# Create bin directory
mkdir -p backend/bin

# Download for your architecture (x86_64 or aarch64)
QDRANT_VERSION="v1.12.1"
ARCH="x86_64-unknown-linux-gnu"  # or aarch64-unknown-linux-gnu

curl -L "https://github.com/qdrant/qdrant/releases/download/${QDRANT_VERSION}/qdrant-${ARCH}.tar.gz" \
    | tar -xz -C backend/bin/

chmod +x backend/bin/qdrant
```

#### Step 3: Build Application

```bash
# Backend
cd backend
cargo build --release

# Frontend
cd ../frontend
npm install
npm run build
```

#### Step 4: Configure Services

Copy service files to systemd:

```bash
# System-level services (requires root)
sudo cp scripts/mira-qdrant.service /etc/systemd/system/
sudo cp scripts/mira-backend.service /etc/systemd/system/

# Edit services to replace placeholders
sudo sed -i "s|MIRA_PATH|$(pwd)|g" /etc/systemd/system/mira-*.service
sudo sed -i "s|MIRA_USER|$(whoami)|g" /etc/systemd/system/mira-*.service

# Reload and enable
sudo systemctl daemon-reload
sudo systemctl enable mira-qdrant mira-backend
sudo systemctl start mira-qdrant mira-backend
```

#### Step 5: Configure Nginx

```bash
# Copy config
sudo cp scripts/mira-nginx.conf /etc/nginx/sites-available/mira
sudo sed -i "s|MIRA_PATH|$(pwd)|g" /etc/nginx/sites-available/mira

# Enable site
sudo ln -sf /etc/nginx/sites-available/mira /etc/nginx/sites-enabled/
sudo rm -f /etc/nginx/sites-enabled/default

# Test and reload
sudo nginx -t
sudo systemctl reload nginx
```

---

## Docker Deployment

### Full Stack with Docker Compose (Recommended for Users)

**This is the recommended way for users to run Mira.** Docker Compose bundles everything needed - no Rust, Node.js, or manual setup required.

For **developers** contributing to Mira, see [Quick Start (Development)](#quick-start-development) for the native setup which provides faster iteration.

#### Prerequisites

- Docker 24+ with Docker Compose v2
- OpenAI API key for GPT-5.1

#### Quick Start

```bash
# 1. Configure environment
cp .env.example .env
# Edit .env and add your OPENAI_API_KEY

# 2. Build and start all services
docker compose up -d

# 3. Access Mira
# Frontend: http://localhost:8080 (or port 80 if available)
# Backend health: http://localhost:3001/health
# Qdrant dashboard: http://localhost:6333/dashboard
```

#### Service Architecture

```
docker-compose.yml runs:
+------------------+     +------------------+     +------------------+
|     frontend     |     |     backend      |     |      qdrant      |
|  (nginx:alpine)  | --> |  (rust binary)   | --> | (qdrant:latest)  |
|    port 8080     |     |    port 3001     |     |  ports 6333/6334 |
+------------------+     +------------------+     +------------------+
        |                        |                        |
        v                        v                        v
   Static files           /data volume              qdrant_data volume
```

#### Docker Compose Commands

```bash
# Start all services
docker compose up -d

# View logs
docker compose logs -f          # All services
docker compose logs backend -f   # Backend only

# Rebuild after code changes
docker compose build backend
docker compose up -d backend

# Stop all services
docker compose down

# Reset data (fresh start)
docker compose down
docker volume rm mira_backend_data mira_qdrant_data
docker compose up -d
```

#### Configuration

Environment variables can be set in the root `.env` file:

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `OPENAI_API_KEY` | Yes | - | OpenAI API key for GPT-5.1 |
| `RUST_LOG` | No | `info` | Log level (debug, info, warn, error) |

The backend uses `backend/.env.docker` for additional configuration. The docker-compose.yml overrides database and Qdrant URLs for container networking.

#### Persistent Data

Data is stored in Docker volumes:
- `mira_backend_data`: SQLite database (`/data/mira.db`)
- `mira_qdrant_data`: Qdrant vector storage

#### Building Images Manually

```bash
# Build backend
cd backend
docker build -t mira-backend .

# Build frontend
cd frontend
docker build -t mira-frontend .
```

#### Notes

- First startup runs database migrations automatically
- Backend waits for Qdrant health check before starting
- Frontend proxies `/ws` and `/api` to backend container

### Qdrant Only (Development)

For development, you may want to run only Qdrant in Docker while running backend/frontend natively:

```bash
# Start just Qdrant
docker compose up -d qdrant

# Verify Qdrant is running
curl http://localhost:6333/health
```

---

## Service Management

### Using mira-ctl (Recommended)

The `mira-ctl` script provides convenient service management:

```bash
# Add to PATH (optional)
export PATH="$PATH:/path/to/mira"

# Start services
mira-ctl start all           # Start backend + frontend
mira-ctl start backend       # Start backend only
mira-ctl start frontend      # Start frontend only

# Stop services
mira-ctl stop all
mira-ctl stop backend

# Restart services
mira-ctl restart all
mira-ctl restart backend

# Check status
mira-ctl status

# View logs
mira-ctl logs backend        # Recent logs
mira-ctl logs backend -f     # Follow logs
mira-ctl logs frontend -f

# Rebuild after code changes
mira-ctl rebuild             # Build release and restart backend
mira-ctl build               # Build only (no restart)
```

### Using systemctl Directly

```bash
# User services (development)
systemctl --user start mira-backend
systemctl --user stop mira-backend
systemctl --user restart mira-backend
systemctl --user status mira-backend

# System services (production, requires sudo)
sudo systemctl start mira-backend
sudo systemctl stop mira-backend
sudo systemctl restart mira-backend
sudo systemctl status mira-backend

# View logs
journalctl --user -u mira-backend -f     # User service
sudo journalctl -u mira-backend -f       # System service
```

### Service Dependencies

```
mira-qdrant (Qdrant vector database)
    |
    v
mira-backend (Rust WebSocket server, port 3001)
    |
    v
mira-frontend (Vite dev server, port 5173)
    |
    v
nginx (reverse proxy, port 80/443)
```

---

## Configuration Reference

### Environment Variables (backend/.env)

#### Required

| Variable | Description | Example |
|----------|-------------|---------|
| `OPENAI_API_KEY` | OpenAI API key for GPT-5.1 | `sk-proj-...` |

#### LLM Configuration

| Variable | Default | Description |
|----------|---------|-------------|
| `MODEL_ROUTER_ENABLED` | `true` | Enable 4-tier model routing |
| `MODEL_FAST` | `gpt-5.1-codex-mini` | Fast tier model |
| `MODEL_VOICE` | `gpt-5.1` | Voice tier model (user chat) |
| `MODEL_CODE` | `gpt-5.1-codex-max` | Code tier model |
| `MODEL_AGENTIC` | `gpt-5.1-codex-max` | Agentic tier model |

#### Budget Management

| Variable | Default | Description |
|----------|---------|-------------|
| `BUDGET_DAILY_LIMIT_USD` | `5.0` | Daily spending limit |
| `BUDGET_MONTHLY_LIMIT_USD` | `150.0` | Monthly spending limit |
| `CACHE_ENABLED` | `true` | Enable LLM response caching |
| `CACHE_TTL_SECONDS` | `86400` | Cache time-to-live (24 hours) |

#### Database & Storage

| Variable | Default | Description |
|----------|---------|-------------|
| `DATABASE_URL` | `sqlite:./data/mira.db` | SQLite database path |
| `QDRANT_URL` | `http://localhost:6334` | Qdrant gRPC endpoint |
| `QDRANT_COLLECTION` | `mira` | Collection name prefix |

#### Server

| Variable | Default | Description |
|----------|---------|-------------|
| `MIRA_HOST` | `0.0.0.0` | Bind address |
| `MIRA_PORT` | `3001` | WebSocket port |
| `RUST_LOG` | `info` | Log level |

#### Rate Limiting

| Variable | Default | Description |
|----------|---------|-------------|
| `RATE_LIMIT_ENABLED` | `true` | Enable rate limiting |
| `RATE_LIMIT_REQUESTS_PER_MINUTE` | `60` | Max requests per minute |

See `backend/.env.example` for complete configuration options.

---

## Nginx Configuration

### Basic Setup

The default Nginx config (`scripts/mira-nginx.conf`) provides:

- Static file serving for frontend
- WebSocket proxy to backend (/ws)
- API proxy (/api)
- Health check endpoint (/health)

### Key Settings

```nginx
# WebSocket timeout (24 hours for long-running sessions)
proxy_read_timeout 86400s;
proxy_send_timeout 86400s;

# Gzip compression
gzip on;
gzip_types text/plain text/css application/json application/javascript;
```

---

## SSL/TLS Setup

### Using Let's Encrypt (Certbot)

```bash
# Install certbot
sudo apt install certbot python3-certbot-nginx

# Obtain certificate
sudo certbot --nginx -d yourdomain.com

# Auto-renewal is configured automatically
```

### Manual SSL Configuration

```nginx
server {
    listen 443 ssl http2;
    server_name yourdomain.com;

    ssl_certificate /etc/letsencrypt/live/yourdomain.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/yourdomain.com/privkey.pem;

    # SSL settings
    ssl_protocols TLSv1.2 TLSv1.3;
    ssl_ciphers ECDHE-ECDSA-AES128-GCM-SHA256:ECDHE-RSA-AES128-GCM-SHA256;
    ssl_prefer_server_ciphers off;

    # ... rest of config
}

# Redirect HTTP to HTTPS
server {
    listen 80;
    server_name yourdomain.com;
    return 301 https://$server_name$request_uri;
}
```

---

## Monitoring & Logging

### Log Locations

| Component | Location |
|-----------|----------|
| Backend | `journalctl -u mira-backend` |
| Qdrant | `journalctl -u mira-qdrant` |
| Nginx | `/var/log/nginx/access.log`, `/var/log/nginx/error.log` |

### Structured Logging

Backend logs use tracing with structured fields:

```
INFO mira_backend::operations: LLM orchestration completed
    operation_id="op-123"
    duration_ms=1500
    tokens_input=5000
    tokens_output=1000
    cost_usd=0.05
```

### Health Checks

The backend provides three health check endpoints:

```bash
# Full health check (DB + Qdrant)
curl http://localhost:3001/health
# Returns: {"status":"healthy","db":"ok","qdrant":"ok"}

# Readiness probe (migrations applied)
curl http://localhost:3001/ready
# Returns: {"status":"ready","migrations":"applied"}

# Liveness probe (simple ping)
curl http://localhost:3001/live
# Returns: {"status":"alive"}

# Qdrant health
curl http://localhost:6333/health
```

### Prometheus Metrics

Prometheus metrics are available at `/metrics`:

```bash
curl http://localhost:3001/metrics
```

Available metrics:
- `mira_requests_total` - Total requests by type
- `mira_request_duration_seconds` - Request latency histogram
- `mira_llm_calls_total` - LLM API calls by model and status
- `mira_llm_cache_total` - Cache hit/miss counts
- `mira_budget_daily_used_usd` - Current daily budget usage
- `mira_budget_monthly_used_usd` - Current monthly budget usage
- `mira_active_connections` - Active WebSocket connections
- `mira_llm_tokens_total` - Token usage by type (prompt/completion/reasoning)
- `mira_tool_executions_total` - Tool execution counts

### Rate Limiting

Rate limiting is configurable via environment variables:

```bash
RATE_LIMIT_ENABLED=true
RATE_LIMIT_REQUESTS_PER_MINUTE=60
```

### Metrics to Monitor

- **Response time**: Target < 5s for typical queries
- **Cache hit rate**: Target > 80% (`mira_llm_cache_total`)
- **Daily/monthly spend**: Check budget limits (`mira_budget_*_used_usd`)
- **Error rate**: Check journalctl for errors
- **Qdrant memory**: Monitor collection sizes
- **Active connections**: Monitor WebSocket connections (`mira_active_connections`)

---

## Troubleshooting

### Common Issues

#### Backend won't start

```bash
# Check logs
journalctl -u mira-backend -n 50

# Common causes:
# 1. Qdrant not running
curl http://localhost:6333/health

# 2. Missing API key
grep OPENAI_API_KEY backend/.env

# 3. Port already in use
lsof -i :3001
```

#### WebSocket connection fails

```bash
# Check Nginx config
sudo nginx -t

# Check backend is listening
ss -tlnp | grep 3001

# Test WebSocket directly
wscat -c ws://localhost:3001/ws
```

#### Qdrant issues

```bash
# Check Qdrant status
curl http://localhost:6333/collections

# Restart Qdrant
sudo systemctl restart mira-qdrant

# Check disk space (Qdrant stores data in backend/data/qdrant/)
df -h
```

#### Database errors

```bash
# Check SQLite database
sqlite3 backend/data/mira.db ".tables"

# Run migrations manually
cd backend
sqlx migrate run
```

### Reset Commands

```bash
# Reset SQLite only (keep embeddings)
./backend/scripts/db-reset-sqlite.sh

# Reset Qdrant only (keep structured data)
./backend/scripts/db-reset-qdrant.sh

# Full reset (nuclear option)
./backend/scripts/db-reset.sh
```

---

## Backup & Recovery

### What to Backup

| Data | Location | Size | Frequency |
|------|----------|------|-----------|
| SQLite DB | `backend/data/mira.db` | ~100MB+ | Daily |
| Qdrant data | `backend/data/qdrant/` | ~1GB+ | Weekly |
| Configuration | `backend/.env` | <1KB | On change |

### Backup Commands

```bash
# SQLite backup (online, safe)
sqlite3 backend/data/mira.db ".backup 'backup-$(date +%Y%m%d).db'"

# Qdrant snapshot
curl -X POST "http://localhost:6333/snapshots"

# Full backup script
tar -czf mira-backup-$(date +%Y%m%d).tar.gz \
    backend/data/mira.db \
    backend/data/qdrant/ \
    backend/.env
```

### Recovery

```bash
# Restore SQLite
cp backup-20241203.db backend/data/mira.db

# Restore Qdrant (stop service first)
sudo systemctl stop mira-qdrant
tar -xzf qdrant-backup.tar.gz -C backend/data/
sudo systemctl start mira-qdrant
```

---

## Security Checklist

- [ ] API keys not in version control
- [ ] `.env` file has mode 600
- [ ] Firewall allows only ports 80, 443
- [ ] SSL/TLS enabled in production
- [ ] Nginx rate limiting configured
- [ ] Regular backups configured
- [ ] Log rotation configured
- [ ] Budget limits set appropriately

---

## Architecture Overview

```
+-------------+      +-------------+      +-------------+
|   Browser   | <--> |    Nginx    | <--> |   Backend   |
| (React SPA) |      | (port 80)   |      | (port 3001) |
+-------------+      +-------------+      +------+------+
                                                 |
                     +-------------+      +------v------+
                     |   SQLite    | <--> |   Qdrant    |
                     | (mira.db)   |      | (port 6334) |
                     +-------------+      +-------------+
```

**Data Flow:**
1. User connects via WebSocket to Nginx
2. Nginx proxies to backend on port 3001
3. Backend queries SQLite for structured data
4. Backend queries Qdrant for vector similarity search
5. Backend calls OpenAI GPT-5.1 for LLM responses
6. Responses stream back via WebSocket

---

## Version History

| Version | Date | Changes |
|---------|------|---------|
| 0.9.0 | 2025-12 | Initial deployment documentation |
