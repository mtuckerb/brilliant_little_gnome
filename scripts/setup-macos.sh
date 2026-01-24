#!/bin/bash
# set -x # Enable if intense debugging needed

# setup-macos.sh: Prepares portable Ruby distribution for macOS
# Handles RPath relinking and Hardened Runtime signing requirements.

echo "Starting macOS Native Setup (Portability & Signing)..."

IDENTITY="$1"
PORTABLE_DIR="$(pwd)/bin/ruby_dist/macos-arm64"
RUBY_BIN="$PORTABLE_DIR/bin/ruby"
LIB_DIR="$PORTABLE_DIR/lib"
ENTITLEMENTS="$(pwd)/build/entitlements.mac.plist"

# 1. Unpack Portable Ruby
mkdir -p "$PORTABLE_DIR"
URL="https://github.com/ruby/ruby-builder/releases/download/ruby-3.4.1/ruby-3.4.1-darwin-arm64.tar.gz"
echo "Downloading Ruby 3.4.1 from $URL..."
if ! curl -L "$URL" | tar -xz -C "$PORTABLE_DIR" --strip-components=1; then
    echo "ERROR: Failed to download or unpack Ruby"
    exit 1
fi

chmod -R +w "$PORTABLE_DIR"
chmod +x "$RUBY_BIN"

# 2. Extract and relink libruby
echo "Bundling libruby..."
mkdir -p "$LIB_DIR"
L_SRC=$(find "$PORTABLE_DIR" -name "libruby.3.4.dylib" | grep -v "$LIB_DIR/libruby.3.4.dylib" | head -n 1)
if [ -n "$L_SRC" ]; then
  cp -f "$L_SRC" "$LIB_DIR/libruby.3.4.dylib"
else
  echo "WARNING: Could not find libruby.3.4.dylib"
fi

L_FILE="$LIB_DIR/libruby.3.4.dylib"
if [ -f "$L_FILE" ]; then
  echo "Relinking $L_FILE..."
  chmod +w "$L_FILE"
  codesign --remove-signature "$L_FILE" 2>/dev/null || true
  install_name_tool -id "@rpath/libruby.3.4.dylib" "$L_FILE" || true
fi

# 3. Relink Ruby Interpreter
echo "Relinking Ruby binary..."
chmod +w "$RUBY_BIN"
codesign --remove-signature "$RUBY_BIN" 2>/dev/null || true

# Change absolute links to libruby into @rpath links
otool -L "$RUBY_BIN" | grep "libruby" | awk '{print $1}' | while read -r old_path; do
  if [[ "$old_path" != "@rpath"* ]]; then
    echo "  Changing $old_path to @rpath/libruby.3.4.dylib"
    install_name_tool -change "$old_path" "@rpath/libruby.3.4.dylib" "$RUBY_BIN" 2>/dev/null || true
  fi
done

install_name_tool -add_rpath "@executable_path/../lib" "$RUBY_BIN" 2>/dev/null || true
install_name_tool -add_rpath "@loader_path/../../lib" "$RUBY_BIN" 2>/dev/null || true

