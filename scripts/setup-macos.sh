#!/bin/bash
set -e

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
curl -L "$URL" | tar -xz -C "$PORTABLE_DIR" --strip-components=1

chmod -R +w "$PORTABLE_DIR"
chmod +x "$RUBY_BIN"

# 2. Extract and relink libruby
echo "Bundling libruby..."
mkdir -p "$LIB_DIR"
# Find actual libruby in the unpacked dist (favors the one outside LIB_DIR if it just got unpacked)
L_SRC=$(find "$PORTABLE_DIR" -name "libruby.3.4.dylib" | grep -v "$LIB_DIR/libruby.3.4.dylib" | head -n 1) || true
if [ -n "$L_SRC" ]; then
  cp -f "$L_SRC" "$LIB_DIR/"
fi

L_FILE="$LIB_DIR/libruby.3.4.dylib"
if [ -f "$L_FILE" ]; then
  echo "Relinking $L_FILE..."
  chmod +w "$L_FILE"
  # Must remove signature before changing ID/RPath
  codesign --remove-signature "$L_FILE" || true
  install_name_tool -id "@rpath/libruby.3.4.dylib" "$L_FILE"
fi

# Helper for Deep Portability Repair
relink_dependencies() {
  local target="$1"
  # echo "    Analyzing dependencies for $(basename "$target")..."
  otool -L "$target" | grep -v "$(basename "$target")" | grep -Ei "(/usr/local|/opt/homebrew|/Users/|$(pwd))" | awk '{print $1}' | while read -r dep; do
    local dep_name=$(basename "$dep")
    # If we have the dependency bundled in our lib dir, point to it via @rpath
    if [ -f "$LIB_DIR/$dep_name" ]; then
      echo "    [Fix] Changing $dep to @rpath/$dep_name in $(basename "$target")"
      install_name_tool -change "$dep" "@rpath/$dep_name" "$target" || true
    fi
  done
}

# 3. Relink Ruby Interpreter
echo "Relinking Ruby binary..."
chmod +w "$RUBY_BIN"
codesign --remove-signature "$RUBY_BIN" || true

# Change absolute links to libruby into @rpath links
relink_dependencies "$RUBY_BIN"

otool -L "$RUBY_BIN" | grep "libruby" | awk '{print $1}' | while read -r old_path; do
  if [[ "$old_path" != "@rpath"* ]]; then
    echo "  Changing $old_path to @rpath/libruby.3.4.dylib"
    install_name_tool -change "$old_path" "@rpath/libruby.3.4.dylib" "$RUBY_BIN" || true
  fi
done

# Ensure Ruby can find libraries in the sidecar's lib folder relative to itself
# Packaging puts ruby at Contents/Resources/bin/ruby_dist/macos-arm64/bin/ruby
# and lib at Contents/Resources/bin/ruby_dist/macos-arm64/lib
install_name_tool -add_rpath "@executable_path/../lib" "$RUBY_BIN" || true
# For some gems, they might expect it in the root lib relative to their own location
install_name_tool -add_rpath "@loader_path/../../lib" "$RUBY_BIN" || true

# 4. Bundle Homebrew dependencies (common requirements for native gems)
echo "Bundling native system dependencies (OpenSSL, LibYAML, GMP, SQLite)..."
for d in gmp libyaml openssl@3 sqlite; do
  P=$(brew --prefix $d 2>/dev/null)/lib || continue
  if [ -d "$P" ]; then
    # Use -L to follow symlinks since many brew libs are symlinks
    find "$P" -maxdepth 1 -name "*.dylib" -not -type d | while read -r src; do
      target="$LIB_DIR/$(basename "$src")"
      if [ ! -f "$target" ]; then
        cp -a "$src" "$LIB_DIR/" 2>/dev/null || true
      fi
    done
  fi
done

# Fix IDs and RPaths for all bundled dylibs
find "$LIB_DIR" -maxdepth 1 -name "*.dylib" -type f | while read -r d; do
  chmod +w "$d"
  if [ -f "$d" ]; then
    echo "    Preparing $d..."
    codesign --remove-signature "$d" || true
    install_name_tool -id "@rpath/$(basename "$d")" "$d" || true
    install_name_tool -add_rpath "@loader_path/" "$d" 2>/dev/null || true
    relink_dependencies "$d"
  fi
done

