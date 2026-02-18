#!/bin/bash
# release.sh
set -e

# Usage: ./release.sh 0.8.5

VERSION=$1

if [ -z "$VERSION" ]; then
    echo "Usage: ./release.sh <version>"
    echo "Example: ./release.sh 0.8.5"
    exit 1
fi

# Validate version format
if ! [[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    echo "Error: Version must be in format X.Y.Z (e.g., 0.8.5)"
    exit 1
fi

echo "Releasing v$VERSION..."
echo ""

# --- Pre-flight checks ---

echo "Pre-flight: cargo fmt"
cargo fmt
if ! git diff --quiet; then
    echo "  Formatted files changed, committing..."
    git add -A
    git commit -m "style: cargo fmt"
fi

echo "Pre-flight: cargo clippy"
cargo clippy --all-targets --all-features -- -D warnings

echo "Pre-flight: cargo test"
cargo test

echo "Pre-flight: CHANGELOG.md"
if ! grep -q "## \[$VERSION\]" CHANGELOG.md; then
    echo "  Error: No CHANGELOG.md entry for [$VERSION]"
    echo "  Add a '## [$VERSION]' section before releasing."
    exit 1
fi

echo ""
echo "Pre-flight passed."
echo ""

# --- Version bump ---

echo "  Updating crates/mira-server/Cargo.toml"
sed -i "s/^version = \".*\"/version = \"$VERSION\"/" crates/mira-server/Cargo.toml

echo "  Updating crates/mira-types/Cargo.toml"
sed -i "s/^version = \".*\"/version = \"$VERSION\"/" crates/mira-types/Cargo.toml

echo "  Updating plugin/.claude-plugin/plugin.json"
sed -i "s/\"version\": \".*\"/\"version\": \"$VERSION\"/" plugin/.claude-plugin/plugin.json

echo "  Updating plugin/bin/mira-wrapper"
sed -i "s/^MIRA_VERSION=\".*\"/MIRA_VERSION=\"$VERSION\"/" plugin/bin/mira-wrapper

echo "  Updating .claude-plugin/marketplace.json"
sed -i "s/\"version\": \".*\"/\"version\": \"$VERSION\"/" .claude-plugin/marketplace.json

echo "  Updating Cargo.lock"
cargo check --quiet

# Commit, tag, push
git add crates/mira-server/Cargo.toml crates/mira-types/Cargo.toml Cargo.lock plugin/.claude-plugin/plugin.json .claude-plugin/marketplace.json plugin/bin/mira-wrapper
git commit -m "chore: bump version to $VERSION"

git tag "v$VERSION"
git push origin main
git push origin "v$VERSION"

echo ""
echo "Done! Release v$VERSION triggered."
echo "Watch the build: https://github.com/ConaryLabs/Mira/actions/workflows/release.yml"
