# Privacy

[README](../README.md) · [Changelog](../CHANGELOG.md) · **Privacy** · [Troubleshooting](TROUBLESHOOTING.md) · [Security](../SECURITY.md) · [Русский](PRIVACY.ru.md)

Last updated: September 2, 2026

SwitchAI is a local desktop app. It does not require a SwitchAI account and
does not have its own cloud service.

## The short version

- Your accounts and settings stay on your computer.
- SwitchAI does not include advertising, analytics, or crash reporting.
- The app connects to OpenAI/ChatGPT and Google only to sign in, switch
  accounts, refresh access, and show account information.
- A small request may be sent to Cloudflare or ChatGPT to check whether the
  service is reachable from your region.

## What is stored on your computer

SwitchAI stores the information needed to manage your accounts, including:

- email address, provider, account identifier, and active account;
- subscription and quota information;
- app settings, refresh times, and recent error messages.

Access tokens are kept in an encrypted vault. The key for that vault is stored
using Windows Credential Manager or macOS Keychain. Tokens are handled by the
desktop part of the app and are not sent to the app window.

SwitchAI also keeps local state backups so it can recover from a damaged file.

## Internet connections

Network requests go directly from your computer to the service involved:

- OpenAI and ChatGPT for Codex sign-in, account access, and quota information;
- Google for Antigravity/Gemini sign-in, basic account information, token
  refresh, and quota information;
- Cloudflare or ChatGPT trace pages for an optional reachability check.

These services can receive the normal information included with an internet
request, such as your IP address. Their own privacy policies apply to data they
process. SwitchAI does not send this information to a separate SwitchAI server.

## Account switching

To switch accounts, SwitchAI reads and updates the local authentication files
used by Codex or Antigravity. This happens on your computer. The app does not
know or change your OpenAI or Google password.

## Your controls

- Removing an account deletes its saved data and protected tokens from
  SwitchAI.
- **Start fresh** clears the protected token vault and starts with empty app
  data. Previous state files are kept in quarantine for recovery and can be
  deleted manually if you no longer need them.

Removing an account from SwitchAI may not end an existing session at OpenAI or
Google. Use the security settings of the relevant provider if you also want to
revoke access there.

Uninstalling the app may leave local settings or recovery files behind. If you
want to remove everything, clear your accounts first, then remove
the SwitchAI data folder.

## Sharing logs or files

Treat exported account files, authentication files, vault files, backups, and
logs as private. Before sharing a diagnostic file or screenshot, remove email
addresses, account identifiers, tokens, and other personal information.

For security reports, follow the private process in
[SECURITY.md](../SECURITY.md).
