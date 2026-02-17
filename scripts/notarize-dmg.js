const { notarize } = require('@electron/notarize');
const path = require('path');

// afterAllArtifactBuild hook: notarizes the final DMG so Gatekeeper
// accepts it without needing to inspect the .app inside.
module.exports = async function notarizeDmg(context) {
  if (process.platform !== 'darwin') {
    return [];
  }

  const appleId = process.env.APPLE_ID;
  const appleIdPassword = process.env.APPLE_APP_SPECIFIC_PASSWORD;
  const teamId = process.env.APPLE_TEAM_ID;

  if (!appleId || !appleIdPassword || !teamId) {
    if (process.env.CI) {
      throw new Error('Notarization credentials missing for DMG notarization. Check GitHub Secrets.');
    }
    console.warn('  • dmg notarize    skipped (local build): credentials not found');
    return [];
  }

  const dmgFiles = context.artifactPaths.filter(p => p.endsWith('.dmg'));

  for (const dmg of dmgFiles) {
    console.log(`  • dmg notarize    notarizing ${path.basename(dmg)}...`);
    try {
      await notarize({
        tool: 'notarytool',
        appPath: dmg,
        appleId: appleId,
        appleIdPassword: appleIdPassword,
        teamId: teamId,
      });
      console.log(`  • dmg notarize    ${path.basename(dmg)} successful`);
    } catch (error) {
      console.error(`  • dmg notarize    ${path.basename(dmg)} failed: ${error.message}`);
      throw error;
    }
  }

  return [];
};
