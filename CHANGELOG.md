# Changelog

[README](README.md) · **Changelog** · [Privacy](docs/PRIVACY.md) · [Troubleshooting](docs/TROUBLESHOOTING.md) · [Security](SECURITY.md) · [Русский](CHANGELOG.ru.md)

This file lists changes that are useful to SwitchAI users. Release dates are
added when a version is published.

## 1.0.0 — 2026-09-02

### Main features

- Manage several ChatGPT/Codex and Antigravity/Gemini accounts in one app.
- Switch the active account without copying authentication files by hand.
- View available quotas, subscription details, and account status.
- Refresh account information manually or on a schedule.
- See which healthy account currently has the most available quota.

### Privacy and safety

- Account tokens are stored in an encrypted local vault.
- The vault key is protected by Windows Credential Manager or macOS Keychain.
- Protected tokens are not sent to the app window.
- Account and settings files are saved with backups and recovery support.
- Releases include SHA-256 checksums and are platform-signed when signing
  certificates are configured.

### Platforms

- Windows 10 and 11 on x64 computers.
- macOS 11 or newer on Apple Silicon and Intel Macs.
