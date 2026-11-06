# Rust port parity plan

Target: Windows x64 feature parity with the local Java source in `D:\test\LegacyLauncher`.

The Java project has a separate Bootstrap module. This plan focuses on the desktop launcher and its game-management flows; bootstrap parity is tracked in the packaging milestone.

## Baseline already present

- [x] Login panel recreates the supplied LegacyLauncher screen with embedded background and icons.
- [x] US English strings are embedded.
- [x] Offline account names, version selection, force-update toggle, and basic settings persistence.
- [x] Vanilla install/download/verify/launch path through `minecraft-java-rs-core`.

## Milestones

### 1. Screen and navigation parity

- [x] Implement home, account manager, version manager, and core launcher/game settings screens.
- [x] Fetch the personal notice feed in US English, render embedded/remote images, display notices from the launcher and a notice manager, persist one-week hides, and handle safe URL/known scene/promoted-server actions.
- [x] Render optional personal-feed notice banners.
- [x] Fetch and verify the source's signed bootstrap configuration; show US/global fallback notices and notification links, and merge US/global promoted servers into Minecraft's server list.
- [x] Add the US support popup and links to the source Discord and support email.
- [x] Check for newer GitHub releases and show release notes for a newer Rust build.
- [x] Move settings-page titles/controls and common notice, log, server, account, and update labels into the US locale resource.
- [x] Port the Java `NoticeScene` as a full-screen notice flow with previous/next navigation, one-week hide/restore, notice actions, and a home action.
- [x] Port the Java SettingsPanel About us tab with the US source copy, version display, creator/partner links, and license text.
- [x] Port startup connectivity checks and the source-style warning window for Mojang metadata, Legacy Launcher repositories, bootstrap hosts, and Ely.by.
- [ ] Port the remaining scene and dialog flows found under `launcher/src/main/java/net/legacylauncher/ui`. The crash dialog now honors `permitHelp` and exposes source-style help, log, and exit actions; other scene/dialog comparisons remain open.
- [x] Add opt-in diagnostic upload, success link actions, sensitive-line filtering, and a local-save fallback using the source Pasta endpoint.
- [x] Include Windows version/build, system manufacturer/model, language, DirectX registry version, processor, memory, and graphics provider/driver/date details in the opt-in diagnostic report without collecting hardware serial numbers or device IDs.
- [x] Move version filters, notices/account controls, and launch/download event labels into the US locale resource; every literal `tr("key")` in Rust currently resolves to a US property.
- [ ] Finish moving remaining backend/network and dynamic notice/crash error wording into the locale; Microsoft/Ely.by authentication, catalog, bootstrap, notices, diagnostics, updater, Java/server, import, background-image, and crash fallback errors are localized. The update, bootstrap, notices, and diagnostic lifecycle log messages now also resolve from the US locale.
- [ ] Load all visible strings from the US locale resource and match the source layout at common Windows scales.
- [x] Wire the login toolbar to support, game-folder/mods-folder, catalog refresh, and settings actions.
- [x] Port the source's nine login-form positions and migrate `gui.direction.loginform`.
- [ ] Match the original toolbar popup details and verify common Windows scales visually. The Rust menu now follows the source's Launcher, Versions, Accounts order and only shows Notices when the feed has entries; Rust-only About, Servers, and update actions moved into launcher preferences. Multi-scale visual verification remains open.

### 2. Accounts and authentication

