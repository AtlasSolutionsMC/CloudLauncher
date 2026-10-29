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
- [ ] Port the remaining scene flows found under `launcher/src/main/java/net/legacylauncher/ui`.
- [x] Add opt-in diagnostic upload, success link actions, sensitive-line filtering, and a local-save fallback using the source Pasta endpoint.
- [ ] Load all visible strings from the US locale resource and match the source layout at common Windows scales.
- [x] Wire the login toolbar to support, game-folder/mods-folder, catalog refresh, and settings actions.
- [ ] Match the original toolbar popup details and verify common Windows scales visually.

### 2. Accounts and authentication

- [x] Add/edit/remove/select offline accounts.
- [x] Add/remove/select Microsoft accounts and persist their Minecraft profiles.
- [x] Add/manage/import Ely.by OAuth accounts, protect credentials with DPAPI, and refresh tokens before launch.
- [x] Add/manage/import Ely.by Legacy accounts, protect credentials with DPAPI, and refresh tokens before launch.
- [x] Port browser OAuth callback, Xbox Live/XSTS, Minecraft Services login, ownership checks, and refresh-token flow.
- [x] Protect Microsoft refresh tokens with the current Windows user's DPAPI.
- [x] Add Minecraft profile creation during Microsoft sign-in when the account lacks one.
- [x] Migrate retired Mojang entries from `tlauncher_profiles.json` to offline identities with their selected profile UUID; online authentication requires Microsoft sign-in.
- [x] Import offline, Microsoft, Ely.by OAuth, and Ely.by Legacy accounts from `tlauncher_profiles.json`, preserving offline UUIDs and moving online credentials into DPAPI storage.
- [x] Import supported Java launcher game, display, Java, argument, directory, and launch-action settings from `legacy.properties`.
- [x] Import `login.auto` and `gui.size`; use the source's 1000×600 initial launcher size and expose editable dimensions.
- [x] Import notice visibility, promoted-notice preference, and per-notice hide expiries from `legacy.properties`.
- [ ] Migrate the remaining Java launcher settings and unsupported legacy account types.
- [x] Port plain/offline account validation, including the source's opt-in for non-standard names.
- [x] Preserve each offline account's `elySkins` preference (defaulting to the Java source's `true` value), import it from `tlauncher_profiles.json`, and apply the matching Ely/plain/Minecraft replacement libraries from `libraries/replace.json` with repo failover and isolated launch caches.

### 3. Version catalog and lifecycle

- [x] Fetch the official manifest and filter/search release, snapshot, old beta/alpha, and installed versions.
- [x] Port version-category defaults, enable/disable filters, the only-installed and pre-1.7.10 filters, and import these values from `legacy.properties`.
- [x] Install, verify/repair, refresh, and remove versions from the manager screen.
- [x] Load installed custom version JSON, resolve `inheritsFrom` parents locally or from Mojang, merge libraries/arguments/downloads, and reject cycles or unsafe IDs.
- [x] Fetch Legacy Launcher's ExtraVersionList alongside Mojang from the source host mirrors, retain all extra release types, and route selected versions through the launch backend; normalize repository-relative JSON URLs and fail over between mirrors.
- [x] Match source category defaults, old-release/old-alpha/old-beta handling, only-installed filtering, and local/remote catalog visibility; import these controls from `legacy.properties`.
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
- [x] Port optimized JVM profiles (None, Default, G1GC, Shenandoah, ZGC), current-Java trust-store compatibility, and `%command%` wrapper commands.
- [x] Save and show the actual managed/custom Java executable and detected major version for the selected game's last successful install.
- [x] Set game directory, memory, Java executable, screen size/fullscreen, and JVM/game arguments.
- [x] Port none/family/version game folders and hide/exit/do-nothing launch behavior.
- [x] Port system/integrated/discrete GPU selection and the Windows per-Java registry preference hook.
- [ ] Port the remaining launch/exit behavior settings.
- [x] Add, edit, remove, and save a Rust launcher server list; quick play opens selected servers using the installed version's quick-play feature rule or legacy `--server`/`--port` arguments.
- [x] Let personal promoted-server notice actions quick-play the server and honor account-type restrictions when provided.
- [x] Read and write Minecraft `servers.dat` in the selected game folder, preserve server fields, and create a `.bak` before replacing it.
- [x] Offer promoted-server current-version and up to two latest compatible family-version choices, including account-type restrictions.
- [x] Import the promoted-server in-game setting and merge available US notice promotions into `servers.dat`, filtered by family with backup and stale-entry cleanup.
- [x] Import `minecraft.deletePatchy` and apply its library replacement profile together with the selected account profile.
- [x] Fetch bootstrap-provided global promoted servers and merge them into the in-game server list.

