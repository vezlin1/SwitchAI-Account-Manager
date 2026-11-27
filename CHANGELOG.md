# Changelog

[README](README.md) · **Changelog** · [Privacy](docs/PRIVACY.md) · [Troubleshooting](docs/TROUBLESHOOTING.md) · [Security](SECURITY.md) · [Русский](CHANGELOG.ru.md)

This file lists changes that are useful to SwitchAI users. Release dates are
added when a version is published.

## 1.4.0 — 2026-11-28

### What's New:
* **Smarter Best-Account Pick:** recommendations weigh plan size (30% of Pro 200 is more than 90% of Plus), spend weekly limits that reset within a day first, treat a limit whose reset time has passed as full again, and never suggest a used-up account. The tray and the window always agree.
* **Choose What Counts in Antigravity:** recommend by Gemini models, Claude & GPT models, or whichever is tighter.
* **Account Tags:** group accounts under your own tags, filter the list by them, and decide whether a tag's accounts also show in All and refresh only inside the tag.
* **Calmer Account List:** a small Best badge marks the account to use next, status appears only when an account needs attention, used-up limits say when they are back ("Back in 39m") while the account steps back, and any row can be dragged to reorder.
* **Quieter Quota Alerts:** no repeated alerts on every start, low-quota alerts only for the account in use with the best reserve suggested, and a note when another account has quota again.
* **Safer Switching:** ChatGPT on Windows is asked to close before it is force-closed, and SwitchAI warns when Codex CLI or an IDE extension keeps using the previous account until restarted.
* **More Accurate Plans and Limits:** Pro 100, Pro 200 and Pro 500 are told apart, the plan is read for the right workspace, refresh warnings appear on the affected account, and missing Gemini reset times are recovered.
* **Faster With Many Accounts:** quicker startup and quota refresh, with long lists shown in pages.
* **Clearer Status and Tray:** "Updated 6 min ago · next in 10m" instead of a bare interval, a labelled privacy toggle, and a tray menu in plain text with the system check mark instead of emoji.
* **Manual Updates:** the built-in updater is removed; download new versions from GitHub Releases.
* Updated rustls to address RUSTSEC-2026-0285, plus general polish and stability improvements.

## 1.3.0 — 2026-09-14

### What's New:
* **Independent Account Refresh:** Codex and Gemini now refresh in separate queues, with per-account activity indicators and progress.
* **Smarter Account Recommendations:** suggestions now take quota freshness and account health into account, excluding stale quotas and accounts with quota errors.
* **Clearer Quota Information:** quota age indicators, consistent event timestamps, and more accurate quota window labels.
* **More Reliable Session Sync:** improved synchronization with external Codex and Antigravity sessions, with retries that respect service cooldowns.
* **Improved Credential Recovery:** restore a missing token vault from its encrypted backup and preserve recovered credentials during later saves.
* **More Resilient Startup:** failed cleanup of legacy account files no longer blocks healthy accounts from loading; the app shows a warning and retries cleanup on the next load.
* Reduced redundant background requests, UI updates, and metadata writes, plus general stability improvements.

## 1.2.0 — 2026-09-04

### What's New:
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
