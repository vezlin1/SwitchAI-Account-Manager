# Changelog

[README](README.md) · **Changelog** · [Privacy](docs/PRIVACY.md) · [Troubleshooting](docs/TROUBLESHOOTING.md) · [Security](SECURITY.md) · [Русский](CHANGELOG.ru.md)

This file lists changes that are useful to SwitchAI users. Release dates are
added when a version is published.

## 1.3.0 — 2026-09-14

**Upgrade note:** the update signing key has changed. Users of v1.2.0 or earlier must download v1.3.0 manually. Future Windows updates use the new key embedded in v1.3.0.

### What's New:
* **Independent Account Refresh:** Codex and Gemini now refresh in separate queues, with per-account activity indicators and progress.
* **Smarter Account Recommendations:** suggestions now take quota freshness and account health into account, excluding stale quotas and accounts with quota errors.
* **Clearer Quota Information:** quota age indicators, consistent event timestamps, and more accurate quota window labels.
* **More Reliable Session Sync:** improved synchronization with external Codex and Antigravity sessions, with retries that respect service cooldowns.
* **Safer Windows Updates:** downloaded updates must match the selected release before installation, with corrected rollback when an older executable backup is locked.
* **Improved Credential Recovery:** restore a missing token vault from its encrypted backup and preserve recovered credentials during later saves.
* **More Resilient Startup:** failed cleanup of legacy account files no longer blocks healthy accounts from loading; the app shows a warning and retries cleanup on the next load.
* Reduced redundant background requests, UI updates, and metadata writes, plus general stability improvements.

## 1.2.0 — 2026-09-04

### What's New:
* **Portable Auto-Updater for Windows:** built-in update checks and 1-click update installation with Minisign cryptographic verification, atomic replacement, and rollback protection.
* **Redesigned System Tray Menu:** structured sectioned menu (`── Codex ──` and `── Antigravity ──`), 1-click quick switch to recommended accounts, and clean bullet-aligned layout.
* **Persistent Privacy Mode:** privacy masking settings now persist cleanly across sessions and synchronize between the UI, tray dashboard, and notifications.
* **Modular Backend Architecture:** refactored command layer into dedicated modules with stricter concurrency gates and CAS state invariants.
* **Enhanced Secret Vault Resilience:** in-memory token caching with corrupted storage recovery and fallback protection.
* General performance optimizations, UI polish, and stability improvements.

## 1.1.0 — 2026-09-02

### What's New:
* **Session Import:** 1-click import to quickly pull active sessions from ChatGPT (Codex) or Google (Antigravity).
* Revamped Hidden accounts tab.
* Enhanced context menus with custom text field actions (Cut, Copy, Paste, Clear) and email/ID copying.
* Improved real-time quota reset countdown timers.
* Improved Antigravity process management (status is preserved when switching tabs).
* Scoped search and subscription filters per provider (ChatGPT vs Gemini).
* Protected drag-and-drop reordering during active search.
* Added `Esc` key and backdrop click dismissal for modals.
* Added titlebar double-click to maximize and restore the window.
* Added 1-click technical error message copying.
* General UI polish and stability improvements.

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