- [x] Add/edit/remove/select offline accounts.
- [x] Add/remove/select Microsoft accounts and persist their Minecraft profiles.
- [x] Link the login account selector directly to Manage accounts and distinguish Ely.by Legacy from OAuth accounts.
- [x] Port the account-type help dialog and the US Buy Minecraft action from the source account-add flow.
- [x] Add/manage/import Ely.by OAuth accounts, protect credentials with DPAPI, and refresh tokens before launch.
- [x] Add/manage/import Ely.by Legacy accounts, protect credentials with DPAPI, and refresh tokens before launch.
- [x] Port browser OAuth callback, Xbox Live/XSTS, Minecraft Services login, ownership checks, and refresh-token flow.
- [x] Protect Microsoft refresh tokens with the current Windows user's DPAPI.
- [x] Add Minecraft profile creation during Microsoft sign-in when the account lacks one.
- [x] Migrate retired Mojang entries from `tlauncher_profiles.json` to offline identities with their selected profile UUID; online authentication requires Microsoft sign-in.
- [x] Port the bootstrap `--requireMinecraftAccount` gate for new offline/Ely accounts, show the US license-required notice, and retain imported Mojang identity for the source-compatible exception.
- [x] Import offline, Microsoft, Ely.by OAuth, and Ely.by Legacy accounts from `tlauncher_profiles.json`, preserving offline UUIDs and moving online credentials into DPAPI storage.
- [x] Import supported Java launcher game, display, Java, argument, directory, and launch-action settings from `legacy.properties`.
- [x] Import `login.auto` and its 2–10 second `login.auto.timeout`; show the source-style cancel countdown before automatic launch. Also import `login.version`, `gui.size`, and `gui.window`; use the source's 1000×600 initial launcher size, expose editable dimensions, and preserve maximized state.
- [x] Import notice visibility, promoted-notice preference, and per-notice hide expiries from `legacy.properties`.
- [x] Import the crash scanning and full launch command logging preferences.
- [x] Import and expose the source's nine login-form positions.
- [x] Import `gui.font`, clamp it to the source's 12–18 pt range, and apply it as launcher-wide UI scaling with a live preferences control.
- [x] Import `gui.font.old` and show the source's revert/dismiss font-size prompt, applying the restored value immediately with live Rust UI scaling.
- [x] Port source command-line configuration for settings path, version, game directory, Java path, arguments, resolution, background, fullscreen, theme, JRE root, auto-launch, and `--username`/`--usertype`; implement `--help`/`--debug` and enforce the source settings schema-version gate.
- [x] Import `gui.laf.v1.state` and the pre-FlatLaf `gui.systemlookandfeel` setting, then expose the source Off/System/Auto/Dark/Light modes as live system-aware UI themes.
- [x] Import `gui.background` image paths and add a live background image picker/reset; video backgrounds remain unsupported.
- [x] Import `gui.logger.width/height/x/y`, use them as the log viewer's initial geometry, and persist its size and position after moving/resizing.
- [x] Import `gui.logger`, add the source Global/None setting, show live launcher events by default, capture Minecraft output in the rolling launcher log, and turn Global mode off when the log window is closed.
- [x] Replace the generic status-driven logger fallback with explicit status events carrying severity and subsystem targets; launch events and startup background-image errors are logged directly.
- [ ] Port the remaining Java Log4j diagnostics and compare logger coverage against the Java source. In-memory log records now carry timestamps, levels, and subsystem targets; Minecraft download/process, auth, Java, mod, server, catalog, update, bootstrap, notice, diagnostic, settings-import, and crash events are categorized. A process-wide Rust panic hook now writes redacted panic details and a forced backtrace to the rolling launcher log; full per-call-site coverage is still open.
- [x] Record source-style startup version/platform, working-directory, and settings-file context in the launcher log.
- [x] Import `bootstrap.switchToBeta` and expose it as an opt-in prerelease update channel for the Rust self-updater.
- [ ] Migrate the remaining Java launcher settings and unsupported legacy account types. The Rust importer preserves `gui.theme` and `gui.settings.blocked`, including source toolbar visibility; theme color, border, shadow, arc, and toolbar icon color properties are applied. `connection.ssl` now maps to a live opt-in TLS certificate/hostname verification setting across launcher and Minecraft HTTP clients, with a visible warning when disabled. Other settings and account-type comparison remain open.
- [x] Follow the source `darkTheme.enabled` theme property when selecting the embedded background, with system/theme-background detection when the property is absent; preserve custom background selection until reset.
- [x] Preserve the source `client` UUID and import/run all six `experiments.enabled` options, including runtime-gated ZGC generational support and LWJGL/JNA replacement profiles.
- [x] Port plain/offline account validation, including the source's opt-in for non-standard names.
- [x] Preserve each offline account's `elySkins` preference (defaulting to the Java source's `true` value), import it from `tlauncher_profiles.json`, and apply the matching Ely/plain/Minecraft replacement libraries from `libraries/replace.json` with repo failover and isolated launch caches.

### 3. Version catalog and lifecycle

