# LegacyLauncher Rust

A Rust desktop port based on the local LegacyLauncher Java source.

## Build on Windows

Install the Rust GNU toolchain and MSYS2 UCRT64, then run:

```powershell
.\Scripts\build-windows.ps1
```

The executable is created at `target\x86_64-pc-windows-gnu\release\legacylauncher-rust.exe`.

Create a portable folder and ZIP after building:

    .\Scripts\package-windows.ps1

The package is written to dist\LegacyLauncher-Rust-portable.zip. The launcher's assets
are embedded and the Windows GNU release build has no bundled third-party DLL requirement.

## Publish a Windows release

Push a semantic version tag such as `v0.1.1`. The Windows release workflow sets the package version from the tag, builds the x64 executable and portable ZIP, generates SHA-256 checksums, and attaches all three files to the GitHub release. The launcher’s **Check for updates** action reads published releases from this repository and displays newer release notes.

## Current features

- Recreated login screen with the original background and icon assets.
- US English strings, offline account management, auto-login, and saved preferences.
- Live Minecraft version catalog with search and release/snapshot/old-version filters.
- Install, verify/repair, select, and remove vanilla versions from the version manager.
- Install/repair through Forge, NeoForge, Fabric, LegacyFabric, or Quilt.
- Configure shared, per-family, or per-version game folders, memory, Java runtime mode, integrated/discrete/system GPU preference, window size, fullscreen, and launch arguments.
- Keep each selected loader/build in its own game-data directory so switching between vanilla and modded profiles does not reuse stale loader state.
- Add Microsoft accounts through the browser, refresh sign-in when launching, and keep refresh tokens DPAPI-protected.
- Sign in to Ely.by OAuth or Ely.by Legacy accounts; OAuth client secrets are read from the environment instead of being stored in this repository.
- Create a Minecraft profile during Microsoft sign-in when the account does not have one yet.
- Import offline and Microsoft accounts from the Java launcher profile file; imported Microsoft refresh tokens are protected with DPAPI.
- Discover local versions and download, verify, and launch with offline or Microsoft accounts.
- Validate custom/system Java against the selected version's Java metadata before launch.
- Import supported game, Java, display, GPU, folder, and launch settings from the Java launcher's `legacy.properties` file.
- Show US personal and signed bootstrap notices, notification links, and promoted servers.
- Browse recent game logs and crash reports, preview or copy the last 64 KiB, open their folders/files, and opt in to uploading a filtered diagnostic report.
- Cancel sign-in, installation, and game launch; inspect crash output against the bundled US English signature catalog and use its safe folder, settings, Java, force-update, or web-link actions.

## Ely.by OAuth configuration

Set `LEGACYLAUNCHER_ELY_CLIENT_SECRET` in the environment before launching the app to enable Ely.by OAuth sign-in. Use a client secret you are authorized to use; no secret is included in this repository.

## Remaining parity work

The launcher can download a newer Windows executable, check its SHA-256, and hand replacement/restart to a Windows helper. This path still needs validation against a published release artifact. Remaining source scenes and settings variants, loader-specific variants, full localization and visual-scale review, and end-to-end comparison against the Java source remain in progress. See `PORTING_PLAN.md` and `STATUS.md` for the current checklist and status.
