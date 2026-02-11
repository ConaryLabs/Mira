#!/bin/bash
set -e

# Simple release script
# Usage: ./release.sh 0.3.3

VERSION=$1

if [ -z "$VERSION" ]; then
    echo "Usage: ./release.sh <version>"
    echo "Example: ./release.sh 0.3.3"
    exit 1
fi

# Validate version format
if ! [[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    echo "Error: Version must be in format X.Y.Z (e.g., 0.3.3)"
    exit 1
fi

echo "Releasing v$VERSION..."

# Update version in Cargo.toml
echo "  Updating crates/mira-server/Cargo.toml"
sed -i "s/^version = \".*\"/version = \"$VERSION\"/" crates/mira-server/Cargo.toml

echo "  Updating crates/mira-types/Cargo.toml"
sed -i "s/^version = \".*\"/version = \"$VERSION\"/" crates/mira-types/Cargo.toml

# Update version in plugin.json
echo "  Updating plugin/.claude-plugin/plugin.json"
sed -i "s/\"version\": \".*\"/\"version\": \"$VERSION\"/" plugin/.claude-plugin/plugin.json

# Update version in mira-wrapper
echo "  Updating plugin/bin/mira-wrapper"
sed -i "s/^MIRA_VERSION=\".*\"/MIRA_VERSION=\"$VERSION\"/" plugin/bin/mira-wrapper

# Update version in marketplace.json
echo "  Updating .claude-plugin/marketplace.json"
sed -i "s/\"version\": \".*\"/\"version\": \"$VERSION\"/" .claude-plugin/marketplace.json

# Update Cargo.lock
echo "  Updating Cargo.lock"
cargo check --quiet

# Commit
git add crates/mira-server/Cargo.toml crates/mira-types/Cargo.toml Cargo.lock plugin/.claude-plugin/plugin.json .claude-plugin/marketplace.json plugin/bin/mira-wrapper
git commit -m "chore: bump version to $VERSION"

# Tag and push
git tag "v$VERSION"
git push origin main
git push origin "v$VERSION"

echo ""
echo "Done! Release v$VERSION triggered."
echo "Watch the build: https://github.com/ConaryLabs/Mira/actions/workflows/release.yml"
echo ""
echo "Don't forget to update CHANGELOG.md"
