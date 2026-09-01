# Troubleshooting

[README](../README.md) · [Changelog](../CHANGELOG.md) · [Privacy](PRIVACY.md) · **Troubleshooting** · [Security](../SECURITY.md) · [Русский](TROUBLESHOOTING.ru.md)

This page covers the most common SwitchAI problems. Start by installing the
latest release, checking your internet connection, and restarting SwitchAI.

## SwitchAI does not open

1. Close SwitchAI from the system tray and start it again.
2. Restart your computer if the app was just installed or updated.
3. Make sure you are using Windows 10/11 or macOS 11 or newer.
4. Download a fresh copy from the
   [Releases page](https://github.com/vezlin1/SwitchAI-Account-Manager/releases).

If the window opens but shows a render error, copy the error text or take a
screenshot before restarting the app.

## The browser does not open during sign-in

Check that a default browser is set and try **Add account** or **Re-login**
again. Complete sign-in in the browser that SwitchAI opens. The attempt ends
after 10 minutes, so start again if you see a timeout message.

If the provider's sign-in page does not load, check whether OpenAI/ChatGPT or
Google is available on your current network.

## An account says “Re-login required”

Select the account and press **Re-login**. A normal refresh cannot repair an
expired or revoked sign-in. If re-login still fails, remove the account from
SwitchAI, add it again, and complete the provider's sign-in process.

## Quotas or subscription details are missing

Press **Refresh** and wait for the check to finish. Some providers do not return
every limit or subscription detail for every account, so an empty field does
not always mean that something is broken.

If you see a connection or region warning:

1. Check that the provider works in your browser.
2. Check your internet connection and system date and time.
3. Try again later if the provider is temporarily unavailable.

SwitchAI does not bypass regional restrictions set by OpenAI or Google.

## The account changed in SwitchAI but not in Codex or Antigravity

SwitchAI normally restarts the selected app after switching. If a restart
warning appears, close Codex or Antigravity completely and open it again.

For Antigravity, also check the **Switch account in** options and make sure the
app you use is selected. If the message says that the Google token expiry is
unknown, refresh or re-login to that account before switching.

## SwitchAI cannot load its saved data

The recovery screen offers three actions:

1. Try **Restore backup** first.
2. Use **Open data directory** if you want to copy the existing files before
   making changes.
3. Use **Start fresh** only if the backup cannot be restored.

**Start fresh** clears the protected SwitchAI token vault and moves the old
state files into quarantine. It does not change the current Codex `auth.json`
file. You will need to add your accounts to SwitchAI again.

## The protected vault cannot be opened

The encrypted vault needs the key stored by Windows Credential Manager or
macOS Keychain. If that key was removed, for example after resetting the
operating system profile, the old vault cannot be opened with a new key.

Use **Start fresh**, then add the accounts again. Do not share the vault file or
authentication files when asking for help.

## Still need help?

Open a regular
[GitHub Issue](https://github.com/vezlin1/SwitchAI-Account-Manager/issues/new)
and include:

- your SwitchAI version and operating system;
- the exact error message;
- what you were doing before the problem appeared;
- simple steps that reproduce it.

Remove email addresses, account identifiers, tokens, and other personal data
from screenshots and logs. Report security problems privately as described in
[SECURITY.md](../SECURITY.md).
