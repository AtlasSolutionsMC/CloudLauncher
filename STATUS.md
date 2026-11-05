# Port status

Updated: 2026-11-05 (Asia/Bangkok)

Implementation is ongoing; feature parity remains incomplete.

## Implemented so far

- Windows x64 Rust launcher with the reference login screen, US English locale, account/version/settings screens, and a portable packaging script.
- Login panel placement can be selected from all nine positions supported by the Java launcher and is imported from `legacy.properties`.
- Offline, Microsoft, Ely.by OAuth, and Ely.by Legacy account flows.
- Optional `--requireMinecraftAccount` account-add gate with the source US warning and Mojang-import exception.
- Minecraft version catalog and lifecycle, custom inherited versions, Java runtime management, Forge/NeoForge/Fabric/LegacyFabric/Quilt launch paths, and source Forge mod-pack argument compatibility.
- Game settings, server list and quick play, Minecraft `servers.dat` integration, promoted servers, crash suggestions, log browsing, and opt-in diagnostic upload.
- Source crash-analysis and full-command logging preferences, with legacy settings migration.
- Legacy `gui.font` migration and live launcher-wide text/UI scaling from 12 to 18 pt.
- Legacy `gui.background` image migration and live image selection/reset; video backgrounds are still open.
- Legacy log-window dimensions and position migration, with current viewer geometry saved across launches.
- Source `gui.logger` Global/None mode with automatic live game output and close-to-disable behavior.
- Migration and persistence for the selected Minecraft version and maximized launcher window state.
- Source version update alerts for enabled release types, backed by the compatible `versions/versions.json` cache and imported `gui.alerton.*` settings.
- Settings and common dialog labels now resolve from the US locale resource.
- US personal notices and signed bootstrap configuration with US notices, promoted servers, and notification links.
- Manual GitHub release checks with release notes, bounded executable download, SHA-256 verification, and a Windows replacement/restart handoff; a tag-triggered workflow publishes the assets.
- Source-style US first-run notice with a required acknowledgment before launcher controls can be used.
- Windows ARM first-run OpenGL compatibility notice with the source's five-second install countdown and support action.
- First-run game-folder conflict detection and destination selection with browse, path validation, and no-copy warning; settings also has a native folder picker.
- Ely.by OAuth client secret is supplied through `LEGACYLAUNCHER_ELY_CLIENT_SECRET`; no client secret is checked into this public repository.

## Still open

- Validate the executable download and Windows updater handoff against a published release artifact (the repository currently has no releases).
- The Java source's new-feature dialog has no call site in this checkout.
- Remaining scenes, settings, account variants, loader variants, and compatibility behaviors from the Java source.
- Full user-interface localization and visual checks across common Windows scaling settings.
- End-to-end runtime comparison against the Java launcher.

See [PORTING_PLAN.md](PORTING_PLAN.md) for the tracked checklist. No new parity percentage is claimed: the old estimate predates the recent work, and a reliable comparison needs the remaining runtime review.

## Windows build

Latest Windows x64 release build and portable ZIP packaging passed on 2026-11-05.

From the project directory:

```powershell
.\Scripts\build-windows.ps1
.\Scripts\package-windows.ps1
```

Packaging generates `dist/LegacyLauncher-Rust-portable.zip`; build output and the archive are excluded from Git.
