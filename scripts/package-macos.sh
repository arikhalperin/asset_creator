#!/usr/bin/env bash
# Builds "Game Asset Creator.app" for Apple Silicon macOS with its own
# decode-only LGPL ffmpeg and Microsoft's ONNX Runtime, ad-hoc signs it, and
# writes a .zip and a .dmg to target/package/.
#
# Usage: scripts/package-macos.sh [--skip-ffmpeg-build]
#
#   --skip-ffmpeg-build  reuse the cached ffmpeg build under target/ffmpeg-lgpl
#                        (fails if it is missing) instead of building it when
#                        absent.
#
# Network access: only the pinned ffmpeg source tarball and the pinned ONNX
# Runtime release, both checked against SHA-256 pins below. Downloads and the
# ffmpeg build are cached under target/ and reused on later runs.
set -euo pipefail

# ---------------------------------------------------------------- pins ----
APP_NAME="Game Asset Creator"
EXE_NAME="game-asset-creator"
BUNDLE_ID="io.github.game-asset-creator"
APP_VERSION="1.0.0"
MIN_MACOS="13.0"

# ffmpeg 7.1.x: the series the app is developed and tested against
# (ffmpeg-next 8 / ffmpeg-sys-next 8.1 support it). SHA-256 cross-checked
# against Homebrew's ffmpeg@7 formula.
FFMPEG_VERSION="7.1.5"
FFMPEG_URL="https://ffmpeg.org/releases/ffmpeg-${FFMPEG_VERSION}.tar.xz"
FFMPEG_SHA256="de668509caf9e35e3cd162473441fdb29538c6d96ed080292b3cf9e6fc5d558f"

# ONNX Runtime 1.31.0: the release ort 2.0.0-rc.14 is built for (it needs at
# least API 17, i.e. 1.17). SHA-256 from the GitHub release asset digest.
ORT_VERSION="1.31.0"
ORT_URL="https://github.com/microsoft/onnxruntime/releases/download/v${ORT_VERSION}/onnxruntime-osx-arm64-${ORT_VERSION}.tgz"
ORT_SHA256="1031b36dd3bdaa8d976a6130d9cf2a05e9be320b93ae804f8427b91dc3432afc"

# --------------------------------------------------------------- setup ----
SKIP_FFMPEG_BUILD=0
for arg in "$@"; do
    case "$arg" in
        --skip-ffmpeg-build) SKIP_FFMPEG_BUILD=1 ;;
        -h | --help)
            sed -n '2,15p' "$0" | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        *)
            echo "unknown argument: $arg" >&2
            exit 2
            ;;
    esac
done

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET="$ROOT/target"
DOWNLOADS="$TARGET/package-downloads"
FFMPEG_PREFIX="$TARGET/ffmpeg-lgpl/$FFMPEG_VERSION"
ORT_DIR="$TARGET/onnxruntime/onnxruntime-osx-arm64-$ORT_VERSION"
BUILD_DIR="$TARGET/package-build"
OUT="$TARGET/package"
APP="$OUT/$APP_NAME.app"
ARTIFACT_BASE="Game-Asset-Creator-$APP_VERSION-arm64"

log() { printf '\n==> %s\n' "$*"; }
info() { printf '    %s\n' "$*"; }
die() {
    printf '\nERROR: %s\n' "$*" >&2
    exit 1
}

[[ "$(uname -s)" == Darwin ]] || die "this script runs on macOS only"
[[ "$(uname -m)" == arm64 ]] || die "this script builds for Apple Silicon (arm64) only"
for tool in curl shasum tar make clang cargo install_name_tool otool codesign ditto hdiutil plutil; do
    command -v "$tool" >/dev/null || die "required tool not found: $tool"
done

export MACOSX_DEPLOYMENT_TARGET="$MIN_MACOS"
JOBS="$(sysctl -n hw.ncpu)"

