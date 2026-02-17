const { notarize } = require('@electron/notarize');
const path = require('path');

module.exports = async function notarizing(context) {
  const { electronPlatformName, appOutDir } = context;
  if (electronPlatformName !== 'darwin') {
    return;
  }

  const appName = context.packager.appInfo.productFilename;
  const appId = context.packager.appInfo.info.build.appId;

  console.log(`  • notarizing      appId=${appId} appName=${appName}`);

  const appleId = process.env.APPLE_ID;
  const appleIdPassword = process.env.APPLE_APP_SPECIFIC_PASSWORD;
  const teamId = process.env.APPLE_TEAM_ID;

  if (!appleId || !appleIdPassword || !teamId) {
    const missing = [
      !appleId && 'APPLE_ID',
      !appleIdPassword && 'APPLE_APP_SPECIFIC_PASSWORD',
      !teamId && 'APPLE_TEAM_ID',
    ].filter(Boolean).join(', ');

    // In CI, fail loudly so we never ship an un-notarized build
    if (process.env.CI) {
      throw new Error(`Notarization credentials missing: ${missing}. Check GitHub Secrets.`);
    }

    console.warn(`  • notarizing      skipped (local build): ${missing} not found`);
    return;
  }

  try {
    await notarize({
      tool: 'notarytool',
      appBundleId: appId,
      appPath: path.join(appOutDir, `${appName}.app`),
      appleId: appleId,
      appleIdPassword: appleIdPassword,
      teamId: teamId,
    });
  } catch (error) {
    console.error(`  • notarizing      failed: ${error.message}`);
    throw error;
  }

  console.log(`  • notarizing      successful`);
};
