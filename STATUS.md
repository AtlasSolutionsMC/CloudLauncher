# Port status

Updated: 2026-11-07 (Asia/Bangkok)

Implementation is ongoing; feature parity remains incomplete.

## Implemented so far

- Windows x64 Rust launcher with the reference login screen, US English locale, account/version/settings screens, and a portable packaging script.
- Login panel placement can be selected from all nine positions supported by the Java launcher and is imported from `legacy.properties`.
- Offline, Microsoft, Ely.by OAuth, and Ely.by Legacy account flows.
- Account type guidance with Microsoft/offline/Ely.by descriptions, plus source-style Buy Minecraft and Ely.by registration links.
- Login account dropdown opens Manage accounts directly and labels Ely.by Legacy accounts distinctly from Ely.by OAuth.
- The Manage accounts page includes the source welcome tip with Rust-specific US instructions for adding, selecting, and editing accounts.
- Optional `--requireMinecraftAccount` account-add gate with the source US warning and Mojang-import exception.
- Minecraft version catalog and lifecycle, custom inherited versions, Java runtime management, Forge/NeoForge/Fabric/LegacyFabric/Quilt launch paths, and source Forge mod-pack argument compatibility.
- Source Auto memory allocation by Minecraft release family and available RAM, including Forge scaling, the Auto/manual setting, and migration of the retired `minecraft.memory` preference.
- Pre-launch Fractureiser signature scan of nested mods JARs, matching the Java detector's three bytecode signatures; migrate and persist scan version, offer long-scan continue/skip, and stop launch with a detection dialog when infected.
- Crash dialogs honor the Java signature catalog's `permitHelp` flag and include help, log, and dismiss actions.
- Game settings, server list and quick play, Minecraft `servers.dat` integration, promoted servers, crash suggestions, log browsing, and opt-in diagnostics with Windows, model, language, DirectX, CPU/memory, and graphics provider/driver details.
- Source crash-analysis and full-command logging preferences, with legacy settings migration.
- `connection.ssl` migration and live TLS certificate/hostname verification across launcher and Minecraft download HTTP clients; disabling it is explicit, off by default, and shows a warning.
- Source-style startup version/platform, working-directory, and settings-file entries in the launcher log.
- Update, bootstrap, notice, and diagnostic lifecycle log messages are loaded from the US locale, including the dynamic loaded-notice count.
- Settings toolbar popup follows the source's Launcher, Versions, Accounts order and conditionally adds Notices; Rust-only About, Servers, and update actions are available from launcher preferences.
- Legacy `gui.font` migration and live launcher-wide text/UI scaling from 12 to 18 pt.
- Legacy `gui.font.old` migration with the source revert/dismiss prompt; restoring the earlier size applies live in Rust.
- Source command-line configuration for an external Java properties or Rust JSON settings file, including the source schema-version gate; version, directory, Java, arguments, window, background, fullscreen, theme, runtime root, delayed auto-launch, username/type account selection, and help/debug startup behavior.
- Legacy `gui.laf.v1.state` and pre-FlatLaf `gui.systemlookandfeel` migration with live Off/System/Auto/Dark/Light UI theme selection.
- Legacy `gui.background` image migration and live image selection/reset; video backgrounds are still open.
- Legacy `gui.theme` file migration and selection; foreground, secondary foreground, background, panel, success/failure, borders, shadow, corner-radius, and toolbar icon color properties are applied to the Rust UI.
- Legacy `darkTheme.enabled` background asset selection, including source-style system/theme background fallback and keeping user-selected backgrounds active until reset.
- Legacy log-window dimensions and position migration, with current viewer geometry saved across launches.
- Source `gui.logger` Global/None mode with a 1 MiB live launcher buffer, a rolling 12 MiB launcher log with ten backups, live sanitized logs, log-file browsing, a stop-game action, and close-to-disable behavior. In-memory records now retain timestamp, severity, and subsystem; launch/download, auth, Java, mods, servers, catalog, update, bootstrap, notice, diagnostics, settings import, and crash events have explicit categories. Status updates now log directly with a level and subsystem. Unhandled Rust panics also write redacted details and a forced backtrace to the rolling log; full Java Log4j coverage comparison remains open.
- Migration and persistence for the selected Minecraft version and maximized launcher window state.
- Legacy auto-login imports its configurable 2–10 second delay and shows a live cancel-start countdown before launching; first-run notices and folder selection finish before the countdown begins.
- Legacy `gui.settings.blocked` migration hides the folder and settings toolbar buttons, matching the Java login toolbar.
- Source version update alerts for enabled release types, backed by the compatible `versions/versions.json` cache and imported `gui.alerton.*` settings.
- Settings and common dialog labels now resolve from the US locale resource.
- The source SettingsPanel About us tab includes the US copy, Rust clone version, creator and Ely.by links, and source license text.
- Source HTML formatting is preserved in About, account-creation success, connectivity-warning, and Doomsday messages, including paragraphs, emphasis, lists, and safe clickable HTTP(S) links. Link failures are reported under the matching launcher or connectivity subsystem.
- Startup connectivity checks cover Mojang version metadata, Legacy Launcher repository mirrors, both bootstrap hosts, and Ely.by; failures show a warning badge with affected hosts and source help copy.
- Version filters, notice/account controls, launch/download status labels, and error text for Microsoft/Ely.by authentication, catalog, bootstrap, notices, diagnostics, updater, Java/server handling, imports, background images, crash fallbacks, external-link/OAuth browser-launch failures, connectivity-check runtime/request/HTTP/JSON failures, and optional notice-image download failures now resolve from US locale entries. Connectivity lifecycle logs use US locale labels while retaining host-specific transport details. Optional notice/bootstrap image failures are logged under their subsystem without exposing query strings or suppressing the notice. Failed external-link launches are logged by subsystem; notification and first-run notices remain available to retry. The ExtraVersionList catalog now also prefers signed-bootstrap `extra_version_repo` mirrors before built-in fallbacks. Dynamic errors and source-provided notice/crash copy still need a full-flow review.
- US personal notices and signed bootstrap configuration with US notices, promoted servers, and notification links; notice content preserves safe inline emphasis, paragraph breaks, and clickable HTTP(S) links across the side panel, notice manager, and Java-style full-screen notice scene with previous/next navigation, timed hide/restore, notice actions, and a home action.
- Separate the Java `notice.enabled` permission from `gui.notices.enabled` panel visibility; signed `allowNoticeDisable` is evaluated against the preserved client UUID using Java's hash rule, and both settings round-trip through legacy properties.
- Startup and manual GitHub release checks with release notes, bounded executable download, SHA-256 verification, and a Windows replacement/restart handoff; the startup prompt waits until first-run setup is complete. A tag-triggered workflow publishes the assets.
- The migrated `bootstrap.switchToBeta` preference enables opt-in GitHub prerelease updates; the default channel continues to use stable releases.
- Source-style US first-run notice with a required acknowledgment before launcher controls can be used.
- Windows ARM first-run OpenGL compatibility notice with the source's five-second install countdown and support action.
- First-run game-folder conflict detection and destination selection with browse, path validation, and no-copy warning; settings also has a native folder picker.
- Ely.by OAuth client secret is supplied through `LEGACYLAUNCHER_ELY_CLIENT_SECRET`; no client secret is checked into this public repository.
- Legacy settings import preserves the source client UUID and all six configured Java experiments; signed bootstrap `ely` permission now gates Ely replacement libraries for offline accounts exactly as in Java, while JVM experiments and experimental LWJGL/JNA replacements are applied during launch.

## Still open

- Validate the executable download and Windows updater handoff against a published release artifact (the repository currently has no releases).
- The Java source's new-feature dialog has no call site in this checkout.
- Remaining scenes, settings, account variants, loader variants, and compatibility behaviors from the Java source.
- Full user-interface localization and visual checks across common Windows scaling settings.
- End-to-end runtime comparison against the Java launcher.

See [PORTING_PLAN.md](PORTING_PLAN.md) for the tracked checklist. It has 113 completed entries and 8 open entries (93.4% of tracked checklist items); this is a checklist count, not a measured feature-parity percentage. Recent rich-text work also covers localized About, account-success, and connectivity messages. A reliable parity estimate still needs the remaining source and runtime review.

## Windows build

Latest Windows x64 release build and portable ZIP packaging passed on 2026-11-07.

From the project directory:

```powershell
.\Scripts\build-windows.ps1
.\Scripts\package-windows.ps1
```

Packaging generates `dist/LegacyLauncher-Rust-portable.zip`; build output and the archive are excluded from Git.
