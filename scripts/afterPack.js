const { execSync } = require('child_process');
const path = require('path');
const fs = require('fs');

function getAllFiles(dirPath, arrayOfFiles) {
  const files = fs.readdirSync(dirPath);

  arrayOfFiles = arrayOfFiles || [];

  files.forEach(function(file) {
    if (fs.statSync(dirPath + "/" + file).isDirectory()) {
      arrayOfFiles = getAllFiles(dirPath + "/" + file, arrayOfFiles);
    } else {
      arrayOfFiles.push(path.join(dirPath, "/", file));
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
  const resourcesPath = path.join(appPath, 'Contents', 'Resources');
  const entitlementsPath = path.join(context.packager.info.projectDir, 'build', 'entitlements.mac.plist');

  // get identity from context if possible
  let identity = '-'; // default to ad-hoc
  try {
     const signingInfo = await packager.codeSigningInfo.value;
     if (signingInfo && signingInfo.name) {
       identity = signingInfo.name;
     }
  } catch (e) {
     identity = process.env.CSC_NAME || '-';
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
      const fileInfo = execSync(`file "${file}"`).toString();
      if (fileInfo.includes('Mach-O')) {
        binaries.push(file);
      }
    } catch (e) {
      // skip errors on file command
    }
  }
    
  console.log(`  • afterPack       found ${binaries.length} Mach-O binaries in Resources to sign`);

  for (const binary of binaries) {
    console.log(`  • afterPack       signing ${path.relative(resourcesPath, binary)}`);
    
    try {
      execSync(`codesign --force --options runtime --timestamp --entitlements "${entitlementsPath}" -s "${identity}" "${binary}"`);
    } catch (err) {
      console.warn(`  • afterPack       warning: failed to sign ${binary}: ${err.message}`);
    }
  }

  // Also specifically ensure the ruby binary has the correct entitlements if it's outside resources or missed
  const rubyPath = path.join(resourcesPath, 'bin', 'ruby_dist', 'macos-arm64', 'bin', 'ruby');
  if (fs.existsSync(rubyPath) && !binaries.includes(rubyPath)) {
    console.log(`  • afterPack       ensuring ruby entitlements for ${rubyPath}`);
    try {
      execSync(`codesign --force --options runtime --timestamp --entitlements "${entitlementsPath}" -s "${identity}" "${rubyPath}"`);
    } catch (err) {
      console.error(`  • afterPack       failed to sign ruby: ${err.message}`);
    }
  }

  console.log(`  • afterPack       signing complete`);
};
