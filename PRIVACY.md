# Brilliant Privacy Policy

_Last updated: May 19, 2026_

## Summary

Brilliant is a local-first Brightspace companion. Your data stays on your devices. We don't operate a server. We don't collect analytics. We don't sell, share, or transmit your data to anyone.

## What Brilliant does with your data

**Your Brightspace credentials.** You enter your Brightspace login (institution URL, username, and password — or an SSO token) directly into Brilliant. These credentials are stored on your device only, in the macOS Keychain. Brilliant uses them to fetch your courses, grades, assignments, modules, announcements, and discussions from your school's Brightspace server. Credentials never leave your device except to authenticate with the Brightspace server you specified.

**Your coursework data.** Course content, grades, assignments, discussion posts, file attachments, and related materials are downloaded from Brightspace and stored locally on your device in a sandboxed SQLite database. This data is yours. It is never sent to Brilliant or any third party.

**Peer-to-peer device sync (optional).** If you choose to pair two or more of your own devices, Brilliant uses iroh — a peer-to-peer networking library built on QUIC and Ed25519 — to sync your courses and grade history directly between your paired devices over an end-to-end encrypted connection. Sync traffic does not pass through any server operated by Brilliant. Pairing is opt-in; you can run Brilliant on a single device and never enable it.

**Tasks you create in Brilliant.** Personal tasks, notes, and reminders you add inside the app are stored locally alongside your course data. They are never transmitted off-device except through the optional peer-to-peer sync described above.

## What Brilliant does NOT do

- Brilliant does not operate a backend server. There is no "Brilliant cloud."
- Brilliant does not collect analytics, telemetry, crash reports, or usage data.
- Brilliant does not embed third-party SDKs — no Google Analytics, no Firebase, no advertising identifiers, no tracking pixels.
- Brilliant does not share, sell, rent, or transfer your data to anyone.
- Brilliant does not contact any servers other than (a) the Brightspace server at the URL you provide and (b) other devices you have explicitly paired.

## Third-party services you interact with through Brilliant

When Brilliant communicates with your school's Brightspace server, that communication is subject to your school's and D2L Brightspace's own privacy policies. Brilliant is not affiliated with D2L. The privacy practices of your Brightspace instance are outside Brilliant's control.

## Children

Brilliant is intended for users 13 and over. We do not knowingly collect data from children under 13. The app does not contain advertising and does not transmit any user data off-device.

## Security

Credentials are stored in the macOS Keychain. Local databases are stored inside the app's sandboxed container. Peer-to-peer sync connections are authenticated with Ed25519 keys generated on your devices and never shared with us.

## Contact

Questions, concerns, or vulnerability reports:
- Email: tucker@tuckerbradford.com
- Issues: https://github.com/mtuckerb/brilliant_little_gnome/issues

## Changes to this policy

Material changes to this policy will be published to this same file. The "Last updated" date at the top of this document will reflect the most recent change.