- [x] Fetch the official manifest and filter/search release, snapshot, old beta/alpha, and installed versions.
- [x] Port version-category defaults, enable/disable filters, the only-installed and pre-1.7.10 filters, and import these values from `legacy.properties`.
- [x] Install, verify/repair, refresh, and remove versions from the manager screen.
- [x] Load installed custom version JSON, resolve `inheritsFrom` parents locally or from Mojang, merge libraries/arguments/downloads, and reject cycles or unsafe IDs.
- [x] Fetch Legacy Launcher's ExtraVersionList alongside Mojang from the source host mirrors, retain all extra release types, and route selected versions through the launch backend; normalize repository-relative JSON URLs and fail over between mirrors.
- [x] Match source category defaults, old-release/old-alpha/old-beta handling, only-installed filtering, and local/remote catalog visibility; import these controls from `legacy.properties`.
- [x] Import and expose the source `gui.alerton.*` preferences, compare refreshed remote versions with `versions/versions.json`, and show the source US update alert with relative release times.
- [x] Show per-category progress, speed and ETA, support cancellation, and return download/launch errors to the UI.
- [x] Retry network/5xx/429 download failures with backoff and write through temporary files so interrupted downloads do not replace valid files.
- [x] Continue interrupted file transfers with HTTP Range retries, then validate final size and checksum before commit.
- [x] Match the source's task lifecycle: discard unfinished partials when a new launcher download task starts.

### 4. Java runtime and launch configuration

- [x] Detect a usable system Java executable and let users choose it.
- [x] Download the recommended runtime automatically through the launch backend when no custom Java is set.
- [x] Match Recommended, Current System Java, and Custom Java modes; check a chosen executable against the selected game's Java requirement.
- [x] Show installed managed Java executables, open the runtime folder, and select a discovered executable as a custom Java path.
- [x] Configure the managed-runtime root, retain the existing game-folder runtime default, and import `minecraft.jre.dir`.
- [x] Port the source Auto memory mode using per-version RAM requirements, safe-memory clamping, Forge recommendations, and the legacy `minecraft.memory` fallback migration.
- [x] Port optimized JVM profiles (None, Default, G1GC, Shenandoah, ZGC), current-Java trust-store compatibility, and `%command%` wrapper commands.
- [x] Save and show the actual managed/custom Java executable and detected major version for the selected game's last successful install.
- [x] Set game directory, memory, Java executable, screen size/fullscreen, and JVM/game arguments.
- [x] Port none/family/version game folders and hide/exit/do-nothing launch behavior.
- [x] Port system/integrated/discrete GPU selection and the Windows per-Java registry preference hook.
- [x] Port all source launch/exit settings: keep the launcher open, hide it, or close it when Minecraft starts.
- [x] Add, edit, remove, and save a Rust launcher server list; quick play opens selected servers using the installed version's quick-play feature rule or legacy `--server`/`--port` arguments.
- [x] Let personal promoted-server notice actions quick-play the server and honor account-type restrictions when provided.
- [x] Read and write Minecraft `servers.dat` in the selected game folder, preserve server fields, and create a `.bak` before replacing it.
- [x] Offer promoted-server current-version and up to two latest compatible family-version choices, including account-type restrictions.
- [x] Import the promoted-server in-game setting and merge available US notice promotions into `servers.dat`, filtered by family with backup and stale-entry cleanup.
- [x] Import `minecraft.deletePatchy` and apply its library replacement profile together with the selected account profile.
- [x] Fetch bootstrap-provided global promoted servers and merge them into the in-game server list.

### 5. Mod loaders and game compatibility

- [x] Inventory the source's ExtraVersion JSON loader flow and Forge mod-pack compatibility (`legacy`, `1.12.2_bug`, `aquatic`, and `forge_transformer`).
- [x] Select and install/repair/launch Forge, NeoForge, Fabric, LegacyFabric, and Quilt through the Rust launch backend.
- [x] Keep each loader/build's cache and save directory separate from vanilla and other loader builds.
- [x] Port source Forge mod-list files, 1.13+ mod-list arguments, version-inferred compatibility, and `forge_transformers` argument substitution.
- [x] Port the source's unwanted-mod cleanup patterns, default setting, settings UI, legacy setting migration, and launch-time deletion scoped to the selected game's `mods` folder.
- [x] Port source Fractureiser scanning before game downloads: recurse through nested mods folders, scan JAR classes with all three bytecode signatures, import and persist the `jarscanner` version, allow cancellation, prompt on long scans, and block launch when a signature is detected.

### 6. Diagnostics, notices, and support

