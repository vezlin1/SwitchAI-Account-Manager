<a id="readme-top"></a>

<p align="center">
  <img src="docs/switchai_hero.png" alt="SwitchAI Account Manager for ChatGPT/Codex and Antigravity" width="100%">
</p>

<h1 align="center">SwitchAI</h1>

<p align="center">
  Switch accounts. See your limits. Keep everything in one place.
</p>

<p align="center">
  <strong>English</strong> · <a href="README.ru.md">Русский</a><br>
  <a href="https://github.com/vezlin1/SwitchAI-Account-Manager/releases/latest"><strong>Download</strong></a>
  · <a href="#how-it-works">How it works</a>
  · <a href="docs/PRIVACY.md">Privacy</a>
  · <a href="docs/TROUBLESHOOTING.md">Help</a>
</p>

<p align="center">
  <img alt="Version 1.0.0" src="https://img.shields.io/badge/version-1.0.0-18a8e8?style=flat-square">
  <img alt="Windows 10 and 11" src="https://img.shields.io/badge/Windows-10%20%7C%2011-0078D4?style=flat-square&logo=windows">
  <img alt="macOS 11 or newer" src="https://img.shields.io/badge/macOS-11%2B-111111?style=flat-square&logo=apple">
  <img alt="MIT License" src="https://img.shields.io/badge/license-MIT-8b5cf6?style=flat-square">
  <img alt="No telemetry" src="https://img.shields.io/badge/telemetry-none-22c55e?style=flat-square">
</p>

SwitchAI is a desktop account manager for **ChatGPT/Codex** and
**Antigravity/Gemini**. Add your accounts once, check the limits available to
each one, and switch without copying authentication files by hand.

It is free, open source, and does not require a separate SwitchAI account.

## Why SwitchAI

| All accounts together | Limits at a glance | Safer switching |
| --- | --- | --- |
| Keep ChatGPT/Codex and Google accounts in one app, with a separate view for each provider. | See available quota, reset times, subscription details, and sign-in health. | Choose an account and let SwitchAI update the correct local authentication files. |

SwitchAI also helps you find a healthy account with the most useful remaining
quota, refreshes account information in the background, and stays available
from the system tray.

## Download

Download the latest version from
[GitHub Releases](https://github.com/vezlin1/SwitchAI-Account-Manager/releases/latest).

### Windows

1. Download `SwitchAI.exe`.
2. Open the file.
3. Add an account and complete sign-in in your browser.

Supported: **Windows 10 and 11, x64**.

### macOS

1. Download the `.dmg` file.
2. Open it and drag SwitchAI into **Applications**.
3. Start SwitchAI and add your first account.

Supported: **macOS 11 or newer**, Apple Silicon and Intel.

Release downloads include SHA-256 checksums and are signed when platform
certificates are configured.

## How it works

1. Open the **Codex** or **Antigravity** section.
2. Press **Add account** or **Sign in with Google**.
3. Complete the normal provider sign-in in your browser.
4. Select an account and press **Switch**.

SwitchAI updates the local sign-in used by the selected service. When needed,
it restarts Codex or the selected Antigravity app so the new account takes
effect immediately.

Already signed in to Antigravity? Use **Import current session** instead of
adding the same Google account again.

## What you can do

- Manage multiple ChatGPT/Codex and Antigravity/Gemini accounts.
- Switch active accounts without editing files manually.
- View the real quota windows returned by each provider.
- See plan details, reset times, and accounts that need a new sign-in.
- Refresh one account, all accounts, or let SwitchAI refresh them on a schedule.
- Search, reorder, and filter accounts by subscription.
- Choose which installed Antigravity apps should receive an account switch.
- Use the system tray for quick status and background refresh.
- Recover from a damaged state file using the built-in backup.

## Privacy and security

SwitchAI is designed to keep account management on your computer:

- there is no SwitchAI cloud account or telemetry service;
- account tokens are stored in an encrypted local vault;
- the vault key is protected by Windows Credential Manager or macOS Keychain;
- protected tokens are never sent to the app window;
- network requests go directly to OpenAI/ChatGPT, Google, or the optional
  service reachability check;
- state files are written with backup and recovery support.

Read the full [Privacy](docs/PRIVACY.md) and
[Security](SECURITY.md) documents before sharing logs or authentication files.

## Good to know

- SwitchAI is an independent open-source project. It is not made by or
  affiliated with OpenAI or Google.
- It does not bypass provider rules, regional restrictions, or subscription
  limits.
- Some quota or subscription fields may be missing when a provider does not
  return them for a particular account.
- Removing an account from SwitchAI does not necessarily revoke an existing
  session at the provider. Revoke access in the provider's security settings
  when needed.

## Help and project links

- [Troubleshooting](docs/TROUBLESHOOTING.md)
- [Privacy](docs/PRIVACY.md)
- [Security](SECURITY.md)
- [Changelog](CHANGELOG.md)
- [Report a problem](https://github.com/vezlin1/SwitchAI-Account-Manager/issues/new)

<details>
<summary><strong>Build from source</strong></summary>

### Requirements

- Node.js 24
- Rust stable
- Platform build tools for Tauri

### Run locally

```bash
git clone https://github.com/vezlin1/SwitchAI-Account-Manager.git
cd SwitchAI-Account-Manager
npm ci
npm --prefix ui ci
npm run dev
```

### Build a release

```powershell
# Windows
npm run build:release
```

```bash
# macOS universal build
npm run build:mac:universal
```

### Checks

```bash
npm --prefix ui run lint
npm --prefix ui run test:quota
cargo test --locked --manifest-path src-tauri/Cargo.toml
```

</details>

## License

SwitchAI is available under the [MIT License](LICENSE).
