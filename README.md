# Brilliant

A fast, private Brightspace companion for students who actually want to *use* their LMS.

Built originally for University of Southern Maine, but works with any D2L Brightspace instance. Brilliant turns the Brightspace web portal into a clean desktop app that puts grades, assignments, modules, and announcements one click away — and quietly keeps a personal record of your entire academic history.

---

## Why Brilliant

### Your courses, on every device you own
Pair your laptop, desktop, and phone with a QR code. Pinned courses, custom colors, target grades, completion ticks, manual due-date overrides, synthetic assignments — all of it stays in sync across your devices automatically. No central server, no account to sign up for. Your devices talk directly to each other over an encrypted peer-to-peer channel.

If you lose a device, rotate the shared secret in **Settings → Device Sync** and re-pair the others. The lost device falls off the network silently and never sees another byte of your data.

### Private by design
- **Your Brightspace cookies never leave your machine.** Magic Login captures them in a local browser window; nothing is sent anywhere except Brightspace itself.
- **Your data stays on your devices.** P2P sync uses iroh's QUIC transport with NAT hole-punching; traffic between your devices is end-to-end encrypted. The data never touches a third-party server.
- **Secrets live in the OS keychain** (macOS Keychain, Windows Credential Manager, Linux keyutils) — not in a config file someone can grep through.
- **Optional Web Access Passcode** if you run Brilliant in a browser tab.

### A permanent personal archive of your academic history
Universities run Brightspace as a rental. When you graduate, USM gives you a choice: pay an alumni fee to keep accessing your old courses, or manually export everything yourself before they cut you off. Most students just lose it all.

Brilliant snapshots everything as you go — every assignment, every grade, every module file, every instructor comment — into a local SQLite database you fully own. Years later, you still have it.

> One of my professors closed our course in Brightspace before all the grades were entered. By the time I noticed, the page was just gone. **My local Brilliant database still had the last known state of every grade**, including the ones that were never posted. Crisis averted.

If an instructor's API response goes "thin" mid-semester (missing descriptions, missing banners, missing rubrics), Brilliant refuses to overwrite the good data it already has. Your history is protected even when the LMS isn't.

---

## What it actually does

- **Unified dashboard** — every course's notifications, grades, and upcoming work in one place, newest first
- **Real grade math** — USM-weighted GPA, cumulative tracking, "max potential" projection, expected-score scenarios for ungraded items, and a confidence shield so you know when the number is solid
- **Hide grades you don't care about** (e.g. that one optional extra-credit weekly survey) so your average reflects reality
- **Course content browser** with collapsible modules, rubrics, instructor feedback, and direct download for syllabi or entire module ZIPs
- **Calendar export** to ICS for due dates
- **Manual overrides** for assignment names, descriptions, and due dates — and Brilliant tells you when it preserved your edits during a sync
- **Degraded mode** when your Brightspace session expires: keep reading everything you've cached, with a one-click re-login banner

---

## Get started

```bash
# Tauri desktop app (v2.0+)
nix develop .            # provides Node.js 22 for Vite/Rolldown builds
cd tauri/
npm install              # keep optional native dependencies enabled
npm run tauri dev
```

For a local macOS Transporter/Brilliant desktop artifact, run:

```bash
cd tauri/
npm run macos:build
```

That command builds a universal Apple Silicon + Intel Tauri bundle using the embedded `dist/` assets; it does not require or contact a Vite dev server at launch.

Pre-built signed installers for macOS (universal) and Windows are on the [Releases page](https://github.com/mtuckerb/brilliant_little_gnome/releases).

First launch:
1. Click **Launch Magic Login**.
2. Sign in to your school's Brightspace portal normally — SSO and MFA work fine.
3. The window closes. You're in.

To pair a second device: **Settings → Device Sync → Show pairing QR**, then scan it from the other device.

---

## Platforms

- **macOS** — universal binary (Apple Silicon + Intel), signed and notarized
- **Windows** — x64 MSI/NSIS installers, signed
- **Linux** — runs from source via Nix shell or system Rust+Node
- **iOS / iPadOS** — Tauri build (see [`tauri/docs/ios.md`](tauri/docs/ios.md))

---

## Contributing safely

Make each change from a dedicated writable Git worktree on an `agent/*` branch. Before editing, write down the task-specific boundaries and acceptance criteria; if they are missing, establish them first instead of inferring a product change.

Keep implementation and verification inside that worktree. Run the checks relevant to the files you changed, commit only the scoped files, push the branch, and open a pull request for independent review. Review must cover the acceptance criteria as well as security, performance, and regression risk before approval.

A code change or successful review does not authorize live external, account, or cloud operations. Those actions require explicit human or operations approval.

---

## Deeper reading

If you want to know how the sausage is made:

- **[`docs/system_design.md`](docs/system_design.md)** — overall architecture, sync engine, persistence model
- **[`tauri/docs/sync/design.md`](tauri/docs/sync/design.md)** — P2P sync architecture, threat model, CRDT mapping
- **[`tauri/README.md`](tauri/README.md)** — Tauri development notes, layout, feature flags
- **[`docs/openapi.yaml`](docs/openapi.yaml)** — authoritative REST and MCP contract for the embedded Tauri server; Swagger UI is served at `/docs`

---

## License & contact

Built by [@mtuckerb](https://github.com/mtuckerb). Issues and pull requests welcome.
