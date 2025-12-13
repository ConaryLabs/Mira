#!/bin/bash
# Rebuild Mira and restart services
set -e

cd /home/peter/Mira

echo "Building Mira..."
SQLX_OFFLINE=true cargo build --release

echo "Restarting mira-http service..."
sudo systemctl restart mira-http

echo "Done. Checking status..."
sleep 2
sudo systemctl status mira-http --no-pager | head -10
