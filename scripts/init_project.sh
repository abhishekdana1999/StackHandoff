#!/bin/bash
set -e

echo "=== Initializing Workspace Clone Tauri 2 Project ==="

# Check prerequisites
echo "Checking prerequisites..."
command -v cargo >/dev/null 2>&1 || { echo "Rust/cargo not found. Install from rustup.rs"; exit 1; }
command -v node >/dev/null 2>&1 || { echo "Node.js not found. Install from nodejs.org"; exit 1; }
command -v npm >/dev/null 2>&1 || { echo "npm not found"; exit 1; }

echo "Rust: $(cargo --version)"
echo "Node: $(node --version)"
echo "npm: $(npm --version)"

# Install Tauri CLI
echo "Installing Tauri CLI..."
cargo install tauri-cli --version "^2.0.0" --locked 2>/dev/null || cargo install tauri-cli --version "^2.0.0"

# Create Tauri 2 project with React + TypeScript
echo "Creating Tauri 2 project..."
cargo tauri init --name "Workspace Clone" --identifier "com.workspaceclone.app" --template "react-ts" --force

echo "=== Project initialized ==="
ls -la
