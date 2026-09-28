#!/usr/bin/env bash
#
# Build a shareable Windows installer. Run this ON the Windows laptop.
#
# Why a script and not a documented command: the two ways this has to go wrong
# are both silent. A stale Rust toolchain fails deep inside a dependency, and a
# frontend `dist/` that is out of date gets bundled into an installer that then
# ships the wrong UI. Both are checked here before the slow part starts.
#
# Needs: Node 20+, Rust (MSVC toolchain), and Visual Studio Build Tools with the
# "Desktop development with C++" workload. `winget` can install the last one.
#
#   winget install Microsoft.VisualStudio.2022.BuildTools
#   winget install Rustlang.Rustup
#   winget install OpenJS.NodeJS.LTS
#
# Re-run after installing any of those, in a NEW terminal.

set -euo pipefail

cd "$(dirname "$0")/.."
ROOT="$(pwd)"

red()   { printf '\033[31m%s\033[0m\n' "$*"; }
green() { printf '\033[32m%s\033[0m\n' "$*"; }
info()  { printf '\033[36m==>\033[0m %s\n' "$*"; }

# --- Is this even the right machine? ---------------------------------------
# The single most likely failure is running this on the Mac, where it appears
# to start and then produces no installer.
case "$(uname -s)" in
  MINGW*|MSYS*|CYGWIN*|Windows_NT) ;;
  *)
    if [[ "$(uname -s)" == "Darwin" ]]; then
      red "This script builds the WINDOWS installer and must run on Windows."
      red "A Tauri app cannot be cross-compiled to Windows from macOS: the build"
      red "needs the MSVC toolchain, the Windows SDK and NSIS."
      red ""
      red "On the Mac, use .github/workflows/windows-installer.yml via GitHub Actions."
    else
      red "Unrecognised platform: $(uname -s). This script is Windows-only."
    fi
    exit 1
    ;;
esac
green "Running on Windows: OK"

# --- Toolchain -------------------------------------------------------------
info "Checking the toolchain"
command -v node >/dev/null || { red "Node is not installed. See the header of this script."; exit 1; }
command -v cargo >/dev/null || {
  red "Rust is not on PATH. Install rustup, then restart this terminal:"
  red "  winget install Rustlang.Rustup"
  exit 1
}

NODE_MAJOR="$(node -p 'process.versions.node.split(".")[0]')"
if (( NODE_MAJOR < 20 )); then
  red "Node ${NODE_MAJOR} is too old; this project needs 20+."
  exit 1
fi
info "  node $(node -v)"
info "  cargo $(cargo --version)"

# The MSVC host triple. Without a host toolchain set, cargo defaults to the
# GNU toolchain on Windows and the Tauri build fails at link time with errors
# that do not mention the real cause.
HOST="$(rustc -vV | awk '/^host:/ {print $2}')"
if [[ "$HOST" != *msvc* ]]; then
  red "Default Rust host is '$HOST', which is not MSVC."
  red "Tauri on Windows needs the MSVC toolchain. Install Visual Studio Build"
  red "Tools with the 'Desktop development with C++' workload and run"
  red "'rustup default stable-x86_64-pc-windows-msvc'."
  exit 1
fi
info "  rust host $HOST (MSVC: OK)"

# --- Dependencies ----------------------------------------------------------
info "Installing frontend dependencies"
npm ci

# --- The two silent failures, checked before the slow build ---------------
info "Checking the build inputs"
if [[ ! -f src-tauri/app/tauri.conf.json ]]; then
  red "tauri.conf.json is missing from src-tauri/app/."
  exit 1
fi
if [[ ! -f src-tauri/icons/icon.ico ]]; then
  red "src-tauri/icons/icon.ico is missing; the Windows bundle cannot be built."
  exit 1
fi
# A pinned aarch64-apple-darwin target in .cargo/config.toml is exactly the
# kind of thing that looks harmless and makes every cargo command here fail
# before it starts. The file is committed with no [build] target on purpose.
# Comments are stripped first: the file's own header discusses the setting it
# deliberately does not have, and a naive grep matches that prose.
if grep -vE '^[[:space:]]*#' .cargo/config.toml 2>/dev/null \
   | grep -qE '^[[:space:]]*target[[:space:]]*='; then
  red ".cargo/config.toml pins a build target, so this would try to compile for"
  red "the wrong platform. Remove the [build] target line."
  exit 1
fi
info "  inputs OK"

# --- Build -----------------------------------------------------------------
# beforeBuildCommand (npm run build) runs the frontend build, so dist/ is
# regenerated here and cannot be stale.
info "Building the Windows installer. This takes 5-20 minutes the first time."
set -x
npx tauri build --config src-tauri/app/tauri.conf.json --target "$HOST"
set +x

BUNDLE="src-tauri/target/${HOST}/release/bundle"
green ""
green "Done. Installers:"
find "$BUNDLE" -type f \( -name '*.exe' -o -name '*.msi' \) -print 2>/dev/null || {
  red "No installer found under $BUNDLE. Check the build output above."
  exit 1
}

cat <<'EOF'

Next, on BOTH machines:
  - Allow the app through the firewall on private networks. Discovery is mDNS
    (UDP 5353); without this the machines simply never see each other and the
    device list stays empty. This is the most common reason pairing fails.
  - Sign out and back in, or the firewall rule may not apply until then.

Then pair: Devices -> Pair a device, and compare the safety number out loud.
EOF
