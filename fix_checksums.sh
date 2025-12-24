#!/bin/bash
set -e

DB="/home/peter/Mira/data/mira.db"
MIGRATIONS_DIR="/home/peter/Mira/migrations"

# Mark a migration as applied
mark_applied() {
    version=$1
    file=$(ls ${MIGRATIONS_DIR}/${version}_*.sql 2>/dev/null)
    if [ -n "$file" ]; then
        checksum=$(openssl dgst -sha384 -binary "$file" | xxd -p | tr -d '\n')
        sqlite3 "$DB" "INSERT OR REPLACE INTO _sqlx_migrations (version, description, installed_on, success, checksum, execution_time) VALUES ($version, 'memory decay', datetime('now'), 1, X'$checksum', 0)"
        echo "Marked $version as applied"
    else
        echo "File not found for $version"
    fi
}

mark_applied 20251224000000

echo "Done!"
