## SwitchAI v1.2.0

### What's New:
* **Portable Auto-Updater for Windows:** built-in update checks and 1-click update installation with Minisign cryptographic verification, atomic replacement, and rollback protection.
* **Redesigned System Tray Menu:** structured sectioned menu (`── Codex ──` and `── Antigravity ──`), 1-click quick switch to recommended accounts, and clean bullet-aligned layout.
* **Persistent Privacy Mode:** privacy masking settings now persist cleanly across sessions and synchronize between the UI, tray dashboard, and notifications.
* **Modular Backend Architecture:** refactored command layer into dedicated modules with stricter concurrency gates and CAS state invariants.
* **Enhanced Secret Vault Resilience:** in-memory token caching with corrupted storage recovery and fallback protection.
* General performance optimizations, UI polish, and stability improvements.

---

### ℹ️ If Windows SmartScreen shows a warning:
As this is an open-source application without an expensive Microsoft enterprise signing certificate, Windows Defender SmartScreen may display a blue warning on first launch:
1. Click **"More info"**.
2. Click **"Run anyway"**.

🛡️ **VirusTotal Report:** [View Scan Analysis](https://www.virustotal.com/gui/file-analysis/ZDA0ZGJjYTRmZTVkNjU3ZGQ3ZTY0NTYyNTE2NTY2OTM6MTc4ODM2MTYwMg==)
