const { notarize } = require('electron-notarize');
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
    console.warn('  • notarizing      skipped: APPLE_ID, APPLE_APP_SPECIFIC_PASSWORD, or APPLE_TEAM_ID not found');
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
