#!/bin/bash

# Name your output file (customize as needed)
ZIP_NAME="mira_frontend_debug_$(date +%Y%m%d_%H%M%S).zip"

# From project root, zip everything EXCEPT the usual suspects
zip -r "$ZIP_NAME" . \
    -x "node_modules/*" \
    -x "dist/*" \
    -x "build/*" \
    -x ".git/*" \
    -x "*.zip" \
    -x ".env" \
    -x ".DS_Store"

echo "Zipped up the project as $ZIP_NAME"
