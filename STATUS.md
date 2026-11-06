# Port status

Updated: 2026-11-06 (Asia/Bangkok)

Implementation is ongoing; feature parity remains incomplete.

## Implemented so far

- Windows x64 Rust launcher with the reference login screen, US English locale, account/version/settings screens, and a portable packaging script.
- Login panel placement can be selected from all nine positions supported by the Java launcher and is imported from `legacy.properties`.
- Offline, Microsoft, Ely.by OAuth, and Ely.by Legacy account flows.
- Account type guidance with Microsoft/offline/Ely.by descriptions, plus source-style Buy Minecraft and Ely.by registration links.
- Login account dropdown opens Manage accounts directly and labels Ely.by Legacy accounts distinctly from Ely.by OAuth.
- Optional `--requireMinecraftAccount` account-add gate with the source US warning and Mojang-import exception.
- Minecraft version catalog and lifecycle, custom inherited versions, Java runtime management, Forge/NeoForge/Fabric/LegacyFabric/Quilt launch paths, and source Forge mod-pack argument compatibility.
- Game settings, server list and quick play, Minecraft `servers.dat` integration, promoted servers, crash suggestions, log browsing, and opt-in diagnostic upload with Windows/CPU/memory/GPU details.
- Source crash-analysis and full-command logging preferences, with legacy settings migration.
- Source-style startup version/platform, working-directory, and settings-file entries in the launcher log.
- Legacy `gui.font` migration and live launcher-wide text/UI scaling from 12 to 18 pt.
- Legacy `gui.font.old` migration with the source revert/dismiss prompt; restoring the earlier size applies live in Rust.
- Legacy `gui.laf.v1.state` and pre-FlatLaf `gui.systemlookandfeel` migration with live Off/System/Auto/Dark/Light UI theme selection.
- Legacy `gui.background` image migration and live image selection/reset; video backgrounds are still open.
- Legacy log-window dimensions and position migration, with current viewer geometry saved across launches.
- Source `gui.logger` Global/None mode with a 1 MiB live launcher buffer, a rolling 12 MiB launcher log with ten backups, live sanitized logs, log-file browsing, a stop-game action, and close-to-disable behavior. In-memory records now retain timestamp, severity, and subsystem; launch/download, auth, Java, mods, servers, catalog, update, bootstrap, notice, diagnostics, settings import, and crash events have explicit categories. Status updates now log directly with a level and subsystem; a full Java Log4j coverage comparison remains open.
- Migration and persistence for the selected Minecraft version and maximized launcher window state.
- Source version update alerts for enabled release types, backed by the compatible `versions/versions.json` cache and imported `gui.alerton.*` settings.
- Settings and common dialog labels now resolve from the US locale resource.
- The source SettingsPanel About us tab includes the US copy, Rust clone version, creator and Ely.by links, and source license text.
- Startup connectivity checks cover Mojang version metadata, Legacy Launcher repository mirrors, both bootstrap hosts, and Ely.by; failures show a warning badge with affected hosts and source help copy.
- Version filters, notice/account controls, launch/download status labels, and error text for Microsoft/Ely.by authentication, catalog, bootstrap, notices, diagnostics, updater, Java/server handling, imports, background images, and crash fallbacks now resolve from US locale entries. Dynamic errors and source-provided notice/crash copy still need a full-flow review.
- US personal notices and signed bootstrap configuration with US notices, promoted servers, and notification links; Java's separate full-screen notice scene now has previous/next navigation, timed hide/restore, notice actions, and a home action.
- Startup and manual GitHub release checks with release notes, bounded executable download, SHA-256 verification, and a Windows replacement/restart handoff; the startup prompt waits until first-run setup is complete. A tag-triggered workflow publishes the assets.
- The migrated `bootstrap.switchToBeta` preference enables opt-in GitHub prerelease updates; the default channel continues to use stable releases.
- Source-style US first-run notice with a required acknowledgment before launcher controls can be used.
- Windows ARM first-run OpenGL compatibility notice with the source's five-second install countdown and support action.
- First-run game-folder conflict detection and destination selection with browse, path validation, and no-copy warning; settings also has a native folder picker.
- Ely.by OAuth client secret is supplied through `LEGACYLAUNCHER_ELY_CLIENT_SECRET`; no client secret is checked into this public repository.
- Legacy settings import preserves the source client UUID and all six configured Java experiments; JVM experiments and experimental LWJGL/JNA library replacements are applied during launch.

## Still open

- Validate the executable download and Windows updater handoff against a published release artifact (the repository currently has no releases).
- The Java source's new-feature dialog has no call site in this checkout.
- Remaining scenes, settings, account variants, loader variants, and compatibility behaviors from the Java source.
- Full user-interface localization and visual checks across common Windows scaling settings.
- End-to-end runtime comparison against the Java launcher.

See [PORTING_PLAN.md](PORTING_PLAN.md) for the tracked checklist. The checklist has 99 completed entries and 10 open entries; this is a count of tracked items, not a feature-parity percentage. A reliable parity estimate needs the remaining source and runtime review.

## Windows build

Latest Windows x64 release build and portable ZIP packaging passed on 2026-11-06.

From the project directory:

```powershell
.\Scripts\build-windows.ps1
.\Scripts\package-windows.ps1
```

Packaging generates `dist/LegacyLauncher-Rust-portable.zip`; build output and the archive are excluded from Git.