# One more pass on libruby specifically since it's the core
relink_dependencies "$LIB_DIR/libruby.3.4.dylib"

# 5. Build Gems
echo "Installing gems into vendor/bundle..."
export BUNDLE_PATH="$(pwd)/vendor/bundle"
export GEM_HOME="$BUNDLE_PATH/ruby/3.4.0"
export INTERNAL_GEMS="$PORTABLE_DIR/lib/ruby/gems/3.4.0"
export GEM_PATH="$GEM_HOME:$INTERNAL_GEMS"
export PATH="$PORTABLE_DIR/bin:$PATH"

# Prevent Ruby from looking at global/system locations during build
unset RUBYLIB RUBYOPT

# We must use the portable ruby to install gems so they are compiled against its headers
# Use a consistent bundler version
"$RUBY_BIN" -r rubygems "$PORTABLE_DIR/bin/gem" install bundler -v 2.6.2 --no-document || "$RUBY_BIN" -r rubygems "$PORTABLE_DIR/bin/gem" install bundler --no-document

# Configure gem builds to find Homebrew dependencies
# This ensures native extensions compile correctly on the runner
for d in openssl@3 sqlite libyaml gmp; do
  PREFIX=$(brew --prefix $d 2>/dev/null)
  if [ -n "$PREFIX" ]; then
    NAME=$(echo $d | sed 's/@3//')
    echo "  Configuring build.$NAME to use $PREFIX"
    "$RUBY_BIN" -r rubygems "$PORTABLE_DIR/bin/bundle" config build.$NAME --with-$NAME-dir="$PREFIX" || true
  fi
done

"$RUBY_BIN" -r rubygems "$PORTABLE_DIR/bin/bundle" config set --local path 'vendor/bundle'
"$RUBY_BIN" -r rubygems "$PORTABLE_DIR/bin/bundle" config set --local deployment 'true'

# Use DYLD_LIBRARY_PATH so compilation can find our bundled libs if needed
export DYLD_LIBRARY_PATH="$LIB_DIR"
"$RUBY_BIN" -r rubygems "$PORTABLE_DIR/bin/bundle" install --jobs 4 --retry 3
unset DYLD_LIBRARY_PATH

# 6. Post-build Gem Repair (Relink .bundle files)
echo "Relinking native gem extensions..."
find "$BUNDLE_PATH" -name "*.bundle" -type f | while read -r bundle; do
  chmod +w "$bundle"
  codesign --remove-signature "$bundle" || true
  # Add rpaths so the extension can find libruby and bundled libs
  # Extensions are deep in vendor/bundle/ruby/3.4.0/extensions/...
  # We add many levels of fallback
  for depth in "../../../../../../../" "../../../../../../../../" "../../../../../../../../../"; do
     install_name_tool -add_rpath "@loader_path/${depth}bin/ruby_dist/macos-arm64/lib" "$bundle" 2>/dev/null || true
  done
  
  # Also change any absolute libruby links in gems
  otool -L "$bundle" | grep "libruby" | awk '{print $1}' | while read -r old_path; do
    if [[ "$old_path" != "@rpath"* ]]; then
      install_name_tool -change "$old_path" "@rpath/libruby.3.4.dylib" "$bundle" || true
    fi
    
    relink_dependencies "$bundle"
  done
done

# 7. Smoke Test
echo "Running smoke test..."
"$RUBY_BIN" -v
"$RUBY_BIN" -e "require 'sqlite3'; puts 'SUCCESS: sqlite3 works portable!'"

# 8. Production Signing
if [ -n "$IDENTITY" ]; then
   echo "Performing production signing with identity: $IDENTITY"
   # Sign all native objects
   find "$PORTABLE_DIR" "$BUNDLE_PATH" -type f | while read -r item; do
     if file "$item" 2>/dev/null | grep -q "Mach-O"; then
       # Don't use entitlements for libraries/extensions
       codesign --force --options runtime --timestamp -s "$IDENTITY" "$item"
     fi
   done
   # Sign the ruby interpreter LAST with entitlements
   echo "Signing Ruby interpreter with entitlements..."
   codesign --force --options runtime --timestamp --entitlements "$ENTITLEMENTS" -s "$IDENTITY" "$RUBY_BIN"
   
   # Verify signing
   codesign -vvv --display "$RUBY_BIN"
else
   echo "No signing identity provided. Skipping production signing."
fi

echo "macOS Native Setup Complete!"
