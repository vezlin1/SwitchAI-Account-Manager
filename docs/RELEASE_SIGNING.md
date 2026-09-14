# Release signing

Windows portable updates use a Minisign key pair generated with the Tauri CLI.
The v1.3.0 key rotation requires a manual upgrade from v1.2.0 and earlier.

The maintainer's local copy is in the repository's ignored `.release-keys/` directory:

- `switchai.key`: encrypted private signing key.
- `switchai.key.pub`: public verification key.
- `password.txt`: generated key password.

The whole directory is excluded by the root `.gitignore`; its Windows ACL grants access to the current maintainer account. Keep a separate private backup of the key and password. They are intentionally absent from Git and will not appear in a fresh clone. Never attach them to a release or paste them into logs.

GitHub Actions repository secrets contain `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. The matching public key is embedded as `DEFAULT_UPDATE_PUBLIC_KEY` in `src-tauri/src/portable_updater.rs`. Future releases must keep this pair unless another explicit key rotation is intended.

The tagged release workflow signs `SwitchAI.exe` and embeds the signature in `latest.json`. Public releases contain only `SwitchAI.exe`, `SwitchAI_macos.dmg`, and `latest.json`; the separate `.sig` and checksum files remain build artifacts. GitHub displays the SHA-256 digest beside each release asset. Minisign is separate from Windows Authenticode and Apple signing/notarization.
