#!/usr/bin/env bash
set -euo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VERSION=""
TARGET="aarch64-apple-darwin"
OUTPUT_DIR="dist"
BINARY=""
SKIP_BUILD=false
while [[ $# -gt 0 ]]; do
  case "$1" in
    --version) VERSION="${2:?missing version}"; shift 2 ;;
    --target) TARGET="${2:?missing target}"; shift 2 ;;
    --output-dir) OUTPUT_DIR="${2:?missing output directory}"; shift 2 ;;
    --binary) BINARY="${2:?missing binary}"; shift 2 ;;
    --skip-build) SKIP_BUILD=true; shift ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done
cd "$ROOT_DIR"
[[ "$TARGET" == aarch64-apple-darwin ]] || { echo "Only macOS arm64 is supported" >&2; exit 2; }
PACKAGE_VERSION="$(python3 -c 'import tomllib; print(tomllib.load(open("Cargo.toml", "rb"))["package"]["version"])')"
VERSION="${VERSION:-v$PACKAGE_VERSION}"
[[ "$VERSION" == "v$PACKAGE_VERSION" ]] || { echo "Tag and Cargo version differ" >&2; exit 1; }
python3 scripts/release_notes.py --check-current
if [[ "$SKIP_BUILD" == false ]]; then
  [[ -z "$BINARY" ]] || { echo "--binary requires --skip-build" >&2; exit 2; }
  cargo build --locked --release --target "$TARGET" --bin tiny-proxy >&2
  BINARY="$ROOT_DIR/target/$TARGET/release/tiny-proxy"
else
  [[ -n "$BINARY" ]] || { echo "--skip-build requires --binary" >&2; exit 2; }
fi
[[ "$BINARY" == /* ]] || BINARY="$ROOT_DIR/$BINARY"
[[ -f "$BINARY" ]] || { echo "Missing executable: $BINARY" >&2; exit 1; }
[[ "$(lipo -archs "$BINARY")" == arm64 ]] || { echo "Executable must be arm64" >&2; exit 1; }
[[ "$("$BINARY" --version)" == "Tiny Proxy $PACKAGE_VERSION" ]] || { echo "Executable version differs from Cargo.toml" >&2; exit 1; }
[[ "$OUTPUT_DIR" == /* ]] || OUTPUT_DIR="$ROOT_DIR/$OUTPUT_DIR"
mkdir -p "$OUTPUT_DIR"
OUTPUT_DIR="$(cd "$OUTPUT_DIR" && pwd)"
APP_DIR="$OUTPUT_DIR/TinyProxy.app"
[[ ! -e "$APP_DIR" ]] || { echo "Refusing to overwrite $APP_DIR" >&2; exit 1; }
mkdir -p "$APP_DIR/Contents/MacOS" "$APP_DIR/Contents/Resources"
cp "$BINARY" "$APP_DIR/Contents/MacOS/tiny-proxy"
chmod 0755 "$APP_DIR/Contents/MacOS/tiny-proxy"
VERSION_CORE="${PACKAGE_VERSION%%-*}"
VERSION_CORE="${VERSION_CORE%%+*}"
sed -e "s/{{VERSION_CORE}}/$VERSION_CORE/g" -e "s/{{VERSION}}/$PACKAGE_VERSION/g" assets/macos/Info.plist > "$APP_DIR/Contents/Info.plist"
printf 'APPL????' > "$APP_DIR/Contents/PkgInfo"
# Build ICNS from the project's existing PNG, without introducing new artwork.
ICONSET="$(mktemp -d)/tiny-proxy.iconset"
mkdir -p "$ICONSET"
for SIZE in 16 32 128 256 512; do
  sips -z "$SIZE" "$SIZE" assets/icons/tiny-proxy.png --out "$ICONSET/icon_${SIZE}x${SIZE}.png" >/dev/null
  DOUBLE="$((SIZE * 2))"
  sips -z "$DOUBLE" "$DOUBLE" assets/icons/tiny-proxy.png --out "$ICONSET/icon_${SIZE}x${SIZE}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$APP_DIR/Contents/Resources/tiny-proxy.icns"
rm -rf "$(dirname "$ICONSET")"
plutil -lint "$APP_DIR/Contents/Info.plist" >&2
# Only system libraries may remain unbundled.
if otool -L "$APP_DIR/Contents/MacOS/tiny-proxy" | tail -n +2 | awk '{print $1}' | grep -Ev '^(/System/Library/|/usr/lib/)' >&2; then
  echo "Application has an unbundled dynamic library dependency" >&2; exit 1
fi
codesign --force --deep --sign "${SIGN_IDENTITY:--}" "$APP_DIR" >&2
codesign --verify --deep --strict "$APP_DIR" >&2
echo "$APP_DIR"
