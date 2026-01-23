#!/bin/bash
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

# 2. Bundle libruby and fix links
echo "Fixing links..."
mkdir -p "$LIB_DIR"
L_SRC=$(find "$PORTABLE_DIR" -name "libruby.3.4.dylib" | grep -v "$LIB_DIR" | head -n 1) || true
if [ -n "$L_SRC" ]; then
  cp "$L_SRC" "$LIB_DIR/"
fi

L_FILE="$LIB_DIR/libruby.3.4.dylib"
if [ -f "$L_FILE" ]; then
  chmod +w "$L_FILE"
  codesign --remove-signature "$L_FILE" || true
  codesign -f -s - "$L_FILE" || true
  install_name_tool -id "@rpath/libruby.3.4.dylib" "$L_FILE" || true
fi

codesign --remove-signature "$RUBY_BIN" || true
codesign -f -s - "$RUBY_BIN" || true
OLD_LIBRUBY=$(otool -L "$RUBY_BIN" | grep "libruby" | awk '{print $1}' | head -n 1)
if [ -n "$OLD_LIBRUBY" ]; then
  install_name_tool -change "$OLD_LIBRUBY" "@rpath/libruby.3.4.dylib" "$RUBY_BIN" || true
fi
install_name_tool -add_rpath "@executable_path/../lib" "$RUBY_BIN" || true

# 3. Build Gems
echo "Installing Gems..."
# Favor our portable ruby for everything
export PATH="$PORTABLE_DIR/bin:$PATH"
export BUNDLE_PATH="$(pwd)/vendor/bundle"
export GEM_HOME="$BUNDLE_PATH/ruby/3.4.0"

# NO RUBYLIB/GEM_PATH manual sets yet - let portable ruby find its stdlib
unset RUBYLIB GEM_PATH RUBYOPT

"$RUBY_BIN" -v
"$RUBY_BIN" -S gem install bundler -v 2.6.2 --no-document
"$RUBY_BIN" -S bundle config set --local path 'vendor/bundle'
"$RUBY_BIN" -S bundle config set --local deployment 'true'
"$RUBY_BIN" -S bundle install --jobs 4 --retry 3

# 4. Smoke Test
echo "Running smoke test..."
"$RUBY_BIN" -e "require 'sqlite3'; puts 'SUCCESS: sqlite3 works portable!'"

# 5. Production Signing
if [ -n "$1" ]; then
   IDENTITY="$1"
   ENTITLEMENTS="build/entitlements.mac.plist"
   echo "Performing production signing..."
   find bin/ruby_dist/macos-arm64 vendor/bundle -type f | while read -r item; do
     if file "$item" 2>/dev/null | grep -q "Mach-O"; then
       codesign --force --options runtime --timestamp -s "$IDENTITY" "$item" || true
     fi
   done
   codesign --force --options runtime --timestamp --entitlements "$ENTITLEMENTS" -s "$IDENTITY" "$RUBY_BIN"
fi