# Downloads $1 to $DOWNLOADS/$2 (unless cached) and checks its SHA-256 ($3).
fetch() {
    local url="$1" file="$DOWNLOADS/$2" sha="$3"
    mkdir -p "$DOWNLOADS"
    if [[ -f "$file" ]] && echo "$sha  $file" | shasum -a 256 -c --status; then
        info "cached: $file"
        return
    fi
    info "downloading $url"
    rm -f "$file.part"
    curl -fL --retry 3 --proto '=https' --tlsv1.2 -o "$file.part" "$url"
    local got
    got="$(shasum -a 256 "$file.part" | awk '{print $1}')"
    if [[ "$got" != "$sha" ]]; then
        rm -f "$file.part"
        die "SHA-256 mismatch for $url: expected $sha, got $got"
    fi
    mv "$file.part" "$file"
    info "SHA-256 verified: $sha"
}

# ------------------------------------------------- 1. LGPL ffmpeg build ----
build_ffmpeg() {
    fetch "$FFMPEG_URL" "ffmpeg-$FFMPEG_VERSION.tar.xz" "$FFMPEG_SHA256"
    local src="$TARGET/ffmpeg-lgpl/src"
    rm -rf "$src" "$FFMPEG_PREFIX"
    mkdir -p "$src"
    tar -xf "$DOWNLOADS/ffmpeg-$FFMPEG_VERSION.tar.xz" -C "$src"
    (
        cd "$src/ffmpeg-$FFMPEG_VERSION"
        # Decode-only, LGPL: no GPL or non-free code, no programs, encoders,
        # muxers, devices or network. --disable-autodetect keeps Homebrew
        # libraries out; only system libraries/frameworks are enabled.
        ./configure \
            --prefix="$FFMPEG_PREFIX" \
            --arch=arm64 --cc=clang \
            --extra-cflags="-mmacosx-version-min=$MIN_MACOS" \
            --extra-ldflags="-mmacosx-version-min=$MIN_MACOS" \
            --disable-gpl --disable-nonfree --disable-version3 \
            --enable-shared --disable-static --enable-pic \
            --disable-programs --disable-doc \
            --disable-encoders --disable-muxers --disable-outdevs --disable-indevs \
            --disable-network --disable-protocols --enable-protocol=file,pipe \
            --disable-autodetect --enable-zlib --enable-bzlib \
            --enable-videotoolbox \
            --enable-avcodec --enable-avformat --enable-avutil --enable-avfilter \
            --enable-avdevice --enable-swscale --enable-swresample \
            >"$TARGET/ffmpeg-lgpl/configure.log" 2>&1 ||
            {
                tail -30 "$TARGET/ffmpeg-lgpl/configure.log" >&2
                exit 1
            }
        make -j"$JOBS" >"$TARGET/ffmpeg-lgpl/make.log" 2>&1 || {
            tail -40 "$TARGET/ffmpeg-lgpl/make.log" >&2
            exit 1
        }
        make install >>"$TARGET/ffmpeg-lgpl/make.log" 2>&1
        mkdir -p "$FFMPEG_PREFIX/share/licenses"
        cp LICENSE.md COPYING.LGPLv2.1 "$FFMPEG_PREFIX/share/licenses/"
    ) || die "ffmpeg build failed (logs in $TARGET/ffmpeg-lgpl)"
    rm -rf "$src"
    touch "$FFMPEG_PREFIX/.complete"
}

log "1/8 ffmpeg $FFMPEG_VERSION (LGPL, decode-only)"
if [[ -f "$FFMPEG_PREFIX/.complete" ]]; then
    info "cached: $FFMPEG_PREFIX"
elif [[ "$SKIP_FFMPEG_BUILD" == 1 ]]; then
    die "--skip-ffmpeg-build given but no cached build at $FFMPEG_PREFIX"
else
    build_ffmpeg
fi
grep -q -- "--enable-gpl\|--enable-nonfree" "$FFMPEG_PREFIX/lib/pkgconfig/libavcodec.pc" &&
    die "the ffmpeg build is configured with GPL/non-free parts"

# --------------------------------------------------- 2. ONNX Runtime ----
log "2/8 ONNX Runtime $ORT_VERSION (osx-arm64)"
if [[ -f "$ORT_DIR/.complete" ]]; then
    info "cached: $ORT_DIR"