### 5. Mod loaders and game compatibility

- [ ] Inventory loader integrations and compatibility rules in the Java source.
- [x] Select and install/repair/launch Forge, NeoForge, Fabric, LegacyFabric, and Quilt through the Rust launch backend.
- [x] Keep each loader/build's cache and save directory separate from vanilla and other loader builds.
- [ ] Port any source-specific loader variants.
- [x] Port the source's unwanted-mod cleanup patterns, default setting, settings UI, legacy setting migration, and launch-time deletion scoped to the selected game's `mods` folder.

### 6. Diagnostics, notices, and support

- [x] Capture game output and show a crash log with initial rule-based suggestions.
- [x] Port the bundled US English crash signatures, richer explanations, and supported suggested-fix actions.
- [x] Add normal log browsing, preview/copy, and log/game-folder actions in the launcher UI.
- [x] Fetch and show personal US English notices, including persisted one-week hiding and supported URL/scene/promoted-server actions.
- [x] Render optional personal-feed notice banners and merge available notice server actions into the in-game list.
- [x] Port signed bootstrap notice, notification-icon, and global promoted-server delivery for the US locale.
- [ ] Port update/new-feature dialogs and the source self-update/bootstrap handoff flow.
- [x] Port the source feedback-on-exit prompt using the US/global survey URL from bootstrap configuration.
- [x] Port the US Discord/email support links and opt-in diagnostics flow.

### 7. Windows delivery and parity review

- [ ] Port launcher self-update/bootstrap handoff and first-run behavior.
- [x] Package a portable Windows x64 folder/ZIP; assets are embedded and the release build does not need an external Rust/Java runtime to start.
- [ ] Review every user-facing flow against the Java source and close the remaining parity gaps.

## Current focus

Next: port update/new-feature dialogs and a Rust-native self-update/bootstrap handoff, then cover remaining source scenes, settings/account variants, loader variants and compatibility behavior, all remaining US localization, and complete the final parity review. Bootstrap data now uses the Java source's signed endpoint format, embedded RSA public key, SHA-256 digest and RSA/SHA-1 verification, gzip/bzip2 decoding, host failover, US notice parsing, direct promoted-server merge, notification links, and US/global feedback URLs. The support flow includes opt-in diagnostic upload to the source Pasta endpoint, filtering of lines that may contain credentials, link actions, and a local-save fallback. The unwanted-mod cleanup matches the source filename patterns, default, migration, and launch timing. The latest Windows release build and portable packaging passed after these changes. The ExtraVersionList catalog merge and remote version resolver plus account-specific Ely/plain/Minecraft and optional Patchy library replacements are implemented; end-to-end runtime behavior remains in the final flow-by-flow review. Java settings include a configurable managed-runtime root, detected runtime path/version display, all five optimized argument profiles, current trust-store compatibility, wrapper commands, and migration of their source properties. Promoted-server notice actions offer the selected version and up to two recent releases from each compatible family; available US promotions can be added to `servers.dat` with a backup. Ely.by OAuth and Legacy accounts support sign-in/import, DPAPI storage, token refresh, and selection; retired Mojang profiles import as offline identities for local play. The version manager exposes source category filters, remote visibility, only-installed and pre-1.7.10 filters, and imports these settings. The personal US notice feed with embedded/remote images, Minecraft `servers.dat` management and quick play, Microsoft profile creation, loader isolation, tri-state game folders, Java mode/compatibility handling, US crash signatures, log browsing, custom version inheritance, in-task Range resume, and supported Java settings migration are implemented.
