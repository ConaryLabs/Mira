# deploy/README.md
# Mira VPS Deployment

Deploy Mira on a fresh Ubuntu 24.04 VPS for use with Claude Connections.

## Prerequisites

1. A VPS running Ubuntu 24.04
2. A domain name with DNS pointing to your VPS IP
3. SSH access to the server

## Quick Start

```bash
# SSH into your VPS
ssh root@your-vps-ip

# Download and run the setup script
curl -sSL https://raw.githubusercontent.com/ConaryLabs/Mira/main/deploy/setup-ubuntu.sh -o setup.sh
chmod +x setup.sh
./setup.sh mira.yourdomain.com
```

Or if you've cloned the repo:

```bash
sudo ./deploy/setup-ubuntu.sh mira.yourdomain.com
```

## What the script does

1. Updates system packages
2. Installs build dependencies (gcc, openssl, etc.)
3. Installs Rust
4. Installs Caddy (HTTPS reverse proxy with automatic SSL)
5. Clones and builds Mira
6. Creates systemd service
7. Configures Caddy with automatic HTTPS
8. Opens firewall ports (22, 80, 443)

## After installation

1. Add your OpenAI API key (required for semantic search):
   ```bash
   nano ~/Mira/.env
   # Add: OPENAI_API_KEY=sk-...
   ```

2. Restart Mira:
   ```bash
   sudo systemctl restart mira
   ```

3. Verify it's working:
   ```bash
   curl https://yourdomain.com/health
   ```

## Connecting to Claude.ai

1. Go to [Claude.ai](https://claude.ai)
2. Open Settings (gear icon)
3. Navigate to "Connections" or "MCP Servers"
4. Add a new connection:
   - URL: `https://mira.yourdomain.com/mcp`
   - Name: "Mira"

## Available MCP Tools

Once connected, Claude will have access to:

- `session_start` - Initialize project context
- `recall` - Search memories by meaning
- `remember` - Store facts and decisions
- `semantic_code_search` - Find code by intent
- `get_symbols` - List functions/classes in a file
- `task` - Manage tasks
- `goal` - Track goals and milestones
- `index` - Index a codebase

## Service Management

```bash
# View logs
journalctl -u mira -f

# Restart
sudo systemctl restart mira

# Stop
sudo systemctl stop mira

# Check status
sudo systemctl status mira
```

## Security Notes

- Caddy automatically provisions and renews SSL certificates via Let's Encrypt
- The MCP endpoint is public - consider adding authentication if storing sensitive data
- API keys are stored in `~/.env` with restricted permissions

## Troubleshooting

**Mira won't start:**
```bash
journalctl -u mira -n 100
```

**SSL certificate issues:**
```bash
journalctl -u caddy -n 100
```

**Port already in use:**
```bash
sudo lsof -i :3000
sudo lsof -i :443
```

## Manual Installation

If you prefer to install manually:

```bash
# 1. Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env

# 2. Install dependencies
sudo apt install build-essential pkg-config libssl-dev git sqlite3

# 3. Clone and build
git clone https://github.com/ConaryLabs/Mira.git
cd Mira
cargo build --release

# 4. Create env file
cp .env.example .env
nano .env  # Add your keys

# 5. Run
./target/release/mira web
```
