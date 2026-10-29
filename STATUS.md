# Port status

Updated: 2026-10-30 (Asia/Bangkok)

Implementation is ongoing. The feature-parity work is paused while this snapshot is published.

## Working now

- Windows x64 Rust launcher with the reference login screen, US English locale, account/version/settings screens, and a portable packaging script.
- Offline, Microsoft, Ely.by OAuth, and Ely.by Legacy account flows.
- Minecraft version catalog and lifecycle, custom inherited versions, Java runtime management, and Forge/NeoForge/Fabric/LegacyFabric/Quilt launch paths.
- Game settings, server list and quick play, Minecraft `servers.dat` integration, promoted servers, crash suggestions, log browsing, and opt-in diagnostic upload.
- US personal notices and signed bootstrap configuration with US notices, promoted servers, and notification links.
- Ely.by OAuth client secret is supplied through `LEGACYLAUNCHER_ELY_CLIENT_SECRET`; no client secret is checked into this public repository.

## Still open

- Source update/new-feature dialogs and a Rust-native launcher updater/bootstrap handoff.
- Remaining scenes, settings, account variants, loader variants, and compatibility behaviors from the Java source.
- Full user-interface localization and visual checks across common Windows scaling settings.
- End-to-end runtime comparison against the Java launcher.

See [PORTING_PLAN.md](PORTING_PLAN.md) for the tracked checklist. No new parity percentage is claimed: the old estimate predates the recent work, and a reliable comparison needs the remaining runtime review.

## Windows build

From the project directory:

```powershell
.\Scripts\build-windows.ps1
.\Scripts\package-windows.ps1
```

The current portable archive is generated at `dist/LegacyLauncher-Rust-portable.zip`; build output and the archive are excluded from Git.
