#!/bin/bash
set -e
echo "Starting macOS Native Setup..."

# 1. Unpack Portable Ruby
mkdir -p bin/ruby_dist/macos-arm64
URL="https://github.com/ruby/ruby-builder/releases/download/ruby-3.4.1/ruby-3.4.1-darwin-arm64.tar.gz"
curl -L "$URL" | tar -xz -C bin/ruby_dist/macos-arm64 --strip-components=1

PORTABLE_DIR="$(pwd)/bin/ruby_dist/macos-arm64"
RUBY_BIN="$PORTABLE_DIR/bin/ruby"
LIB_DIR="$PORTABLE_DIR/lib"

chmod -R +w "$PORTABLE_DIR"
chmod +x "$RUBY_BIN"
xattr -cr "$PORTABLE_DIR" || true

# 2. Identify and Bundle Dependencies
echo "Bundling dependencies..."
mkdir -p "$LIB_DIR"

# Bundle libruby
L_SRC=$(find "$PORTABLE_DIR" -name "libruby.3.4.dylib" | grep -v "$LIB_DIR" | head -n 1) || true
if [ -n "$L_SRC" ]; then
  echo "Copying $L_SRC to $LIB_DIR"
  cp "$L_SRC" "$LIB_DIR/"
fi

L_FILE="$LIB_DIR/libruby.3.4.dylib"
if [ -f "$L_FILE" ]; then
  chmod +w "$L_FILE"
  codesign --remove-signature "$L_FILE" || true
  codesign -f -s - "$L_FILE" || true
  install_name_tool -id "@rpath/libruby.3.4.dylib" "$L_FILE"
  install_name_tool -add_rpath "@loader_path/" "$L_FILE" || true
fi

# Fix Ruby Interpreter Links
echo "Fixing Ruby interpreter links..."
chmod +w "$RUBY_BIN"
codesign --remove-signature "$RUBY_BIN" || true
codesign -f -s - "$RUBY_BIN" || true
# Relink libruby to rpath
OLD_LIBRUBY=$(otool -L "$RUBY_BIN" | grep "libruby" | awk '{print $1}' | head -n 1)
if [ -n "$OLD_LIBRUBY" ]; then
  install_name_tool -change "$OLD_LIBRUBY" "@rpath/libruby.3.4.dylib" "$RUBY_BIN" || true
fi
install_name_tool -add_rpath "@executable_path/../lib" "$RUBY_BIN" || true

# Bundle Homebrew dependencies
for d in gmp libyaml openssl@3; do
  P=$(brew --prefix $d 2>/dev/null)/lib || continue
  if [ -d "$P" ]; then
    for src in "$P"/*.dylib; do
      target="$LIB_DIR/$(basename "$src")"
      if [ "$src" != "$target" ]; then
        cp "$src" "$target" 2>/dev/null || true
      fi
    done
  fi
done

# Cleanup all bundled dylibs
find "$LIB_DIR" -name "*.dylib" -type f | while read -r d; do
  chmod +w "$d"
  if file "$d" | grep -q "Mach-O"; then
    codesign --remove-signature "$d" || true
    codesign -f -s - "$d" || true
    install_name_tool -id "@rpath/$(basename "$d")" "$d" || true
    install_name_tool -add_rpath "@loader_path/" "$d" || true
  fi
done

# 3. Build Gems
echo "Installing Gems..."
export BUNDLE_PATH="$(pwd)/vendor/bundle"
export GEM_HOME="$BUNDLE_PATH/ruby/3.4.0"
export INTERNAL_GEMS="$PORTABLE_DIR/lib/ruby/gems/3.4.0"
export GEM_PATH="$GEM_HOME:$INTERNAL_GEMS"
export PATH="$PORTABLE_DIR/bin:$PATH"
export DYLD_LIBRARY_PATH="$LIB_DIR"
unset RUBYOPT RUBYLIB

"$RUBY_BIN" -r rubygems "$PORTABLE_DIR/bin/gem" install bundler -v 2.6.2 --no-document
"$RUBY_BIN" -r rubygems "$PORTABLE_DIR/bin/bundle" config set --local path 'vendor/bundle'
"$RUBY_BIN" -r rubygems "$PORTABLE_DIR/bin/bundle" config set --local deployment 'true'
"$RUBY_BIN" -r rubygems "$PORTABLE_DIR/bin/bundle" install --jobs 4 --retry 3
unset DYLD_LIBRARY_PATH
xattr -cr vendor/bundle || true

# 4. Deep Portability Repair
echo "Repairing all Mach-O files..."
find "$PORTABLE_DIR" vendor/bundle -type f | while read -r f; do
  if file "$f" 2>/dev/null | grep -q "Mach-O"; then
    chmod +w "$f" 2>/dev/null || true
    codesign --remove-signature "$f" 2>/dev/null || true
    codesign -f -s - "$f" || true
    for depth in "" "../" "../../" "../../../" "../../../../" "../../../../../" "../../../../../../" "../../../../../../../"; do
       install_name_tool -add_rpath "@loader_path/${depth}lib" "$f" 2>/dev/null || true
    done
    otool -L "$f" 2>/dev/null | grep -E "/opt/homebrew|/usr/local|libruby" | awk '{print $1}' | while read -r dep; do
      base=$(basename "$dep")
      if [ -f "$LIB_DIR/$base" ] && [[ "$dep" != "@rpath"* ]]; then
        install_name_tool -change "$dep" "@rpath/$base" "$f" 2>/dev/null || true
      fi
    done
  fi
done

# 5. Smoke Test
echo "Running smoke test..."
"$RUBY_BIN" -v
"$RUBY_BIN" -e "require 'sqlite3'; puts 'SUCCESS: sqlite3 works portable!'"

# 6. Production Signing (if needed)
if [ -n "$1" ]; then
   IDENTITY="$1"
   ENTITLEMENTS="build/entitlements.mac.plist"
   echo "Performing production signing with $IDENTITY..."
   find bin/ruby_dist/macos-arm64 vendor/bundle -type f | while read -r item; do
     if file "$item" 2>/dev/null | grep -q "Mach-O"; then
       codesign --force --options runtime --timestamp -s "$IDENTITY" "$item" || true
     fi
   done
   echo "Signing interpreter..."
   codesign --force --options runtime --timestamp --entitlements "$ENTITLEMENTS" -s "$IDENTITY" "$RUBY_BIN"
fi
