#!/usr/bin/env bash
# Build DBcooper on this machine and publish a GitHub release from the
# version in src-tauri/tauri.conf.json.
#
# Usage:
#   bun run release              # build + draft release
#   bun run release -- --publish # build + published release
#   bun run release -- --skip-build
#
# Updater signatures need the minisign private key that matches
# plugins.updater.pubkey in tauri.conf.json:
#   export TAURI_SIGNING_PRIVATE_KEY="$(cat ~/.tauri/dbcooper.key)"
#   export TAURI_SIGNING_PRIVATE_KEY_PASSWORD=""   # if the key has none

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

DRAFT=true
SKIP_BUILD=false
NO_UPDATER=false
REPO=""
TARGET="aarch64-apple-darwin"

usage() {
  cat <<EOF
Build DBcooper locally and upload a GitHub release.

Options:
  --publish       Make the GitHub release public (default is draft)
  --skip-build    Reuse artifacts from the last tauri build
  --no-updater    Skip updater signatures (dmg only; no TAURI_SIGNING_PRIVATE_KEY)
  --repo OWNER/NAME
                  GitHub repo (default: origin)
  --target TRIPLE Rust target (default: aarch64-apple-darwin)
  -h, --help
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --publish) DRAFT=false; shift ;;
    --skip-build) SKIP_BUILD=true; shift ;;
    --no-updater) NO_UPDATER=true; shift ;;
    --repo) REPO="$2"; shift 2 ;;
    --target) TARGET="$2"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "Unknown option: $1" >&2; usage; exit 1 ;;
  esac
done

if ! command -v gh >/dev/null 2>&1; then
  echo "gh is required (https://cli.github.com/)" >&2
  exit 1
fi
if ! gh auth status >/dev/null 2>&1; then
  echo "gh is not authenticated. Run: gh auth login" >&2
  exit 1
fi

if [[ -z "$REPO" ]]; then
  REPO="$(gh repo view --json nameWithOwner -q .nameWithOwner)"
fi

