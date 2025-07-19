#!/bin/bash

ZIP_NAME="mira_backend_clean_$(date +%Y%m%d_%H%M%S).zip"

zip -r "$ZIP_NAME" . \
    -x "target/*" \
    -x ".git/*" \
    -x "*.db-shm" \
    -x "*.db-wal" \
    -x "*.log" \
    -x "*.zip" \
    -x "*.env" \
    -x "mira.db" \
    -x "frontend/node_modules/*" \
    -x ".DS_Store" \
    -x "__pycache__/*" \
    -x "*.pyc"

echo "Created $ZIP_NAME"
