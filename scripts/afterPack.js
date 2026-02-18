const { execSync } = require('child_process');
const path = require('path');
const fs = require('fs');

function getAllFiles(dirPath, arrayOfFiles) {
  const files = fs.readdirSync(dirPath);

  arrayOfFiles = arrayOfFiles || [];

  files.forEach(function(file) {
    const fullPath = path.join(dirPath, file);
    if (fs.statSync(fullPath).isDirectory()) {
      arrayOfFiles = getAllFiles(fullPath, arrayOfFiles);
    } else {
      arrayOfFiles.push(fullPath);
    }
  });

  return arrayOfFiles;
}

exports.default = async function(context) {
  const { electronPlatformName, appOutDir, packager } = context;

  if (electronPlatformName !== 'darwin') {
    return;
  }

  const appName = packager.appInfo.productFilename;
  const appPath = path.join(appOutDir, `${appName}.app`);

  console.log(`  • afterPack       cleaning extended attributes from ${appPath}`);
  try {
    execSync(`xattr -cr "${appPath}"`);
  } catch (e) {
    console.warn(`  • afterPack       warning: failed to clean xattrs: ${e.message}`);
  }

  const resourcesPath = path.join(appPath, 'Contents', 'Resources');
  const entitlementsPath = path.join(context.packager.info.projectDir, 'build', 'entitlements.mac.plist');

  if (!fs.existsSync(resourcesPath)) {
    console.warn(`  • afterPack       Resources directory not found at ${resourcesPath}`);
    return;
  }

  // --- Phase 1: Fix dylib paths (install_name_tool) before signing ---
  console.log(`  • afterPack       fixing dylib paths in Ruby distribution...`);

  const archDirs = ['macos-arm64', 'macos-x64'];
  for (const archDir of archDirs) {
    const rubyDistBase = path.join(resourcesPath, 'bin', 'ruby_dist', archDir);
    const rubyBin = path.join(rubyDistBase, 'bin', 'ruby');
    const libDir = path.join(rubyDistBase, 'lib');

    if (!fs.existsSync(rubyBin)) continue;

    console.log(`  • afterPack       relinking Ruby binary for ${archDir}`);

    // Remove existing signature so install_name_tool can modify the binary
    try { execSync(`codesign --remove-signature "${rubyBin}"`); } catch (e) {}

    // Fix libruby reference: change absolute CI path to @rpath
    try {
      const otoolOutput = execSync(`otool -L "${rubyBin}"`).toString();
      const librubyMatch = otoolOutput.match(/\s+(\/\S*libruby\S*\.dylib)/);
      if (librubyMatch && !librubyMatch[1].startsWith('@')) {
        console.log(`  • afterPack       changing ${librubyMatch[1]} -> @rpath/libruby.3.4.dylib`);
        execSync(`install_name_tool -change "${librubyMatch[1]}" "@rpath/libruby.3.4.dylib" "${rubyBin}"`);
      }

      // Fix any other absolute Homebrew/CI paths (e.g., libgmp)
      const absLibMatches = otoolOutput.matchAll(/\s+(\/\S+\/(lib\S+\.dylib))/g);
      for (const match of absLibMatches) {
        const absPath = match[1];
        const libName = match[2];
        if (absPath.startsWith('@') || absPath.startsWith('/usr/lib') || absPath.startsWith('/System')) continue;
        // Check if we have this lib bundled
        if (fs.existsSync(path.join(libDir, libName))) {
          console.log(`  • afterPack       changing ${absPath} -> @rpath/${libName}`);
          execSync(`install_name_tool -change "${absPath}" "@rpath/${libName}" "${rubyBin}"`);
        }
      }
    } catch (e) {
      console.warn(`  • afterPack       warning: failed to fix dylib paths in ruby: ${e.message}`);
    }

    // Add rpaths so @rpath resolves to the bundled lib directory
    try { execSync(`install_name_tool -add_rpath "@executable_path/../lib" "${rubyBin}" 2>&1`); } catch (e) {}
    try { execSync(`install_name_tool -add_rpath "@loader_path/../../lib" "${rubyBin}" 2>&1`); } catch (e) {}

    // Fix bundled dylibs: set their IDs and fix internal cross-references
    if (fs.existsSync(libDir)) {
      const dylibs = fs.readdirSync(libDir).filter(f => f.endsWith('.dylib'));
      for (const dylib of dylibs) {
        const dylibPath = path.join(libDir, dylib);
        try {
          execSync(`codesign --remove-signature "${dylibPath}" 2>&1`);
          execSync(`install_name_tool -id "@rpath/${dylib}" "${dylibPath}" 2>&1`);

          // Fix internal references to absolute paths
          const dylibOtool = execSync(`otool -L "${dylibPath}"`).toString();
          const internalMatches = dylibOtool.matchAll(/\s+(\/\S+\/(lib\S+\.dylib))/g);
          for (const match of internalMatches) {
            const absPath = match[1];
            const depName = match[2];
            if (absPath.startsWith('@') || absPath.startsWith('/usr/lib') || absPath.startsWith('/System')) continue;
            execSync(`install_name_tool -change "${absPath}" "@rpath/${depName}" "${dylibPath}" 2>&1`);
          }
        } catch (e) {}
      }
    }

    // Fix native gem extensions (.bundle files)
    const vendorPath = path.join(resourcesPath, 'vendor', 'bundle');
    if (fs.existsSync(vendorPath)) {
      const bundleFiles = getAllFiles(vendorPath).filter(f => f.endsWith('.bundle'));
      for (const bundleFile of bundleFiles) {
        try {
          execSync(`codesign --remove-signature "${bundleFile}" 2>&1`);

          const bundleOtool = execSync(`otool -L "${bundleFile}"`).toString();

          // Fix libruby reference
          const librubyRef = bundleOtool.match(/\s+(\/\S*libruby\S*\.dylib)/);
          if (librubyRef && !librubyRef[1].startsWith('@')) {
            execSync(`install_name_tool -change "${librubyRef[1]}" "@rpath/libruby.3.4.dylib" "${bundleFile}"`);
          }

          // Fix other absolute lib references
          const absRefs = bundleOtool.matchAll(/\s+(\/\S+\/(lib\S+\.dylib))/g);
          for (const match of absRefs) {
            const absPath = match[1];
            const depName = match[2];
            if (absPath.startsWith('@') || absPath.startsWith('/usr/lib') || absPath.startsWith('/System')) continue;
            if (fs.existsSync(path.join(libDir, depName))) {
              execSync(`install_name_tool -change "${absPath}" "@rpath/${depName}" "${bundleFile}"`);
            }
          }

          // Add rpaths so extensions can find the bundled libs
          try { execSync(`install_name_tool -add_rpath "@executable_path/../lib" "${bundleFile}" 2>&1`); } catch (e) {}
          // Relative path from vendor/bundle/ruby/X.Y.Z/extensions/.../gem/ to bin/ruby_dist/arch/lib
          try { execSync(`install_name_tool -add_rpath "@loader_path/../../../../../../../bin/ruby_dist/${archDir}/lib" "${bundleFile}" 2>&1`); } catch (e) {}
          try { execSync(`install_name_tool -add_rpath "@loader_path/../../../../../../../../../bin/ruby_dist/${archDir}/lib" "${bundleFile}" 2>&1`); } catch (e) {}
        } catch (e) {}
      }
    }

    console.log(`  • afterPack       dylib relinking complete for ${archDir}`);
  }

  // --- Phase 2: Code signing ---
  // get identity from context or environment
  let identity = '-';
  try {
     const signingInfo = await packager.codeSigningInfo.value;
     if (signingInfo && signingInfo.name) {
       identity = signingInfo.name;
       console.log(`  • afterPack       found identity from signingInfo: "${identity}"`);
     }
  } catch (e) {
     identity = process.env.CSC_NAME || process.env.APPLE_DEVELOPER_IDENTITY || '-';
     console.log(`  • afterPack       using identity from environment: "${identity}"`);
  }

  if (identity === '-') {
    identity = process.env.APPLE_DEVELOPER_IDENTITY || '-';
  }

  console.log(`  • afterPack       signing ruby distribution and gems in ${appPath} with identity "${identity}"`);

  const allFiles = getAllFiles(resourcesPath);
  const binaries = [];

  for (const file of allFiles) {
    try {
      // Use 'file' to check if it's a Mach-O binary (executable, dylib, or bundle)
      const fileInfo = execSync(`file "${file}"`).toString();
      if (fileInfo.includes('Mach-O')) {
        binaries.push(file);
      }
    } catch (e) {
      // skip errors
    }
  }

  console.log(`  • afterPack       found ${binaries.length} Mach-O binaries in Resources to sign`);

  for (const binary of binaries) {
    const relPath = path.relative(resourcesPath, binary);

    try {
      execSync(`codesign --force --options runtime --timestamp --entitlements "${entitlementsPath}" -s "${identity}" "${binary}"`);
    } catch (err) {
      console.warn(`  • afterPack       warning: failed to sign ${relPath}: ${err.message}`);
    }
  }

  // Double check the ruby binary specifically
  for (const archDir of archDirs) {
    const rubyPath = path.join(resourcesPath, 'bin', 'ruby_dist', archDir, 'bin', 'ruby');
    if (fs.existsSync(rubyPath)) {
      console.log(`  • afterPack       ensuring ruby entitlements for ${path.relative(resourcesPath, rubyPath)}`);
      try {
        execSync(`codesign --force --options runtime --timestamp --entitlements "${entitlementsPath}" -s "${identity}" "${rubyPath}"`);
      } catch (err) {
        console.error(`  • afterPack       failed to sign ruby: ${err.message}`);
      }
    }
  }

  console.log(`  • afterPack       signing complete`);
};