else
    fetch "$ORT_URL" "onnxruntime-osx-arm64-$ORT_VERSION.tgz" "$ORT_SHA256"
    rm -rf "$ORT_DIR"
    mkdir -p "$(dirname "$ORT_DIR")"
    tar -xzf "$DOWNLOADS/onnxruntime-osx-arm64-$ORT_VERSION.tgz" -C "$(dirname "$ORT_DIR")"
    [[ -f "$ORT_DIR/lib/libonnxruntime.$ORT_VERSION.dylib" ]] || die "unexpected ONNX Runtime archive layout"
    touch "$ORT_DIR/.complete"
fi

# --------------------------------------------------- 3. release build ----
log "3/8 cargo build --release (against the LGPL ffmpeg)"
# Only the LGPL ffmpeg is visible to pkg-config, so no system/Homebrew ffmpeg
# can be linked by accident.
PKG_CONFIG_LIBDIR="$FFMPEG_PREFIX/lib/pkgconfig" \
    PKG_CONFIG_PATH="$FFMPEG_PREFIX/lib/pkgconfig" \
    CARGO_TARGET_DIR="$BUILD_DIR" \
    cargo build --release --locked --bin "$EXE_NAME" --manifest-path "$ROOT/Cargo.toml"
BIN="$BUILD_DIR/release/$EXE_NAME"
[[ -x "$BIN" ]] || die "release binary missing: $BIN"

# ------------------------------------------------------ 4. assemble ----
log "4/8 assembling $APP"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Frameworks" "$APP/Contents/Resources"
cp "$BIN" "$APP/Contents/MacOS/$EXE_NAME"
cp "$ROOT/assets/icon/AppIcon.icns" "$APP/Contents/Resources/AppIcon.icns"
cp "$ROOT/tests/fixtures/video/clip.mp4" "$APP/Contents/Resources/sample.mp4"
printf 'APPL????' >"$APP/Contents/PkgInfo"

# ffmpeg dylibs, copied under their install-name file names (e.g.
# libavcodec.61.dylib).
FFMPEG_LIBS=()
for lib in avutil swresample swscale avcodec avformat avfilter avdevice; do
    real="$( (ls "$FFMPEG_PREFIX/lib/lib$lib".*.*.*.dylib 2>/dev/null || true) | head -1)"
    [[ -n "$real" ]] || die "missing ffmpeg library lib$lib"
    name="$(basename "$(otool -D "$real" | tail -1)")"
    cp "$real" "$APP/Contents/Frameworks/$name"
    FFMPEG_LIBS+=("$name")
done
cp "$ORT_DIR/lib/libonnxruntime.$ORT_VERSION.dylib" "$APP/Contents/Frameworks/libonnxruntime.dylib"
cp "$ORT_DIR/LICENSE" "$APP/Contents/Resources/ONNXRUNTIME-LICENSE.txt"
cp "$FFMPEG_PREFIX/share/licenses/LICENSE.md" "$APP/Contents/Resources/FFMPEG-LICENSE.md"
cp "$FFMPEG_PREFIX/share/licenses/COPYING.LGPLv2.1" "$APP/Contents/Resources/FFMPEG-COPYING.LGPLv2.1.txt"
[[ -f "$ROOT/THIRD_PARTY_NOTICES.md" ]] && cp "$ROOT/THIRD_PARTY_NOTICES.md" "$APP/Contents/Resources/"
[[ -f "$ROOT/LICENSE" ]] && cp "$ROOT/LICENSE" "$APP/Contents/Resources/LICENSE.txt"