VERSION="$(node -p "require('./src-tauri/tauri.conf.json').version")"
if [[ ! "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "Invalid version in src-tauri/tauri.conf.json: $VERSION" >&2
  exit 1
fi
TAG="v$VERSION"

if [[ -z "${TAURI_SIGNING_PRIVATE_KEY:-}" && -f "$HOME/.tauri/dbcooper.key" ]]; then
  TAURI_SIGNING_PRIVATE_KEY="$(cat "$HOME/.tauri/dbcooper.key")"
  export TAURI_SIGNING_PRIVATE_KEY
fi
if [[ -z "${TAURI_SIGNING_PRIVATE_KEY_PASSWORD:-}" && -f "$HOME/.tauri/dbcooper.key.password" ]]; then
  TAURI_SIGNING_PRIVATE_KEY_PASSWORD="$(cat "$HOME/.tauri/dbcooper.key.password")"
  export TAURI_SIGNING_PRIVATE_KEY_PASSWORD
fi

if [[ "$NO_UPDATER" != true && -z "${TAURI_SIGNING_PRIVATE_KEY:-}" ]]; then
  cat >&2 <<EOF
TAURI_SIGNING_PRIVATE_KEY is not set.

Updater artifacts (.sig + latest.json) require the minisign private key that
matches plugins.updater.pubkey in src-tauri/tauri.conf.json.

  bunx tauri signer generate -w ~/.tauri/dbcooper.key
  export TAURI_SIGNING_PRIVATE_KEY="\$(cat ~/.tauri/dbcooper.key)"
  export TAURI_SIGNING_PRIVATE_KEY_PASSWORD=""

If you generate a new key, update pubkey in tauri.conf.json before releasing.

To ship a .dmg without updater signatures:
  bun run release -- --no-updater
EOF
  exit 1
fi
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD="${TAURI_SIGNING_PRIVATE_KEY_PASSWORD:-}"

if [[ "$SKIP_BUILD" != true ]]; then
  echo "==> bun install"
  bun install
  echo "==> tauri build --target $TARGET"
  BUILD_ARGS=(build --target "$TARGET" --ci)
  if [[ "$NO_UPDATER" == true ]]; then
    BUILD_ARGS+=(--config '{"bundle":{"createUpdaterArtifacts":false}}')
  fi
  bun run tauri "${BUILD_ARGS[@]}"
fi

BUNDLE_ROOTS=(
  "$ROOT/src-tauri/target/${TARGET}/release/bundle"
  "$ROOT/src-tauri/target/release/bundle"
)

find_one() {
  local pattern="$1"
  local match=""
  local root
  for root in "${BUNDLE_ROOTS[@]}"; do
    [[ -d "$root" ]] || continue
    match="$(find "$root" -type f -name "$pattern" | head -n 1 || true)"
    if [[ -n "$match" ]]; then
      printf '%s' "$match"
      return 0
    fi
  done
  return 1
}

DMG="$(find_one "*.dmg" || true)"
APP_TAR="$(find_one "*.app.tar.gz" || true)"
APP_SIG="$(find_one "*.app.tar.gz.sig" || true)"

if [[ -z "$DMG" ]]; then
  echo "No .dmg found under bundle/. Run without --skip-build." >&2
  exit 1
fi

STAGING="$(mktemp -d)"
trap 'rm -rf "$STAGING"' EXIT

cp "$DMG" "$STAGING/"
ASSETS=("$STAGING/$(basename "$DMG")")

if [[ -n "$APP_TAR" ]]; then
  cp "$APP_TAR" "$STAGING/"
  ASSETS+=("$STAGING/$(basename "$APP_TAR")")
fi
if [[ -n "$APP_SIG" ]]; then
  cp "$APP_SIG" "$STAGING/"
  ASSETS+=("$STAGING/$(basename "$APP_SIG")")
fi

if [[ -n "$APP_TAR" && -n "$APP_SIG" ]]; then
  TAR_NAME="$(basename "$APP_TAR")"
  SIGNATURE="$(tr -d '\n' < "$APP_SIG")"
  PUB_DATE="$(date -u +"%Y-%m-%dT%H:%M:%SZ")"
  cat > "$STAGING/latest.json" <<EOF
{
  "version": "$VERSION",
  "notes": "See the GitHub release for details.",
  "pub_date": "$PUB_DATE",
  "platforms": {
    "darwin-aarch64": {
      "signature": "$SIGNATURE",
      "url": "https://github.com/${REPO}/releases/download/${TAG}/${TAR_NAME}"
    }
  }
}
EOF
  ASSETS+=("$STAGING/latest.json")
else
  echo "Warning: updater tarball/signature missing; latest.json will not be uploaded." >&2
fi

if ! git rev-parse "$TAG" >/dev/null 2>&1; then
  echo "==> creating git tag $TAG"
  git tag -a "$TAG" -m "Release $TAG"
fi
if ! git ls-remote --tags origin "refs/tags/$TAG" | grep -q "$TAG"; then
  echo "==> pushing tag $TAG"
  git push origin "$TAG"
fi

NOTES="$(mktemp)"
{
  echo "Local release of DBcooper $TAG (macOS Apple Silicon)."
  echo
  echo "macOS users: after installing, bypass Gatekeeper once:"
  echo
  echo '```'
  echo "xattr -cr /Applications/DBcooper.app"
  echo '```'
} > "$NOTES"

DRAFT_ARGS=()
if [[ "$DRAFT" == true ]]; then
  DRAFT_ARGS+=(--draft)
fi

if gh release view "$TAG" --repo "$REPO" >/dev/null 2>&1; then
  echo "==> uploading assets to existing $TAG on $REPO"
  gh release upload "$TAG" "${ASSETS[@]}" --repo "$REPO" --clobber
  if [[ "$DRAFT" != true ]]; then
    gh release edit "$TAG" --repo "$REPO" --draft=false
  fi
else
  echo "==> creating GitHub release $TAG on $REPO"
  gh release create "$TAG" "${ASSETS[@]}" \
    --repo "$REPO" \
    --title "$TAG" \
    --notes-file "$NOTES" \
    "${DRAFT_ARGS[@]}"
fi
rm -f "$NOTES"

echo
echo "Release: https://github.com/${REPO}/releases/tag/${TAG}"
if [[ "$DRAFT" == true ]]; then
  echo "This is a draft. Publish it in the GitHub UI when the assets look right."
fi