- [x] Capture game output and show a crash log with initial rule-based suggestions.
- [x] Preserve the source `minecraft.crash` preference, including legacy import, and skip crash scanning when disabled.
- [x] Preserve the source `gui.logger.fullcommand` opt-in and only include the sanitized launch command in logs when enabled.
- [x] Port the bundled US English crash signatures, richer explanations, and supported suggested-fix actions.
- [x] Add normal log browsing, preview/copy, and log/game-folder actions in the launcher UI.
- [x] Fetch and show personal US English notices, including persisted one-week hiding and supported URL/scene/promoted-server actions.
- [x] Render optional personal-feed notice banners and merge available notice server actions into the in-game list.
- [x] Port signed bootstrap notice, notification-icon, and global promoted-server delivery for the US locale.
- [x] Download the release executable with a size limit, verify its SHA-256, and hand off replacement/restart to a Windows helper process.
- [ ] Validate the download and restart flow against a published Windows release artifact.
- [ ] Port the source new-feature dialog if it becomes active in the source launcher flow (`NewFeaturesFrame` has no call site in this checkout).
- [x] Port the source feedback-on-exit prompt using the US/global survey URL from bootstrap configuration.
- [x] Port the US Discord/email support links and opt-in diagnostics flow.

### 7. Windows delivery and parity review

- [ ] Complete signed-metadata bootstrap self-update, validate the release handoff, and finish remaining first-run behavior.
- [x] Show the source's US first-run welcome and acknowledgment notice when the Rust settings file is absent; save settings after acknowledgment.
- [x] Detect first-run game-folder conflicts, offer source-compatible destination suggestions and a folder browser, validate the selected path, and preserve the source data without copying it.
- [x] Show the source's Windows ARM OpenGL compatibility notice on first run, with a five-second install countdown and support action.
- [x] Add a tagged Windows release workflow that publishes the executable, portable ZIP, and SHA-256 checksums.
- [x] Package a portable Windows x64 folder/ZIP; assets are embedded and the release build does not need an external Rust/Java runtime to start.
- [x] Check for Rust release updates at startup and defer the release-notes prompt until first-run setup is complete; keep the verified download and Windows replacement/restart flow.
- [ ] Review every user-facing flow against the Java source and close the remaining parity gaps.

## Current focus

Next: continue remaining source scenes/settings, localization, visual-scale comparison, and end-to-end runtime review. Validate the checksum-verified executable download and Windows updater handoff against a published release. The update check uses the Rust project's GitHub Releases and displays release notes; a tag-triggered Windows workflow prepares executable, portable ZIP, and SHA-256 assets. The current repository has no published release yet, so the download and restart handoff have not been exercised with a release artifact. In the Java source, `UpdateFrame` is shown after the bootstrap update flow supplies notes; `NewFeaturesFrame` has no call site in this checkout. Loader parity includes Forge `legacy`, `1.12.2_bug`, and `aquatic` mod-pack argument modes plus `forge_transformer` placeholder expansion; the launch backend also installs Forge, NeoForge, Fabric, LegacyFabric, and Quilt. Bootstrap data now uses the Java source's signed endpoint format, embedded RSA public key, SHA-256 digest and RSA/SHA-1 verification, gzip/bzip2 decoding, host failover, US notice parsing, direct promoted-server merge, notification links, and US/global feedback URLs. The support flow includes opt-in diagnostic upload to the source Pasta endpoint, filtering of lines that may contain credentials, link actions, and a local-save fallback. The unwanted-mod cleanup matches the source filename patterns, default, migration, and launch timing. The ExtraVersionList catalog merge and remote version resolver plus account-specific Ely/plain/Minecraft and optional Patchy library replacements are implemented; end-to-end runtime behavior remains in the final flow-by-flow review. Java settings include a configurable managed-runtime root, detected runtime path/version display, all five optimized argument profiles, current trust-store compatibility, wrapper commands, and migration of their source properties. Promoted-server notice actions offer the selected version and up to two recent releases from each compatible family; available US promotions can be added to `servers.dat` with a backup. Ely.by OAuth and Legacy accounts support sign-in/import, DPAPI storage, token refresh, and selection; retired Mojang profiles import as offline identities for local play. The version manager exposes source category filters, remote visibility, only-installed and pre-1.7.10 filters, and imports these settings. The personal US notice feed with embedded/remote images, Minecraft `servers.dat` management and quick play, Microsoft profile creation, loader isolation, tri-state game folders, Java mode/compatibility handling, US crash signatures, log browsing, custom version inheritance, in-task Range resume, and supported Java settings migration are implemented. The source crash-analysis toggle and opt-in full launch-command logging now migrate from Java settings and control Rust behavior.
