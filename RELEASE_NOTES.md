## 🚀 SwitchAI v1.4.0

**SwitchAI now tells you which account to use next — and why. Account tags, a calmer account list, quieter quota alerts and safer switching.**

### What's New:
* **Smarter Best-Account Pick:** recommendations weigh plan size (30% of Pro 200 is more than 90% of Plus), spend weekly limits that reset within a day first, treat a limit whose reset time has passed as full again, and never suggest a used-up account.
* **Account Tags:** group accounts under your own tags and filter the list by them.
* **Calmer Account List:** a small **Best** badge, status only when something needs you, and "Back in 39m" instead of "0% left".
* **Quieter Quota Alerts:** only about the account in use, with the best reserve suggested.
* **Safer Switching:** ChatGPT closes gracefully, and SwitchAI warns about Codex sessions still on the old account.
* **Manual Updates:** the built-in updater is removed; new versions come from GitHub Releases.

## 🎯 Best Account
- A small **Best** badge sits next to the account worth using next in the list, and the tray marks it too.
- The pick weighs plan size, so a bigger plan with a lower percentage can win.
- A weekly limit that resets within a day is spent first, so its quota is not lost.
- A limit whose reset time has passed counts as full again, shown as **~100% left** until the next check confirms it.
- A used-up account is never recommended.
- **Antigravity:** choose in Settings whether the pick follows Gemini models, Claude & GPT models, or whichever is tighter.
- The tray and the window use the same calculation, so they always agree.

## 🏷️ Account Tags
- Group accounts under your own tags; each tag becomes a filter above the list.
- Choose whether a tag's accounts also appear in **All**, and whether they refresh only while the tag is open.

## 🧹 Calmer Account List
- Status appears only when an account needs attention: **Signed out → Sign in again**, **Couldn't check limits → Check again**.
- A used-up limit says **Used up · Back in 39m**, and the account steps back until it resets.
- Drag any row to reorder; the Order and Actions columns are gone.
- Plain column names (**5-hour limit**, **Weekly limit**, **Plan**) and neutral plan badges.
- The toolbar says **Updated 6 min ago · next in 10m**.
- The privacy toggle is labelled **Hide emails**; the version number moved to Settings.

## 🔔 Quota Alerts
- No repeated alerts every time SwitchAI starts.
- Low-quota alerts only for the account in use, with the best reserve named in the alert.
- When the account in use runs dry, SwitchAI tells you as soon as another account has quota again.

## 🔁 Switching
- **Windows:** ChatGPT is asked to close first and is only force-closed if it does not, so drafts are not cut off.
- **Codex CLI / IDE extension:** if one is still running after a switch, SwitchAI warns that it keeps the previous account until restarted.
- The switch dialog says plainly what will happen: *ChatGPT closes and reopens with this account if it is open.*

## 🗂️ Plans, Limits and Tray
- Pro 100, Pro 200 and Pro 500 are told apart, and the plan is read for the right workspace.
- Refresh warnings appear on the account they belong to.
- Missing Gemini reset times are recovered from model data.
- Faster startup and quota refresh with many accounts; long lists load in pages.
- The tray menu is plain text with the system check mark — no more emoji.

## ⬆️ How to Update
1. Download the file for your system from **Assets** below.
2. **Windows:** replace your old `SwitchAI.exe` with the new one. **macOS:** open the `.dmg` and drag SwitchAI into **Applications**, replacing the old copy.
3. Start SwitchAI. Your accounts, tags and settings are kept.

> Version 1.4.0 removes the built-in updater, so 1.3.0 will not offer this update by itself. Download it once from this page; future versions are published here as well.

## 📦 Downloads

| System | File |
|---|---|
| Windows 10 / 11 (x64) | `SwitchAI.exe` |
| macOS 11 or newer (Apple Silicon and Intel) | `SwitchAI_macos.dmg` |

<details>
<summary><b>📋 Detailed changes</b></summary>

### Recommendation
- New `recommendation` module in the Rust core, shared by the tray, its tooltip and the window (`get_recommended_account` command). The UI's own copy of the logic is removed.
- Score = tightest remaining percent × plan weight × urgency. Plan weights are approximate: Free 0.25, Plus/Team/Business 1, Pro 100 ×5, Pro 200/500 ×20; Google AI Plus 0.5, Pro 1, Ultra 4.
- Urgency rises up to ×2 for a window of a day or longer that resets within 24 hours while still holding at least 10%.
- A window whose `reset_at` passed after its last check is projected as 100%. Accounts under 1% remaining are excluded.
- New setting `geminiQuotaFocus`: `gemini`, `claude_and_gpt` or `both`.

### Alerts
- The first alert level seen for an account is recorded silently, so restarts no longer replay alerts.
- Escalation alerts only for the active account; at the critical level the best reserve is appended.
- A "Quota available" alert fires when an inactive account recovers while the active one is critical.

### Switching
- Windows: plain `taskkill` first with a 2.5 s grace period, then `taskkill /F /T`.
- After a Codex switch, `codex` processes outside the ChatGPT app's process tree are counted and reported as `clientNotice`, both in the window and in the tray notification.

### Account list
- Rows are dragged by their whole surface (a custom pointer sensor ignores inputs and `[data-no-drag]`). The Order, Status and Actions columns are removed.
- Healthy accounts render no status; problems render under the email.
- Reset countdown in days and hours; a used-up window shows *Used up · Back in …* and dims its account in the table and the cards.
- One radius scale (6 / 10 / 16 px, Tailwind tokens aligned), neutral plan badges, no violet.

### Tray
- Account items use native check menu items for the account in use; labels carry no symbols.
- "Open SwitchAI" and "Refresh limits now" replace the previous wording.

### Earlier changes since 1.3.0
- The portable updater, its update dialog, the `latest.json` manifest generation and Minisign signing are removed.
- Account operations are consolidated, unused code is removed, and startup and quota refresh are faster for large account lists.
- Account tag groups, refresh warnings on the affected account, Pro tier detection, and Gemini reset times recovered from model metadata.
- rustls, rustls-webpki, quinn-proto, quinn-udp, cookie, url and webpki-root-certs updated to address RustSec advisories, including RUSTSEC-2026-0285.

</details>

---

### ℹ️ If Windows SmartScreen shows a warning:
The Windows executable is not Authenticode-signed. Verify the download against the SHA-256 digest displayed next to the file in GitHub Assets before running it.
