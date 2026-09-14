## SwitchAI v1.3.0

### What's New:
* **Independent Account Refresh:** Codex and Gemini now refresh in separate queues, with per-account activity indicators and progress.
* **Smarter Account Recommendations:** suggestions now take quota freshness and account health into account, excluding stale quotas and accounts with quota errors.
* **Clearer Quota Information:** quota age indicators, consistent event timestamps, and more accurate quota window labels.
* **More Reliable Session Sync:** improved synchronization with external Codex and Antigravity sessions, with retries that respect service cooldowns.
* **Safer Windows Updates:** downloaded updates must match the selected release before installation, with corrected rollback when an older executable backup is locked.
* **Improved Credential Recovery:** restore a missing token vault from its encrypted backup and preserve recovered credentials during later saves.
* **More Resilient Startup:** failed cleanup of legacy account files no longer blocks healthy accounts from loading; the app shows a warning and retries cleanup on the next load.
* Reduced redundant background requests, UI updates, and metadata writes, plus general stability improvements.

---

### Upgrading from v1.2.0 or earlier:
The update signing key has changed. Download and replace **SwitchAI.exe** manually for this release; older versions cannot verify the new update signature. Version 1.3.0 includes the new public key for future signed Windows updates. On macOS, install the new DMG as usual.

### ℹ️ If Windows SmartScreen shows a warning:
The Windows executable is not Authenticode-signed. Verify the download against **SHA256SUMS-win.txt** before running it. Minisign protects in-app updates and does not remove SmartScreen warnings.