# 4. Bundle Homebrew dependencies
echo "Bundling native system dependencies..."
for d in gmp libyaml openssl@3 sqlite; do
  BREW_PREFIX=$(brew --prefix $d 2>/dev/null || true)
  if [ -n "$BREW_PREFIX" ] && [ -d "$BREW_PREFIX/lib" ]; then
    echo "  Found $d at $BREW_PREFIX"
    # Only copy main dylibs to avoid bloat and circular symlinks
    cp -af "$BREW_PREFIX/lib"/*.dylib "$LIB_DIR/" 2>/dev/null || true
  fi
done

# Fix IDs and RPaths for bundled dylibs
echo "Fixing library IDs..."
find "$LIB_DIR" -maxdepth 1 -name "*.dylib" -type f | while read -r d; do
  chmod +w "$d"
  libname=$(basename "$d")
  codesign --remove-signature "$d" 2>/dev/null || true
  install_name_tool -id "@rpath/$libname" "$d" 2>/dev/null || true
done

# 5. Build Gems
echo "Installing gems..."
export BUNDLE_PATH="$(pwd)/vendor/bundle"
export GEM_HOME="$BUNDLE_PATH/ruby/3.4.0"
export INTERNAL_GEMS="$PORTABLE_DIR/lib/ruby/gems/3.4.0"
export GEM_PATH="$GEM_HOME:$INTERNAL_GEMS"
export PATH="$PORTABLE_DIR/bin:$PATH"
unset RUBYLIB RUBYOPT

"$RUBY_BIN" -v

echo "Installing Bundler..."
"$RUBY_BIN" -r rubygems "$PORTABLE_DIR/bin/gem" install bundler -v 2.6.2 --no-document || true

echo "Configuring gem build settings..."
for d in openssl@3 sqlite libyaml gmp; do
  PREFIX=$(brew --prefix $d 2>/dev/null || true)
  if [ -n "$PREFIX" ] && [ -d "$PREFIX" ]; then
    NAME=$(echo $d | sed 's/@3//')
    export PKG_CONFIG_PATH="$PREFIX/lib/pkgconfig:$PKG_CONFIG_PATH"
    if [ "$NAME" == "sqlite" ]; then
      "$RUBY_BIN" -r rubygems "$PORTABLE_DIR/bin/bundle" config build.sqlite3 --with-sqlite3-dir="$PREFIX" || true
    fi
    "$RUBY_BIN" -r rubygems "$PORTABLE_DIR/bin/bundle" config build.$NAME --with-$NAME-dir="$PREFIX" || true
  fi
done

"$RUBY_BIN" -r rubygems "$PORTABLE_DIR/bin/bundle" config set --local path 'vendor/bundle'
"$RUBY_BIN" -r rubygems "$PORTABLE_DIR/bin/bundle" config set --local deployment 'true'

echo "Running bundle install..."
export DYLD_LIBRARY_PATH="$LIB_DIR"
if ! "$RUBY_BIN" -r rubygems "$PORTABLE_DIR/bin/bundle" install --jobs 4; then
    echo "Bundle install failed. Attempting without deployment mode..."
    "$RUBY_BIN" -r rubygems "$PORTABLE_DIR/bin/bundle" config unset deployment
    "$RUBY_BIN" -r rubygems "$PORTABLE_DIR/bin/bundle" install --jobs 4
fi
unset DYLD_LIBRARY_PATH

# 6. Repair and Sign
echo "Relinking extensions..."
find "$BUNDLE_PATH" -name "*.bundle" -type f | while read -r bundle; do
  chmod +w "$bundle"
  codesign --remove-signature "$bundle" 2>/dev/null || true
  install_name_tool -add_rpath "@loader_path/../../../../../../../bin/ruby_dist/macos-arm64/lib" "$bundle" 2>/dev/null || true
  otool -L "$bundle" | grep "libruby" | awk '{print $1}' | while read -r old_path; do
    if [[ "$old_path" != "@rpath"* ]]; then
      install_name_tool -change "$old_path" "@rpath/libruby.3.4.dylib" "$bundle" 2>/dev/null || true
    fi
  done
done

echo "Smoke test..."
if ! "$RUBY_BIN" -e "require 'sqlite3'; puts 'SUCCESS'"; then
    echo "Smoke test failed - trying to fix sqlite3 specifically"
    # Potential fix: force @rpath for sqlite3 extension
    find "$BUNDLE_PATH" -name "sqlite3_native.bundle" -type f | while read -r s; do
       install_name_tool -change "$(brew --prefix sqlite)/lib/libsqlite3.0.dylib" "@rpath/libsqlite3.0.dylib" "$s" 2>/dev/null || true
    done
    "$RUBY_BIN" -e "require 'sqlite3'; puts 'SUCCESS AFTER FIX'" || echo "Still failing..."
fi

if [ -n "$IDENTITY" ]; then
   echo "Signing..."
   find "$PORTABLE_DIR" "$BUNDLE_PATH" -type f | while read -r item; do
     if file "$item" 2>/dev/null | grep -q "Mach-O"; then
       codesign --force --options runtime --timestamp -s "$IDENTITY" "$item" 2>/dev/null || true
     fi
   done
   codesign --force --options runtime --timestamp --entitlements "$ENTITLEMENTS" -s "$IDENTITY" "$RUBY_BIN" || true
fi

echo "macOS Native Setup Complete!"
