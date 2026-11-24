#!/usr/bin/env bash
# Build Lap with Cargo's `release-fast` profile (thin LTO, parallel codegen)
# and install/relaunch the macOS `.app` bundle.
#
# Usage:
#   ./scripts/build_install.sh [options]
#
# Run `./scripts/build_install.sh --help` for options.

set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "$SCRIPT_DIR/.." && pwd)"

PROFILE="release-fast"
INSTALL_DIR="${LAP_INSTALL_DIR:-$HOME/Applications}"
TARGET_DIR="${CARGO_TARGET_DIR:-}"
DO_INSTALL=1
DO_LAUNCH=1
DRY_RUN=0

usage() {
  cat <<'EOF'
Build and install Lap locally on macOS (defaults to the fast `release-fast` profile).

Usage:
  ./scripts/build_install.sh [options]

Options:
  --profile <name>      Cargo profile to build (default: release-fast; e.g. release)
  --target-dir <dir>    Cargo target directory (default: $CARGO_TARGET_DIR, or
                        /Volumes/Builds/lap/target if writable, else src-tauri/target)
  --install-dir <dir>   Directory where Lap.app is installed (default: ~/Applications)
  --no-install          Only build Lap.app; skip installing and relaunching
  --no-launch           Install and verify Lap.app, but do not quit/relaunch it
  --dry-run             Print the resolved build/install plan without executing
  -h, --help            Show this help message

Examples:
  ./scripts/build_install.sh
  ./scripts/build_install.sh --no-launch
  ./scripts/build_install.sh --profile release --install-dir /Applications
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --profile)
      [[ $# -ge 2 ]] || { echo "Error: --profile requires a value" >&2; exit 2; }
      PROFILE="$2"
      shift 2
      ;;
    --target-dir)
      [[ $# -ge 2 ]] || { echo "Error: --target-dir requires a value" >&2; exit 2; }
      TARGET_DIR="$2"
      shift 2
      ;;
    --install-dir)
      [[ $# -ge 2 ]] || { echo "Error: --install-dir requires a value" >&2; exit 2; }
      INSTALL_DIR="$2"
      shift 2
      ;;
    --no-install)
      DO_INSTALL=0
      DO_LAUNCH=0
      shift
      ;;
    --no-launch)
      DO_LAUNCH=0
      shift
      ;;
    --dry-run)
      DRY_RUN=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Error: unknown option: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

if [[ -z "$TARGET_DIR" ]]; then
  if [[ -d "/Volumes/Builds" && -w "/Volumes/Builds" ]]; then
    TARGET_DIR="/Volumes/Builds/lap/target"
  else
    TARGET_DIR="$REPO_ROOT/src-tauri/target"
  fi
fi

# Pick frontend build command (matches tauri.conf.json when pnpm is present, else npm).
if command -v pnpm >/dev/null 2>&1; then
  FRONTEND_BUILD_CMD="pnpm --dir $REPO_ROOT/src-vite build"
elif command -v npm >/dev/null 2>&1; then
  FRONTEND_BUILD_CMD="npm --prefix $REPO_ROOT/src-vite run build"
else
  echo "Error: neither pnpm nor npm was found in PATH." >&2
  exit 1
fi

# Resolve the Tauri CLI binary.
TAURI_CMD=()
if command -v cargo-tauri >/dev/null 2>&1; then
  TAURI_CMD=(cargo tauri)
elif command -v tauri >/dev/null 2>&1; then
  TAURI_CMD=(tauri)
else
  cached_tauri=""
  if [[ -d "$HOME/.npm/_npx" ]]; then
    # Pick the most recently modified cached npx tauri binary if one exists.
    cached_tauri="$(ls -t "$HOME"/.npm/_npx/*/node_modules/.bin/tauri 2>/dev/null | head -n 1 || true)"
  fi
  if [[ -n "$cached_tauri" && -x "$cached_tauri" ]]; then
    TAURI_CMD=("$cached_tauri")
  elif command -v npx >/dev/null 2>&1; then
    TAURI_CMD=(npx --yes @tauri-apps/cli@2)
  else
    echo "Error: Tauri CLI not found. Install it with 'cargo install tauri-cli --version ^2.0.0 --locked' or install Node.js/npx." >&2
    exit 1
  fi
fi

BUNDLE_APP="$TARGET_DIR/$PROFILE/bundle/macos/Lap.app"
INSTALL_APP="$INSTALL_DIR/Lap.app"
TAURI_CONFIG_JSON="{\"build\":{\"beforeBuildCommand\":\"$FRONTEND_BUILD_CMD\"},\"bundle\":{\"createUpdaterArtifacts\":false}}"

echo "Lap local build & install"
echo "  Repo:        $REPO_ROOT"
echo "  Profile:     $PROFILE"
echo "  Target dir:  $TARGET_DIR"
echo "  Tauri CLI:   ${TAURI_CMD[*]}"
echo "  Frontend:    $FRONTEND_BUILD_CMD"
echo "  Bundle out:  $BUNDLE_APP"
if [[ "$DO_INSTALL" -eq 1 ]]; then
  echo "  Install to:  $INSTALL_APP"
fi

if [[ "$DRY_RUN" -eq 1 ]]; then
  exit 0
fi

cd "$REPO_ROOT"
CARGO_TARGET_DIR="$TARGET_DIR" "${TAURI_CMD[@]}" build \
  --bundles app \
  --config "$TAURI_CONFIG_JSON" \
  -- --profile "$PROFILE"

if [[ ! -d "$BUNDLE_APP" ]]; then
  echo "Error: expected bundle not found at $BUNDLE_APP" >&2
  exit 1
fi

if [[ "$DO_INSTALL" -eq 0 ]]; then
  echo "Done (skipped install): $BUNDLE_APP"
  exit 0
fi

mkdir -p "$INSTALL_DIR"
echo "Installing to $INSTALL_APP..."
rsync -a --delete --checksum "$BUNDLE_APP/" "$INSTALL_APP/"
diff -qr "$BUNDLE_APP" "$INSTALL_APP"
echo "Verified $INSTALL_APP matches build output."

if [[ "$DO_LAUNCH" -eq 1 && "$(uname -s)" == "Darwin" ]]; then
  echo "Relaunching Lap..."
  osascript -e 'tell application "Lap" to quit' 2>/dev/null || true
  sleep 1
  open -a "$INSTALL_APP"
fi

echo "Done."