cat >"$APP/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleDevelopmentRegion</key>
    <string>en</string>
    <key>CFBundleExecutable</key>
    <string>$EXE_NAME</string>
    <key>CFBundleIdentifier</key>
    <string>$BUNDLE_ID</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>
    <key>CFBundleName</key>
    <string>$APP_NAME</string>
    <key>CFBundleDisplayName</key>
    <string>$APP_NAME</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>$APP_VERSION</string>
    <key>CFBundleVersion</key>
    <string>$APP_VERSION</string>
    <key>CFBundleIconFile</key>
    <string>AppIcon</string>
    <key>LSMinimumSystemVersion</key>
    <string>$MIN_MACOS</string>
    <key>LSApplicationCategoryType</key>
    <string>public.app-category.graphics-design</string>
    <key>NSHighResolutionCapable</key>
    <true/>
    <key>NSPrincipalClass</key>
    <string>NSApplication</string>
</dict>
</plist>
EOF
plutil -lint "$APP/Contents/Info.plist" >/dev/null || die "invalid Info.plist"

# ------------------------------------------------- 5. install names ----
log "5/8 rewriting install names to @rpath"
macho_files() {
    find "$APP/Contents" -type f \( -perm -u+x -o -name '*.dylib' \) -print0 |
        while IFS= read -r -d '' f; do
            file -b "$f" | grep -q 'Mach-O' && printf '%s\0' "$f"
        done
}
# Every non-system reference whose file name is bundled becomes @rpath/<name>.
rewrite_refs() {
    local f="$1" dep name
    otool -L "$f" | tail -n +2 | awk '{print $1}' | while read -r dep; do
        case "$dep" in
            /usr/lib/* | /System/* | @rpath/* | @executable_path/* | @loader_path/*) continue ;;
        esac
        name="$(basename "$dep")"
        if [[ -f "$APP/Contents/Frameworks/$name" ]]; then
            install_name_tool -change "$dep" "@rpath/$name" "$f" 2>/dev/null
        fi
    done
}
for name in "${FFMPEG_LIBS[@]}" libonnxruntime.dylib; do
    install_name_tool -id "@rpath/$name" "$APP/Contents/Frameworks/$name" 2>/dev/null
done
while IFS= read -r -d '' f; do
    rewrite_refs "$f"
done < <(macho_files)
EXE="$APP/Contents/MacOS/$EXE_NAME"
add_rpath() {
    otool -l "$2" | grep -A2 LC_RPATH | grep -q " path $1 " || install_name_tool -add_rpath "$1" "$2" 2>/dev/null
}
add_rpath "@executable_path/../Frameworks" "$EXE"
for name in "${FFMPEG_LIBS[@]}" libonnxruntime.dylib; do
    add_rpath "@loader_path" "$APP/Contents/Frameworks/$name"
done

# ---------------------------------------------------------- 6. sign ----
log "6/8 ad-hoc signing"
for f in "$APP/Contents/Frameworks/"*.dylib; do
    codesign --force --sign - --timestamp=none "$f"
done
codesign --force --deep --sign - --timestamp=none "$APP"

# ------------------------------------------------------- 7. archives ----
log "7/8 writing .zip and .dmg"
ZIP="$OUT/$ARTIFACT_BASE.zip"
DMG="$OUT/$ARTIFACT_BASE.dmg"
rm -f "$ZIP" "$DMG"
ditto -c -k --sequesterRsrc --keepParent "$APP" "$ZIP"
DMG_STAGE="$OUT/dmg-stage"
rm -rf "$DMG_STAGE"
mkdir -p "$DMG_STAGE"
ditto "$APP" "$DMG_STAGE/$APP_NAME.app"
ln -s /Applications "$DMG_STAGE/Applications"
hdiutil create -quiet -volname "$APP_NAME" -srcfolder "$DMG_STAGE" -ov -format UDZO "$DMG"
rm -rf "$DMG_STAGE"

# --------------------------------------------------------- 8. checks ----
log "8/8 bundle checks"
FAIL=0
fail() {
    printf '    FAIL: %s\n' "$*" >&2
    FAIL=1
}

info "-- Mach-O references (otool -L)"
while IFS= read -r -d '' f; do
    rel="${f#"$APP/"}"
    bad="$(otool -L "$f" | tail -n +2 | awk '{print $1}' |
        grep -Ev '^(/usr/lib/|/System/|@rpath/|@executable_path/|@loader_path/)' || true)"
    if [[ -n "$bad" ]]; then
        fail "$rel references non-system absolute paths: $(echo "$bad" | tr '\n' ' ')"
    else
        info "ok  $rel ($(otool -L "$f" | tail -n +2 | wc -l | tr -d ' ') refs)"
    fi
    # Every @rpath reference must resolve inside the bundle.
    for dep in $(otool -L "$f" | tail -n +2 | awk '{print $1}' | grep '^@rpath/' || true); do
        [[ -f "$APP/Contents/Frameworks/${dep#@rpath/}" ]] || fail "$rel: $dep is not bundled"
    done
done < <(macho_files)

info "-- ffmpeg configuration"
AVCODEC="$(ls "$APP/Contents/Frameworks/"libavcodec.*.dylib)"
CONF="$(strings -a "$AVCODEC" | grep -- '--prefix=' | head -1 || true)"
[[ -n "$CONF" ]] || fail "no configuration string in $(basename "$AVCODEC")"
if grep -q -- '--enable-gpl\|--enable-nonfree\|--enable-version3' <<<"$CONF"; then
    fail "bundled ffmpeg is configured with GPL/non-free/v3 parts"
fi
grep -q -- '--disable-encoders' <<<"$CONF" || fail "bundled ffmpeg is not decode-only"
LICENSE_STR="$(strings -a "$AVCODEC" | sed -n 's/^libavcodec license: //p' | head -1)"
[[ "$LICENSE_STR" == LGPL* ]] || fail "bundled ffmpeg licence is not LGPL: '$LICENSE_STR'"
info "license: $LICENSE_STR"
info "configuration: ${CONF/$FFMPEG_PREFIX/<prefix>}"

info "-- ONNX Runtime"
ORT_ID="$(otool -L "$APP/Contents/Frameworks/libonnxruntime.dylib" | sed -n 2p | sed 's/^[[:space:]]*//')"
info "$ORT_ID"
grep -q "current version $ORT_VERSION" <<<"$ORT_ID" || fail "bundled ONNX Runtime is not $ORT_VERSION"

info "-- codesign --verify --deep --strict"
if codesign --verify --deep --strict --verbose=2 "$APP" 2>&1 | sed 's/^/    /'; then
    :
else
    fail "codesign verification failed"
fi

info "-- --self-test (bundled sample, bundled ONNX Runtime)"
# A throwaway HOME and no ORT_DYLIB_PATH: the self-test must neither touch the
# user's data nor rely on anything outside the bundle. ORT_DISABLE_TELEMETRY is
# unset so the check proves the app itself turns ONNX Runtime's telemetry store
# off (it would otherwise appear under HOME/Library/Application Support).
SELFTEST_HOME="$(mktemp -d "$TARGET/selftest-home.XXXXXX")"
if env -u ORT_DYLIB_PATH -u ORT_DISABLE_TELEMETRY HOME="$SELFTEST_HOME" "$EXE" --self-test >"$OUT/self-test.log" 2>&1; then
    sed 's/^/    /' "$OUT/self-test.log"
    grep -q "loaded from $APP/Contents/Frameworks/libonnxruntime.dylib" "$OUT/self-test.log" ||
        fail "ONNX Runtime was not loaded from the bundle"
    grep -q "license=LGPL" "$OUT/self-test.log" || fail "self-test did not report an LGPL ffmpeg"
else
    sed 's/^/    /' "$OUT/self-test.log" >&2
    fail "--self-test exited non-zero"
fi
[[ -z "$(ls -A "$SELFTEST_HOME")" ]] || fail "--self-test wrote into HOME: $(ls -A "$SELFTEST_HOME")"
rm -rf "$SELFTEST_HOME"

[[ "$FAIL" == 0 ]] || die "bundle checks failed"

log "done"
info "app: $APP ($(du -sh "$APP" | awk '{print $1}'))"
info "zip: $ZIP ($(du -h "$ZIP" | awk '{print $1}'))"
info "dmg: $DMG ($(du -h "$DMG" | awk '{print $1}'))"
