#!/bin/sh
# Setup script for development environment
# Run this once after cloning the repository

set -e

echo "🔧 Setting up development environment..."

# Install pre-commit hook
HOOK_PATH=".git/hooks/pre-commit"
SCRIPT_PATH="scripts/pre-commit"

if [ -f "$SCRIPT_PATH" ]; then
    cp "$SCRIPT_PATH" "$HOOK_PATH"
    chmod +x "$HOOK_PATH"
    echo "✓ Pre-commit hook installed"
else
    echo "❌ Pre-commit script not found at $SCRIPT_PATH"
    exit 1
fi

# Verify Rust toolchain
if command -v rustup &> /dev/null; then
    echo "✓ Rust toolchain found"
    
    # Ensure clippy and rustfmt are installed
    rustup component add clippy rustfmt 2>/dev/null || true
    echo "✓ clippy and rustfmt components ready"
else
    echo "⚠️  rustup not found. Please install Rust from https://rustup.rs"
fi

echo ""
echo "✅ Setup complete!"
echo ""
echo "The pre-commit hook will now run 'cargo fmt --check' and 'cargo clippy'"
echo "before each commit. To skip (not recommended): git commit --no-verify"
