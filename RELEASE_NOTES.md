## SwitchAI v1.3.0 — Draft

### What's New:
* **Independent Account Refresh:** Codex and Gemini now refresh in separate queues, with per-account activity indicators and progress.
* **Smarter Account Recommendations:** suggestions now take quota freshness and account health into account, excluding stale quotas and accounts with quota errors.
* **Clearer Quota Information:** quota age indicators, consistent event timestamps, and more accurate quota window labels.
* **More Reliable Session Sync:** improved synchronization with external Codex and Antigravity sessions, with retries that respect service cooldowns.
* **Safer Windows Updates:** fixed rollback when an older executable backup is locked, so recovery restores the actual previous version.
* **Improved Credential Recovery:** protected credentials take priority over legacy plaintext backups, and migrated token files are cleaned up after successful migration.
* Reduced redundant background requests, UI updates, and metadata writes, plus general stability improvements.

---

Draft for discussion; v1.3.0 has not been published. Release validation and the final Windows signing/SmartScreen information are tracked in [the preparation checklist](docs/RELEASE_PREPARATION_1.3.0.ru.md). Add a VirusTotal report for the actual release executable after the final build; the v1.2.0 report does not apply to v1.3.0.
