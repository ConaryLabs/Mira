#!/bin/bash
# Start Mira daemon with Qdrant dependency

set -e

cd /home/peter/Mira

# Ensure Qdrant is running via docker-compose
if ! docker ps | grep -q mira-qdrant; then
    echo "Starting Qdrant..."
    docker compose up -d qdrant
    sleep 5
fi

# Load environment
if [ -f .env ]; then
    export $(grep -v '^#' .env | xargs)
fi

export DATABASE_URL="${DATABASE_URL:-sqlite://data/mira.db}"
export QDRANT_URL="${QDRANT_URL:-http://localhost:6334}"

# Start the daemon
exec /home/peter/Mira/target/release/mira daemon start -p /home/peter/Mira
