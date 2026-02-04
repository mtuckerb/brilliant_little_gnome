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

  if (!fs.existsSync(resourcesPath)) {
    console.warn(`  • afterPack       Resources directory not found at ${resourcesPath}`);
    return;
  }

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
      // We use --force to overwrite any existing signature
      // We use --options runtime for Hardened Runtime
      // We use --timestamp for notarization requirement
      // We apply entitlements to everything in the sidecar to be safe, 
      // though typically only the main executable strictly needs them.
      // But for nested ruby with native extensions, this is often the most reliable way.
      execSync(`codesign --force --options runtime --timestamp --entitlements "${entitlementsPath}" -s "${identity}" "${binary}"`);
      // console.log(`  • afterPack       signed ${relPath}`);
    } catch (err) {
      console.warn(`  • afterPack       warning: failed to sign ${relPath}: ${err.message}`);
    }
  }

  // Double check the ruby binary specifically
  const rubyPath = path.join(resourcesPath, 'bin', 'ruby_dist', 'macos-arm64', 'bin', 'ruby');
  if (fs.existsSync(rubyPath)) {
    console.log(`  • afterPack       ensuring ruby entitlements for ${path.relative(resourcesPath, rubyPath)}`);
    try {
      execSync(`codesign --force --options runtime --timestamp --entitlements "${entitlementsPath}" -s "${identity}" "${rubyPath}"`);
    } catch (err) {
      console.error(`  • afterPack       failed to sign ruby: ${err.message}`);
    }
  }

  console.log(`  • afterPack       signing complete`);
};
