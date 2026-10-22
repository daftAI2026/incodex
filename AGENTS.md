# Incodex Agent Guide

This file is the shared source of truth for any AI agent working on this repo (Claude Code, Codex, Grok, etc.). `CLAUDE.md` is a symlink to this file. Put machine-specific or personal overrides in `AGENTS.local.md` / `CLAUDE.local.md`; both are gitignored.

Keep this shared guide focused on actual behavior and safety contracts. Local experimental boundary contracts and module maps must describe the implemented behavior without weakening those contracts.

## Project

Incodex adds a Chrome-style incognito window to a locally installed OpenAI Codex desktop app (`ChatGPT.app`, `com.openai.codex`). It is an unofficial add-on. The installer patches the official bundle. `incodex open` is a second launch path: it does not patch or resign the official app, but that isolated window still gets the hat-glasses control and banner by injecting the same `inject.js` over a localhost debug port.

Safety rules matter more than speed. Treat installer, signing, session cleanup, and IPC as the dangerous surface.

## Runtime hot-update contract

Read [RUNTIME.md](RUNTIME.md) before changing any Runtime behavior. Hot update is the default design requirement for every Incodex-owned feature, not an optional follow-up. Reuse the existing verified-generation transaction: prepare inert services, require exact acknowledgements, commit one active generation, then retire old requests/resources. Keep IPC, window/session ownership and authorization in the stable host; do not duplicate them in a candidate factory. Never exit/restart the official App, clear module caches, or bypass hashes to simulate live activation. Published files do not prove active services. The contract distinguishes the full-coverage target from implemented and real-app-accepted capabilities; native images and ASAR bootstrap require explicit migration boundaries, not a blanket hot-update claim.

Permission-only business updates must reuse the installed responder and registered native helper. Do not change fixed Runtime verifier/bootstrap bytes for behavior that already fits that protocol. Execute the candidate with the frozen installed verifier before deployment, then prove both the same-process Runtime ACK and execution of the matching new helper. Native asset changes need their own hot path and acceptance; a new whitelist does not upgrade an already running verifier.

The opt-in `INCODEX_DIAGNOSTIC_QUIT_TRACE=1` observer records the official `sky.node` privacy-termination getter and synchronous quit lifecycle only. It never derives its own sender decision, eagerly loads the addon, changes return values, prevents quit, or relaunches the App. Its early load observation is embedded in the fixed external main, not the pure action factory. Enabling it requires an explicitly authorized normal-restart diagnostic migration; it is not a hot-update capability or an automatic-reopen fix. Logs reuse the bounded target Runtime log and contain closed event codes, PID and booleans only. `getter-attached` proves wrapping without invoking the getter; `enabled` alone proves only the opt-in entry.

The native `INCODEX_DIAGNOSTIC_UPDATE_TRACE=1` observer is also default-off. It records existing Sparkle interposer matches, the original delegate's on-quit return or exception, and Coordinator branch/helper outcomes through `com.daftai.incodex` / `macos-update-diagnostic` system logs. It preserves the original callback receiver, selector, update-item object, immediate block, return and exception; the official delegate alone owns installation. The shared header's stderr sink is compile-time test-only. No new refusal, wait, relaunch or pending schema belongs in this observer. Native diagnostic bytes require a separate explicit migration and do not prove hot activation or automatic-update recovery.

## macOS queued-update continuation

The matched Sparkle adapter records `installQueued` after either normal NO or YES callback return. It uses the existing pending lock and captured install/app/handoff/owner identity. It never copies or invokes the installation block, changes the original BOOL or exception, or depends on the diagnostic flag. A state-write failure must fail open to the official updater. The shared lock releases its descriptor in `@finally`, including an owned state exception caught by the adapter.

A queued ordinary Host close preserves `sourceBuild` and `relaunchHost=false`. The existing Coordinator starts its same content-addressed executable in a one-shot console mode, then exits its AppKit instance so Sparkle can finish installation. The console never creates `NSApplication`; it waits for the old owner to exit, adopts the same handoff under the existing CAS, arms a parent-directory vnode before reading the build, and calls the registered helper once only after a different build exists. The 180-second bound covers machine installation waiting, not human authorization or helper work. No new package means no helper. Successful recovery burns its owned pending state without reopening a normally closed Host.

If the replacement Host is already running, the existing Coordinator keeps pending ownership and waits on exact executable-path process-exit events without modifying or terminating that Host. The registered helper reports internal exit 75 only when its initial, pre-transaction PID probe positively finds the target running. A probe, signing, generation, transaction or rollback error remains an ordinary failure. Persist `relaunchHost=false` under the pending CAS before waiting or retrying; if the busy target has already exited before the callback, revalidate once without reopening it. A repeated busy result with no live target exhausts that immediate retry. Cancellation or a superseded handoff never resumes. These native changes require migration and real-App acceptance; deferred recovery does not prove that the first post-update window was injected.

Explicit Sparkle termination/relaunch retains the existing AppKit recovery path. A competing second stage cannot steal a live same-executable owner, and retained `relaunchHost=false` survives adoption. Failed state publication or console launch still lets the old Coordinator exit: official installation remains available, but that recovery is not successful and retains manual install as fallback. This is not a LaunchAgent, permanent observer, separate installer, or native-image hot-update claim; native migration still requires its own explicit boundary and real-App acceptance.

## Experimental macOS CDP launcher

This branch adds a macOS-only opt-in `inc launch` route and a thin launcher App.
A matching committed Incodex install record selects its existing Loader, IPC and Runtime through
normal LaunchServices activation. It does not add CDP, create another controller,
publish Runtime, uninstall, or re-sign. Use the existing ASAR marker and checksummed transaction reader for launch recognition only. Normal activation does not need mutation-level signing or backup scans; this recognition must never authorize install, restore, or Runtime publication.
An uninstalled official normal App uses LaunchServices plus CDP and reuses
shared renderer/actions plus the existing native `open` session lifecycle.
Windows keeps its existing installed Store integration and official icon; do not
add a second launcher bundle or injection path for this macOS alternative.
It does not replace `install` or change the normal official-icon route. The
existing prohibition on changing the default macOS entry remains in force.
Main-process inspector injection is not this route: the observed official App
has disabled that fuse. Never flip fuses, patch, re-sign or force-quit to enable it.
Renderer/action hot updates reuse the verified-generation transaction and exact
ACKs. Native menus and installed permission responders are not supplied by a
renderer CDP binding and must not be claimed as equivalent. Real normal-App
acceptance is still required; fixture tests do not establish it.

The Finder entry is `~/Applications/ChatGPT - Inc.app`. Generation 4 reads the resolved Host bundle ID's last-published `DockIconPreference` and `DockIconResourceName` keys from that app's preference domain. It accepts `app-default`, `codex-system` and `space-system`; it maps legacy `codex-light` and `codex-dark` to `codex-system`. It decodes only the named PNG or ICNS resource inside the Host bundle. Missing, unknown or unsafe values retain the existing LaunchServices icon fallback. This is a read-only preference mirror, not the canonical settings database; the launcher does not write preferences or change the Host. It adds the embedded INCODEX plaque without copying or changing the Host. The current ownership marker binds the generated icon digest; removal does not require the Host to remain available. Exact V1/V2/V3 entries retain their existing migration and removal contracts. The hat-glasses control remains Runtime UI, not the new Finder icon. `inc launch --remove` previews or confirms removal of only recognized current/legacy entries; it never closes the Host or deletes Runtime/session state. Script-managed CLI self-uninstall reuses that cleanup and leaves unknown entries with a warning.

A live standard Codex profile can belong to an already captured Host at another app path. Treat Chromium's `SingletonLock` PID as a selection hint only. Reuse the unique private published owner after the existing controller/Host identity and CDP ownership checks; activate the recorded app, not the requested path. Recheck the lock, keep the Finder target unchanged, and never delete profile locks or create another normal profile to bypass Chromium's handoff. Without a proven owner, retain the existing launch route. Launch diagnostics retain at most eight private events within 4 KiB; an accepted LaunchServices request is not a visible-window readiness ACK. Dry-run and entry removal do not write these diagnostics.

Publish the validated Finder entry before starting either host route. A conflicting external launcher bundle must fail before Codex starts. Do not defer a fallible icon write until after CDP UI activation.

Transient target-discovery failures and empty snapshots retain still-connected pages and pending native requests. Retry discovery at the existing cadence; exact connection ownership checks still guard every command. A discovery snapshot alone does not prove that a live page closed.

Target discovery and every CDP command parse lsof's `p/f/t/d/n` fields per FD.
Group matching IPv4 endpoint FDs by one validated, nonzero `d` socket identity.
Accept multiple PID records only for that same socket and only when the captured
Host PID, start time, and executable remain exact. Reject missing identities,
multiple socket devices, changed startup sockets, and mismatched IPv4 endpoints.

The Finder shell exits after invoking the same CLI. Only the CDP branch retains
an App-scoped CLI. In that branch, repeated launch
uses a private process/start/executable-bound record and a separate launch lock,
not the install mutation lock. Reactivation uses the captured CDP connection;
it must not fall back to LaunchServices and accidentally start a plain App.
Repeated private-window requests reuse native open and its inherited stdio.
Require the control capability plus actual page/system activation ACK; a pipe
write alone is not success. Keep the owner on failure and keep pending ACKs
nonblocking. No global session agent or new IPC service belongs in this route.

On macOS native open, an explicit official-UI blocker prepares the same shared
renderer payload before waiting for final mode confirmation. It does not click
or dismiss the official dialog, select a model, send fallback keys, or report
OPENED. Blocked retries repair only the current document and use the existing
control marker instead of repeatedly executing the payload. A short-lived CDP
connection must not be recorded as a persistent new-document registration.
After mode and UI acceptance, native open retains the registration's DevTools
session until child exit or disconnect. A reconnect registers the same shared
payload in a fresh session, resets only its recovery clock, never repeats the
mode fallback, and never publishes a second OPENED. Cancellation joins its
existing supervision resources. Preparation failure keeps the original Blocked
result and mode failure budget. Windows keeps its existing one-shot injection
and absolute-deadline policy; this macOS acceptance does not prove Windows reload.

macOS native `open` starts a real child with `posix_spawn` and disclaims inherited
process responsibility, so official Host/CUA business requests belong to the
official child rather than the CLI controller. The dynamically resolved Darwin
`responsibility_spawnattrs_setdisclaim` interface is non-public; absence or failure
must return a launch error, not silently inherit the controller's identity. Keep
the existing session handoff, true child exit status, CDP and burn lifecycle.
This does not grant TCC access. Do not transfer grants, change the parent's
responsibility, or add a permission proxy.

Native `open` loads one read-only WindowServer observer before creating a session.
It does not request CLI Accessibility, Automation, or Screen Recording access.
Pin only a positively observed, unique normal window of the exact child PID.
Require an exact bounds match with the unique CDP primary page bearing the shared
control before pinning, even while an official dialog blocks final mode readiness.
Refresh unmatched startup bounds, then retain that native ID across moves.
Window ownership does not report OPENED or bypass the final mode confirmation.
Minimization and App hide retain the session. Failed queries, foreign identity, or
ambiguous candidates remain unknown and reset close evidence. A successful empty
query for the pinned WindowServer ID is only a close candidate. The complete
CoreGraphics snapshot must not contain that ID. Confirm the candidate with a fresh
successful CDP target snapshot while the child stays alive. No primary Codex
page in that snapshot converts the candidate to Missing. Any primary page,
including a replacement, or a CDP error remains unknown and resets close evidence.
The explicit ordered-out path stays unchanged. A confirmed Missing state must occur
twice before the existing child-exit/quiescence/burn path can run. An empty on-screen
list alone is not close evidence. Never infer close from missing on-screen metadata,
CDP disconnection, or page blur alone.
The dynamically loaded SkyLight symbols and tag bits are private macOS interfaces;
missing capabilities fail before session creation, not by prompting for permissions.
Private-window reactivation uses the official same-process window action and waits
for actual document focus. Do not invoke System Events or ask the CLI for Automation.
Normal Host activation reuses that official action and requires actual document
focus; `Page.bringToFront` success alone is not an activation acknowledgment.
Keep normal-target selection and socket-owner proof in the caller.

## Product Direction

Users launch the official Codex icon as usual. After `incodex install`, a hat-glasses control sits at the leading edge of Search's toolbar action group, before its other actions. Reuse the live official Search Button and SVG sizing/style tokens; do not pin official generated class names, asset hashes, or pixel sizes. Click or `Shift+Command+N` opens a second isolated Codex window: same login, language, and base settings; no old chats; close burns that temp home.

`incodex open` is the other launch path: spawn the official binary with an isolated home, do not copy/patch/resign the official app, then inject the **same** `inject.js` (hat-glasses + banner) through Chrome DevTools Protocol. CDP is not the macOS Dock / `install` entry; the validated Windows Store exception is defined below.

### What Incodex Should Do

- Keep the official icon and bundle id. Do not invent a second app the user launches every day.
- Isolate incognito data under `~/.incodex/sessions/`. Do not write or delete `~/.codex` session databases.
- Copy only `auth.json` and `config.toml` as files into the isolated home; a fresh private global-state file may contain only allowlisted window layout values, never chat or account state.
- Burn on a normal close. Do not claim “no traces on the machine” unless forensics say so (`absolutePrivacyClaimAllowed()` is false).
- Read-only official Runtime directories inside a proven session must not prevent burn. Prepare directory owner permissions only after the existing root/owner/ancestry checks; never chmod ordinary files or follow an external symlink. Retain failed cleanup rather than reporting success. The Electron safe-home module is a fixed startup dependency: changing it retains `restartRequired`, not a new live-reload exception.
- Default `incodex install` / `uninstall` to `/Applications/ChatGPT.app`. `--clone` and `--app` are exceptions.
- Keep `--help` and a TTY menu. Non-TTY with no args prints help. Destructive commands print a plan; TTY asks once; non-TTY requires `--yes`.
- Update Incodex-only code explicitly with `incodex runtime` (writes `~/.incodex/runtime/`, no resign); `inc update` invokes that publisher automatically through the installed CLI. After a Sparkle update, a registered default macOS install attempts automatic recovery. If recovery is unavailable or the integration remains missing, run `incodex install` on the **current** official package; `--app` and `--clone` targets retain explicit installation. An installed Windows integration may automatically rebind across an observed Store package generation change; if the new package is already running or repair evidence is uncertain, it fails closed and retains manual `incodex install` as the fallback.
- Keep `inc update` as the public stable-update entry point and the only command named in update notices. Route by the running binary's install channel: Homebrew performs bounded `brew update` (metadata refresh is best-effort) followed by bounded `brew upgrade incodex` (failure output is preserved), reports the installed version through `brew list` with the public CLI probe as fallback, resolves the new Cellar CLI through `brew --prefix incodex`, and publishes its bundled Runtime; script installs fetch the latest stable release, run its pinned installer, verify the installed binary version, retain the main-branch compatibility-installer fallback, and publish Runtime through that installed binary. A current-version script update still repairs Runtime. Clear the notice cache only after Runtime succeeds; if CLI replacement succeeded but Runtime failed, report partial success without rolling the CLI back. Source checkouts receive explicit `git pull` / `cargo install` guidance. The menu launches update-notice refresh in a detached internal worker so the next invocation can consume the cache even when the current menu exits immediately; it checks the stable release first, consults the Homebrew formula only when that release is newer, and clears stale notices when refresh cannot prove an update. Both mutable update paths use a channel-stable transaction target lock so concurrent updates cannot cross generations. Script/TTY, Homebrew, and Runtime handoff coverage lives in `crates/incodex-cli/tests/update.rs`, `update_homebrew.rs`, and `update_runtime.rs`; menu coverage lives in `crates/incodex-cli/tests/support/update_menu.rs`.

### What Incodex Should Not Do

- Do not delete, archive, or rewrite `~/.codex` session DBs.
- Do not change the bundle id or force a re-login.
- Do not restore a valid OpenAI signature after asar changes. Appshot (智能快照) is a hard triangle; document it, do not fake Team ID `2DC432GLL2`.
- Do not sign vendor CUA sidecars. Stash them, sign the host and Sparkle update framework within one explicit signing context so updater IPC shares the host identity, restore the CUA sidecars, then sign the outer app with its filtered entitlements. Legacy installs retain ad-hoc; the macOS experiment below uses a registered local certificate, never an OpenAI Team identity.
- Do not default live-patch from `install.sh` / `inc update`. `install.sh` manages only the CLI; `inc update` manages the CLI plus external Runtime and never patches or re-signs Codex.
- Do not add Overlay, an independent Session Agent, LaunchAgent auto-repair, runtime pubkeys, or Homebrew core. Own tap is `daftAI2026/homebrew-tap`; bump it from `release.yml`. Do not open a Homebrew/homebrew-core PR.
- Do not use CDP as the everyday Dock / `install` launch path on macOS. `incodex open` may start the official binary with `--remote-debugging-port` on `127.0.0.1` and inject `dist/incodex-inject.js`; the Windows Store installed path may use only the package-bound suspended-launch adapter described below. Do not clone the official app for `open`. Do not copy AGPL injector scripts.
- Do not add hidden confirmation aliases beyond the native parser's tested compatibility surface.
- Do not write tests after the implementation to match it. Write a failing repro first.

### Product Decision Filter

1. Does it belong to install, uninstall, runtime, open, status, doctor, recover, update, or self-uninstall?
2. Can a user preview it with `--dry-run` if it is destructive?
3. Does it touch `~/.codex` sessions, OpenAI signatures, or `/Applications/ChatGPT.app` without an explicit command?
4. Would “don’t do this” plus a README note be enough?

If the answer is no or unclear, decline or narrow.

The developer-only `scripts/composite-launcher-badge.swift` replaces only the capsule mask support in an already flattened Apple Icon Composer export. It never uses host alpha as a coverage gate, and does not install a launcher or publish Runtime. Full-word INCODEX artwork remains a visual design until the product generation and migration paths are accepted.

## Repository Map

- `labs/` contains non-shipped, isolated experiments. `labs/macos-ax-continuity` owns the stable-identity/AX prototype; its evidence cannot claim Codex permission persistence until the real host passes.

- `AGENTS.md` is the contract. `CLAUDE.md` must stay a symlink to it.
- `RUNTIME.md` is the Runtime hot-update architecture and acceptance contract. Keep candidate history and machine-specific receipts in private docs.
- The TypeScript product router, parser, mutation implementation, and old Runtime publishers have been retired. Rust owns the product CLI and native mutation path; legacy TypeScript v1 disk compatibility is limited to the Rust `legacy_typescript.rs` reader and `legacy_proof.rs` safety fixtures.
- `scripts/` contains build, CI-routing and explicit acceptance tools. Their visible-host actions require the corresponding run authorization; they are not the product launch path.
- `crates/incodex-cli` is the native CLI: `parse.rs` owns its command language, while `install.rs` and `open.rs` dispatch dangerous operations through the lower crates.
- `crates/incodex-transaction` owns native mutation locks, durable journals, rollback, and recovery. `crates/incodex-asar` and `crates/incodex-macos` own ASAR and macOS signing/plist mechanics.
- `crates/incodex-core/src/session.rs` owns native `open` session create/burn; it must stay behaviorally aligned with `src/runtime/incodex-safe-home.cts` without sharing language-specific code.
- `crates/incodex-cli/src/update_flow.rs` owns the shared update stages, progress cleanup, installer fallback presentation, and final result. macOS script/Homebrew and Windows managed update supply platform operations; normal output must not fork into platform-specific progress or success copy. Channel/path details belong in `update --dry-run`.
- `native/CLAUDE.md` maps committed macOS native Runtime assets and their source/manifest verification boundary.
- `runtime-artifacts.json` is the single current-generation Runtime asset catalog. `src/build-runtime.ts` builds committed `dist/` artifacts; cross-platform `incodex-runtime-assets` embeds that catalog for Rust, and native `incodex-runtime-bundle` publishes it on macOS. The installer must not rebuild Runtime by spawning Bun.
- `src/runtime/*.cts` is Electron-side source. `bun run build:runtime` emits portable `dist/*.cjs` with `__dirname`, never a machine-absolute path.
- `src/runtime/incodex-loader.cts` is the only file that belongs in official asar. Everything else loads from `~/.incodex/runtime/` after hash check and fail-opens to official main.
- `install.sh` installs the CLI binary only. It must verify `SHA256SUMS` and must not run `incodex install`.
- `docs/` is gitignored local research. Do not commit it. The Native CLI integration section below is the committed release boundary.
- `manual/` is the public bilingual Astro/Nimbus documentation site. Use `.claude/skills/public-manual/SKILL.md` for capability changes and stable-release documentation sync; keep private research out of public content.
- `.claude/skills/` is the agent skill tree. `.agents/skills/<name>` must stay a symlink to `../../.claude/skills/<name>`.

## macOS stable-signing experiment

Integrate stable signing into the existing native install/update-recovery pipeline. Synthetic identity evidence does not prove real Codex permission continuity or future update recovery.

- New macOS installs use the final signed App for a short, hidden permission-only launch before the work UI. The existing install CLI remains the guide owner. LaunchServices establishes the App launch; the probe reuses the existing nonce/PID/start protocol and exits only after the CLI completes the coherent snapshot and retires its request. Confirmed missing access opens the same native two-row card in its installation presentation context. Unknown does not open a card or mean denial. Permission-change hints only schedule a fresh one-shot probe; idle card polling never launches Electron. Record the user's terminal choice before opening the normal App once. Older verified loaders retain their existing running-App guide; do not silently re-sign or rewrite them. The new loader and native host require explicit installation migration and real-App acceptance, not a hot-update claim. Runtime updates retain their no-auto-launch contract. The isolated install chain reuses native mutation, this default post-commit dispatcher, nonce transport, cold-probe retirement and final launch ordering; OS/TCC and Presenter substitutes must be reported separately from real-App acceptance.

- Resolve the official update bridge's JavaScript through Electron's canonical `app.asar` namespace. ASAR metadata owns packed/unpacked routing; do not require a physical `app.asar.unpacked` JavaScript entry.

- `macos_signing_assets.rs` owns a per-device private identity under `~/.incodex/macos-signing/`. Only an explicit new official install creates it. The one-shot native signing store manages that private Keychain without changing the user search list or certificate trust. It is not an AX proxy, agent, or permission broker. Never publish its password, private Keychain, temporary key or PKCS12 material.
- `SigningContext` keeps local acceptance separate from ad-hoc/vendor/generic policies. A local component must match the registered certificate and its own identifier-bound DR; host and updater components share a certificate, not one host DR. Official originals remain vendor-only; CUA sidecars remain untouched.
- Native compatibility adapters reuse `native/macos/host-identity.h` for read-only host identity: legacy ad-hoc or a strictly verified host whose leaf certificate matches the existing public signing registration. Identity is not a permission grant. Remote-control keys, Storage Key access and Accessibility retain their own capability checks and authorization flows. This interface never reads private signing material or creates another identity store; full signing-Keychain health remains owned by `macos_signing_assets.rs`.
- Update registration binds the signer fingerprint to the install epoch. A missing signer field means the explicit legacy ad-hoc mode. Runtime/helper refresh does not migrate it. Recovery checks epoch/helper/signer before mutation and uses a signer-aware generation CAS; registered local identity loss or corruption fails closed, never regenerates or downgrades.
- An existing ad-hoc install still skips mutation. Migration is an explicit uninstall followed by install of the verified current official package. Its first local identity may require one user AX grant. TCC permission is per device: do not promise inheritance from ad-hoc/vendor identity or another device, or confuse signing verification with an actual AX operation.
- The experimental native permission guide reuses independent Accessibility and Screen Recording rows. Completed rows remain visible; only a fresh complete snapshot may finish the guide. The normal injected Electron main reads its own Screen Recording status, bound to the CLI request, install and process generation. Missing evidence stays unknown, including after official restoration removes Runtime. The native presenter only sends intent: explicit Accessibility Allow retains the existing at-most-once reset. Explicit Screen Recording Allow also retires ChatGPT's old ScreenCapture registration once per guide, only after a locked fresh not-granted recheck; granted or unknown never authorizes that reset, and Retry only reopens Settings. Persist Screen continuation intent before acquiring the reset lock because marker publication holds the same App lock. Neither permission row changes vendor CUA sidecars or adds an `open` precondition.
- Successful Runtime publication and official update recovery schedule one bounded permission check through the registered native CLI. Reuse the installed responder and shared two-row guide; never launch the App, reset TCC, or open Settings automatically. Both granted skips UI; confirmed missing access permits the guide; unknown alone remains pending. Keep one guide owner while humans complete Settings, with no authorization countdown; hold the existing App transaction lock only for request/result writes and explicit per-permission reset. Pin native guide assets read-only, preserve in-flight guides across publication, and revalidate install/signing identity. New Runtime hosts treat legacy AX markers as read-only migration signals and forward them only through the existing generation-bound native permission task. Never wire the legacy JS presenter/reset controller into production; the native CLI alone owns guide mutation. Bind each fresh snapshot to one PID/start identity; a normal restart invalidates that read, not the user's entire guide. This post-update check is not proof of full main-service hot activation or real permission continuity.
- Update request admission binds the installation and Runtime manifest digest. Preserve a user's Skip for the same digest; reuse the original request and continuation reason while unfinished. After a previous granted result, each successful publication admits a fresh permission check: the old marker is not current TCC evidence. Both granted skips UI, confirmed missing access opens the shared guide, and unknown remains pending. A new digest may check again after Skip. `incodex perm` is the sole explicit dual-permission reentry command; do not keep long command aliases. Reuse the bounded coordinator log for closed permission phases and worker/subject identities; never log raw request IDs, nonces or user content, or treat card-ready as human-visible acceptance.
- A native permission-task or guide behavior change must also rebuild the committed Runtime provenance and its matching CLI. Permission admission uses the selected Runtime digest; retaining an old manifest for changed native business logic would incorrectly reuse the previous generation's terminal decision.
- Permission `requestId` is a fresh UUIDv4, independent of the installation UUID. Keep the installed responder's wire contract; do not replace fixed host code to change an ephemeral request encoding. Historical 32hex markers remain readable, but a normal guide reentry creates a fresh interoperable request. Production-producer to frozen-responder tests must prove the handoff; fixture-only IDs do not.
- Permission names and icons follow the running macOS, not the build machine. Preserve the existing presentation through macOS 26. On macOS 27 and later, share one read-only system-resource adapter between the native CLI guide and compatibility Runtime. Pass the selected catalog locale explicitly; unsupported system languages use Apple's English label, and missing system resources retain bundled copy and icon fallbacks. Never rename TCC services, reset commands or Settings deep links to match a display label.
- Reuse `macos-update/coordinator.log` and the private `releases/acceptance/watch-macos-update.py` observer. The finite signing phases are `signing-identity`, `signing-start`, `signing-finished`, `recovery-committed`. Logging records only build/install ID/signing kind/public certificate fingerprints; the observer separately binds readonly signature/AX evidence to the exact host PID/executable/start time. No second monitor service or repeated deep Doctor loop. App transactions and registration CAS retain the existing separate-lock boundary: do not run `incodex runtime` concurrently with official update recovery, and do not claim an atomic commit spanning both journals/registration. A CAS refusal after app commit retains evidence for manual diagnosis.
- Experimental candidate IDs use `INCODEX-V<major>[.<minor>]-<MAC|WIN>[-<CAPABILITY>]-RC<N>`, independently of product semver. Freeze each candidate's source and payload hashes before deployment. Frozen payloads stay immutable; any payload change requires a new candidate. Keep candidate history and acceptance evidence in the private acceptance SOP, not this guide.

## Native CLI integration

The Rust CLI is the sole product CLI. Preserve the native contract suites, frozen legacy fixture reader, binary size gate, cold-start probe, and TTY/open verification. Bun builds the embedded Electron Runtime and its checks; it must not produce or launch a shipped TypeScript CLI binary.

The native Rust contract tests are the product behavior source of truth. The remaining TypeScript source is limited to Electron Runtime and forensics; legacy v1 fixture/proof material lives in Rust and must not be launched as a product CLI or used as an output oracle.

Default automated checks retain signing policy, identity storage, registration schema/CAS, ad-hoc signing and transaction regressions. Environment-dependent certificate signing is explicit local acceptance: one Runtime host-identity case and four Rust component/registration cases. Keep these tests and run the commands in `tests/README.md` when those contracts change. Passing default CI does not prove macOS certificate-signing compatibility, real TCC continuity or official-update acceptance. Do not upload user signing assets or modify trust/search lists for CI.

### Branching and TDD

1. New Rust CLI PRs target `main`. One review-sized change per PR.
2. Each behavior change starts with a failing `cargo test` repro commit, followed by the implementation commit. Do not land tests and code in the same first commit.
3. The retired TypeScript product router and mutation implementation must not return. Keep legacy v1 compatibility only in the named Rust fixture/proof modules; do not create new TypeScript parity paths.
4. Rust `install` / `uninstall` / `recover` / `open` / `status` / `doctor` are the product paths. Preserve their proven safety contracts without creating new TypeScript parity work.

### Windows adaptation boundary

- Installed UI generation diagnostics reuse `windows/installed-ui.json` (4 KiB, at most eight events). Separate the verified publication, fixed controller generation, committed UI and positively acknowledged renderer injector; include helper/App PID plus creation FILETIME, with null for unavailable evidence. Preserve uncertain rollback across later verification failure, clear unverified publication, and deduplicate the complete snapshot so incompatible B-to-C publications remain visible. A new document invalidates the previous renderer ACK; startup and reinjection require a real current-generation ACK even when the published release is unchanged. Record closed failure codes only; Runtime manifests do not prove native helper availability, so `installRequired` remains unknown. Diagnostics must not change authorization or normal App lifetime.
- Installed Windows hot updates may change the injector and the shared self-contained `incodex-main-actions.cjs` factory together; every other verified Runtime asset must remain unchanged. The stable CDP bridge adapts only native `open`, retaining owner, pending requests, request IDs, source-bound validation and original failure responses. Prepare the factory without switching requests, require the exact UI ACK, revalidate selection, then commit and acknowledge the action generation; lost ACK or superseded selection rolls back both. A request captures its action object before awaiting. Keep separate renderer/action ACK identities; no helper replacement, App exit or page reload occurs during this transaction. An older native helper still requires one explicit install after normal exit to enable the controller; unit or socket fixtures do not prove installed App acceptance.

- Windows installed injection keeps one absolute CDP readiness deadline. Do not carry that deadline policy into macOS target discovery: its existing IPv4/IPv6 and `/json/list` to `/json` fallback retain independent request budgets. The cross-platform socket fixture in `cdp_unit_tests.rs` exercises Mac fallback policy on Windows as well; this is not a substitute for macOS app acceptance.

- The installed Windows hat bridge retains a pending native `open` request and its owner/readiness channel. It must not report failure after a second arbitrary bridge timer, or report success merely because the child is alive. Only the actual `OPENED` signal resolves success; real launch/read/exit failures resolve failure. Reuse one live owner and revalidate the requesting execution context before replying. This lifecycle is isolated in `windows_installed_native_open.rs`; the shared official notification components remain unchanged by the bridge fix.

- Installed Windows action requests carry the release captured by their prepared factory. Validate each request against the startup release and verified compatible generations whose commit was attempted before enqueueing or reusing an owner; an untagged request stays bound to startup. Without the update controller, accept only startup or untagged requests. Reject an unknown release through that request's context-checked response without changing the shared owner, pending queue, or outcome. Resolve a new launch's native CLI by its validated captured release, not the latest selection.

- Experimental prearm resolves healthy staged-only targets through `GetStagedPackagePathByFullName`, not `Package.InstalledLocation`; current-user lookup preserves successful nullable WinRT results as absence without swallowing failed HRESULTs. Before resuming an exact verified installed primary, prepare the official bundled CUA runtime in its current-user content-addressed cache so local disk preparation does not consume the official organization-policy RPC deadline. Derive the observed official cache key from bundled content, validate the manifest and copied files, and atomically publish only new generations; never overwrite/delete official cache generations or Store assets. Cache preparation failure is logged and still resumes the official App. This does not skip policy reads, request elevation, change macOS, or prove a future Store-update lifecycle.

- The experimental login observer must retain its subscription after a failed reconciliation: uncertain package health blocks mutation, not future observation. Wait for official-family update completion, install completion or package-status events, then revalidate authorization and all package evidence from scratch. Explicit authorization retirement and cancellation still exit; subscription/wait failures remain fatal. Record `reconciliation-deferred` rather than `watching` when repair did not succeed. Do not turn transient login registration into permanent observer exit, bypass package validation, or add periodic scanning.

- Installed normal-App readiness allows up to ten minutes for post-update initialization, using serial one-second retries and one absolute CDP deadline. Once the official primary thread has resumed successfully, injection or bridge failure must never terminate that normal App. Pre-resume preparation and failed-resume cleanup remain unchanged, as do incognito Job/privacy failures. Installed UI diagnostics reuse the private bounded logger in one separate `windows/installed-ui.json` file (4 KiB, eight events); record phase transitions only, not retry polls, DOM or account data. The existing observer file retains its independent 64 KiB/128-event cap. Lifecycle phase writes retain the latest Runtime generation evidence only when the content-addressed helper identity and both helper/App PID plus creation time match; unknown or changed owners never inherit it. Final selection-read errors clear candidate selection evidence before independently rolling UI and business actions back. A retained compatible Runtime resolves native open through its own validated versioned managed CLI, even after the current CLI pointer advances; the existing same-version helper shortcut and prepublication generation checks remain. Warm-start and simulated-delay tests do not prove a new real Store update lifecycle.

- Experimental Store update repair must establish its PackageCatalog subscription before the official installed launch proceeds. Run the coordinator concurrently with the installed CDP bridge inside the same helper, cancel on route failure, and join it on normal exit so pending update repair can finish. A live helper or a hat alone does not prove subscription readiness. Lifecycle regression tests live in `windows_update_repair_lifecycle.rs`; this experiment is not a stable automatic-update promise.

- Both Windows update subscribers acquire a scoped PackageCatalog activation factory through `open_current_user_package_catalog`; do not retain the projection's static factory cache across WinRT apartment teardown. The native subscription regression test repeats initialization, subscription and teardown in one process to catch stale-factory access violations.

- On observer startup, an unchanged package generation still requires reapplying the authorized system debugger registration after exact package processes exit normally. Durable state proves authorization, not that the activation hook survived login. Reuse the existing registration API under the install gate, validate the recorded helper/Runtime/evidence, preserve Runtime selection and state identity, and log rearming/registration-reapplied separately from watching. Check cancellation immediately before rearming and recheck package processes afterward; a concurrent launch returns to normal-exit waiting without revoking the still-authorized registration. This is event-driven startup reconciliation, not polling or proof of visible UI acceptance.

- Automatic Windows rebind preserves the Runtime release selected by the authorized install state or retained repair intent. Validate that release before retiring old registration state, including retry; reject retry if the retained intent predates a changed Runtime selection. Do not publish the old helper's embedded Runtime or rewrite `runtime/current.json`. Explicit `install` remains the publishing path. Regression tests cover corrupt selected assets, newer Runtime versions, equal-version/different-hash releases, and stale retry selections.

- This experimental branch additionally registers a current-user HKCU Run entry for the same content-addressed native helper during explicit Windows install. The observer subscribes to package-family events before startup reconciliation, detects missed offline generation changes, and reuses existing repair/resume transactions. It waits on kernel events and exact package process handles without periodic scanning; it must run outside an inherited Job and has a single owner per user/session. Recheck the owned login entry under the install lock before mutation so uninstall cancels queued startup as well as a live observer. Explicit uninstall and self-uninstall with app restoration remove the owned login entry and signal cancellation; automatic rebind does not remove it. Run startup may be delayed by Windows: no synchronous first-icon guarantee and no real-update claim without same-candidate evidence. Diagnostics at `windows/update-observer.json` retain latest state and at most 128 events within 64 KiB, atomically replaced with private ACLs; details are capped at 512 characters and consecutive identical states are suppressed. Record subscription, successful package-event wakeup, waiting, repair and exit/error transitions, never polling or account/DOM data. Product version and content-addressed helper identity correlate with the private experimental manifest; logs are not proof of successful UI injection.

- Windows support is under development and is not a public product claim until the corresponding behavior has passed real Windows app and lifecycle verification.
- The Windows Rust boundary exposes parsing, help, version reporting, native `open`, `install` / `uninstall`, `runtime`, managed `update`, and managed `self-uninstall`. Every other mutating product command fails closed before creating Incodex state until that command lands behind its own failing Windows test.
- Windows CI intentionally tests the supported `incodex-core` and `incodex-cli` library/binary surface through `bun run test:windows:rust`. The ASAR mutation, transaction, Runtime publishing, and macOS integration crates remain outside that target until a real Windows responsibility exists; on Windows, do not run `cargo test --workspace` or add placeholder implementations merely to make it compile there.
- Windows `open` is one trust pipeline: discover the current user's `OpenAI.Codex` Store package without hardcoded install paths; create a current-user-only session that rejects reparse ancestry, copies only `auth.json` / `config.toml` and projects allowlisted zoom/sidebar/window geometry into a fresh private state file, and persists directory plus owner-process identity; launch suspended into a kill-on-close Job Object; prove the IPv4 loopback CDP listener belongs to that Job; inject the committed shared Runtime; then burn normal sessions or sweep proven-dead owners while retaining every uncertain cleanup state. The installed hat bridge forwards live source-window bounds to native `open`; native `open` otherwise falls back to the last persisted official bounds.
- Installed Windows Runtime reactivation uses the existing Node byte-stream named pipe. The native client must preserve newline framing and one bounded deadline across connection, writes, and fragmented replies; `CallNamedPipeW` cannot communicate with this byte-type server. Real pipe regression coverage lives in `crates/incodex-cli/tests/windows_runtime_raise.rs`.
- Windows uninstall cancels a prearmed Store update only after proving its target has no running processes. Query that exact package generation before disabling its debugger: a proven-absent package permits retiring the pending intent without an OS disable call; an unknown package query or failed disable retains the intent and fails closed. Do not delete pending state to bypass recovery.
- Windows `install` publishes the same Electron Runtime plus a content-addressed native helper under the current user's private Incodex root, registers a durable package debugger bootstrap without modifying the Store package, and keeps every install transition behind an epoch-checked kill switch. The debugger validates the exact Store package PID and suspended primary thread, appends random IPv4-loopback CDP switches inside the existing x64 process-parameter buffer, resumes the process, proves the listener still belongs to that package, and injects the shared Runtime into the exact primary page; the main-window bridge exposes only the managed `open` action. Install and uninstall never request or force the official package to exit: the current official Windows app bypasses its task-confirmation prompt on quit, so mutation fails closed while any package process is running and tells the user to finish active work, then quit with `Ctrl+Q` or the tray command. The installed hat-glasses path reuses the shared injector, menu, colors, and window hooks; only suspended launch preparation, bridge transport, session creation, Job ownership, readiness, and cleanup cross Windows adapters.
- On each Windows installed primary launch, the content-addressed debugger helper also runs its existing App-scoped, event-driven update coordinator; it does not spawn a child that would inherit the App Job. The helper first proves it is outside that Job, then blocks on current-user `PackageCatalog` events and exact process handles instead of polling. After a successful `OpenAI.Codex` generation change, an updater-auto-relaunched target is left running until the user exits normally; the helper wakes from the target process handles, rediscovers the same trusted generation, and invokes the existing epoch/registration-checked install transaction. This App-scoped path and the experimental login observer share the installation gate and authorization checks; neither is a service, scheduled task, DOM update-button hook, or second installer. Unrelated packages, stale authorization, failed updates, changed generations, and uncertain identity all fail closed without closing Codex; interrupted replacement retains a private intent that retry or a later explicit install can consume.
- Windows `self-uninstall` accepts only the PowerShell installer's proven current generation. It reuses the managed identity checks, the stable installer lock outside the deleted package tree, and the legacy generation lock during migration; the hidden system PowerShell handoff seals both launchers before acknowledging readiness, then rescans until every managed CLI has exited. Default removal is limited to the two launchers, `packages/standalone`, and the exact CLI bin entry in the current user's PATH; Runtime and session state remain. `--restore-app` first reuses the approved Windows Runtime uninstall and still refuses while Codex is running.
- Preserve the shared parser and command names. Add Windows behavior inside the Rust product CLI, one review-sized capability at a time; do not create a second CLI or Runtime.
- Treat Store/AppX installation, Authenticode, reparse points, ACLs, process trees, and updater behavior as evidence-driven Windows boundaries. Any expansion of Windows package mutation or persistent debugger behavior requires separate repository-owner approval and a failing Windows test first.

The experimental Windows update observer is configured by the normal `install` path, not by an agent's post-install hook command. It may prearm only the exact Store-signed healthy target supplied by a current-user package event, with a separate private pending intent bound to the installed source epoch, registration, helper hash, and selected Runtime. That intent reserves the target registration ID before configuring its launch environment; the authorized replacement transaction commits the same ID. Only the exact still-suspended ordinary primary launch may consume that intent through the existing install transaction; other running package processes are never exempted. Failed preparation resumes the official launch and retains the normal-exit recovery path. Explicit uninstall cancels the pending target hook. Windows CLI/helper executables declare `asInvoker` with `uiAccess=false`; no product path requests UAC elevation. First-launch recovery across a real Store update remains an experimental acceptance gate, not a public guarantee.

### Runtime boundary

- Native CDP launch readiness requires the injected action control and the requested unique profile mask, after official-mode and process/listener ownership checks. Banner presence remains a separately reported presentation state; a missing Banner must not withhold the real `OPENED` handshake or terminate an otherwise ready private window. Renderer `UiProbeSnapshot.accepted` describes complete presentation, not native launch success. Early module-source hints are optional: official callable proxies may reject source inspection, which must not block final Search-fiber component ownership validation.
- Privacy Banner placement prefers the unique separate official home banner slot. When it is absent, the unique banner-aware empty container directly owned by `[data-codex-composer-root][data-composer-placement="home"]` is the supported fallback; conversation composer containers and ambiguous slots remain excluded.

- Notification discovery selects bounded function headers near the official `attachedToComposer` capability and verifies the exported Banner prop set. Do not search a consolidated chunk once per export or persist a discovered component cache. Both platforms still render the same current official component. Incodex's new-window cascade is 22 logical points on both platforms by product decision, with live source dimensions; Windows keeps its existing work-area constraints.

- Electron Runtime stays TypeScript (`src/runtime/*.cts` → `dist/*.cjs`) and is still built by Bun; Rust embeds committed `dist/` artifacts.
- Release probes cap the raw sum of `runtime-artifacts.json.external` JS/JSON files at 1 MiB (1,048,576 bytes). This budget excludes the ASAR loader, manifests and native assets; it is not a limit on the whole published Runtime tree. Keep the Bun and Rust checks aligned. Native CLI gates remain 10 MiB raw, 5 MiB gzip and the existing 50 ms local cold-start target.
- Profile masking recognizes only the unique identity in the sidebar's `aria-controls`-linked account menu. Preserve both the legacy direct name/avatar layout and the observed nested layout (name in a flex column, avatar in an extra span); use the same selectors for discovery, masking, and health checks. Do not mask the plan subtitle or unrelated menu actions, or disable fail-closed health checks to accommodate official UI changes. Regression coverage lives in `src/runtime/inject.test.ts`.
- The official navigation rail may expose an avatar-only account trigger inside its `sidebar-item` footer. Mask that unique trigger's observed nested avatar and the identity in its `aria-controls`-linked menu; do not synthesize a sidebar name. An expanded trigger without a recognized linked identity, duplicate candidates, or an unrecognized surviving trigger still fails closed. Repeated injector bootstrap must preserve the sidebar React root and its activation callback while readiness is pending. Regression coverage lives in `src/runtime/incognito-profile-rail.test.ts`.
- Native profile-health polling must synchronously repair the current identity before validating it; background Electron windows may suspend `requestAnimationFrame`, so the renderer's scheduled repaint is not a prerequisite for a healthy native poll. Failed repair still fails closed.
- Profile masking is scoped to the main sidebar identity and its first-level account menu, not the full settings pages. Only a positively recognized full-settings navigation surface or its empty busy loading skeleton may omit the identity without a mask failure; unknown/changed sidebar markup must still fail closed. Re-discover and repair it when it remounts; multiple matching identities or a present identity that cannot be masked still fail validation. Do not inspect or rewrite account details inside settings.
- Settings-loading recognition follows sidebar ownership, not a fixed DOM depth: the official Windows shell wraps its busy navigation in layout containers. Preserve the empty-placeholder and identity checks; selector regressions must exercise actual HTML matching, not only mocked query results.
- Incognito-window hover (hat-glasses → circle-x) is Runtime, not Rust UI code.
- `open` uses the official binary plus an isolated Chromium/CODEX_HOME pair and localhost CDP injection. It must not patch ASAR, clone, or re-sign the app.
- Do not add ratatui, cursive, crossterm, or an AGPL ASAR crate. The native menu is the existing numbered/arrow UI implemented directly with termios.

### Release cutover

1. `release.yml` builds Rust into the stable `incodex-darwin-arm64`, `incodex-darwin-x64`, and `incodex-windows-x64.exe` names on native macOS/Windows runners, then publishes only those binaries and `SHA256SUMS`. The Windows asset is not a public support claim until its stable installer, updater, and real app lifecycle verification complete in the same first-release milestone.
2. `install.sh` and the own Homebrew tap continue selecting only the stable Darwin names; do not add a Rust/Bun selector or fallback. Windows distribution remains behind the Windows first-release gate.
3. New releases do not publish legacy Bun CLI assets. Never delete old Release assets that remain useful for rollback.
4. Merging release-pipeline code is not permission to create a tag or GitHub Release. Obtain explicit repository-owner confirmation immediately before publishing. Do not open a Homebrew/homebrew-core PR.

### Rust commands

```bash
cargo run -p incodex-cli -- --help
cargo run -p incodex-cli -- open --dry-run
cargo test --workspace --release
```

Do not add ratatui, cursive, crossterm, or an AGPL asar crate. The native menu is the existing numbered/arrow UI implemented directly with termios.

## Commands

```bash
bun install --frozen-lockfile
bun run check
bun run test:shared
bun run test:macos
bun run test:windows
bun run test:windows:rust
bun run build:runtime
INCODEX_DEV_HOT=1 bun run deploy:runtime
cargo test --workspace --release
```

Public docs use the native `incodex` / `inc` binaries. Bun is retained for Electron Runtime build/checks, not as a CLI entry point.
`bun run check` dispatches the shared suite plus the current host suite; CI keeps the macOS and Windows platform suites explicit.

## Critical Safety Rules

- Official install/uninstall default to `/Applications/ChatGPT.app`. That is intentional. Confirm on TTY; require `--yes` without a TTY.
- Never write a second installer or restore a TypeScript router around the native CLI.
- Only sign what must be signed. Leave official CUA sidecars official.
- Electron builds that bind `ElectronAsarIntegrity` to a Framework digest need both metadata and digest updated in the staged transaction. Use `sign_app_with_asar_integrity`: validate all original nested signatures and the old digest before mutation, re-sign the unique digest-bearing Framework plus the existing host/Sparkle scope. A hardened descendant Electron helper whose verified CodeDirectory identifier matches its plist identity and is in the host's `.helper` namespace or the exact host-derived Framework `.AlertNotificationService` identity and whose Mach-O directly loads that modified Framework or whose verified classic `ChromeMain`/`dlopen` loader resolves to that exact binary also needs its own filtered entitlements and disabled library validation if it did not already have that exemption; sign these helpers before the Framework. Preserve all other vendor children and all CUA sidecars; a dependent outside that identity boundary fails closed. Never disable the integrity fuse or digest `used` bit to make a patched app launch.
- Do not enable required GitHub reviews. Do not force-push `main`.
- Pin GitHub Actions to a 40-character commit SHA with a version comment: `uses: owner/repo@<sha> # vX.Y.Z`. Do not leave floating `@v4` tags.
- Official CLI packages are git tags `vX.Y.Z`. Follow `.claude/skills/release-flow/SKILL.md`, then `.claude/skills/release-notes/SKILL.md`. Do not `gh release create` and do not turn `generate_release_notes` back on.
- Route Electron Runtime session create/burn through `src/runtime/incodex-safe-home.cts` and native CLI session create/burn through `crates/incodex-core/src/session.rs`. Keep their safety contract aligned; do not fork a third implementation.
- `incodex open` must not patch asar, resign, or clone the official app. It spawns the official binary with `--user-data-dir`, `CODEX_HOME`, `CODEX_ELECTRON_USER_DATA_PATH`, and a localhost `--remote-debugging-port`, then injects the shared `inject.js`.
- A second instance needs that Chromium user-data-dir pair. `CODEX_HOME` alone is swallowed by SingletonLock.

## Working Rules

- CLI/Runtime/helper changes must satisfy the cross-platform update-handoff contract in `RUNTIME.md`: validate the actual registered consumer and its selected executable, not only publication or equal version strings. Windows Runtime synchronization must also hand off same-registration prearm/repair intents under the install gate, including current-version repair; retain target/operation identity and strict helper authorization. Keep immutable/authorized helpers when compatible; report native migration and unverified platform acceptance separately.
- Tests first. Add a failing test that states the bug or contract, then implement until it passes. Do not write tests to match already-written code.
- Keep `install.ps1` ASCII-only, including comments: PowerShell 5.1's ANSI file decoding can swallow commands after non-ASCII comments, while a UTF-8 BOM breaks the documented `irm | iex` entry. Test public LF delivery and expression execution, not just a Windows CRLF checkout.
- If you touch `src/runtime` or `src/build-runtime.ts`, run `bun run build:runtime` and commit matching `dist/` files.
- Keep Chinese and English user-facing copy in `incognito-copy.ts` together.
- Update affected public manual pages in both languages when product behavior changes. Stable releases also require a matching site release entry and deployed guide update through the `public-manual` skill; do not advertise unreleased behavior as stable.
- Mainline product work uses review-sized PRs targeting `main`. Experimental work follows the branch workflow below; do not open or merge partial experimental PRs.
- Do not add AI attribution trailers to commits.

## Local macOS auto-restore integration

Keep this experiment in its existing worktree and branch. Add implementation and acceptance fixes through direct commits and normal pushes to that branch. Do not split the experiment into small PRs. After the owner confirms that the whole experiment is ready, open one aggregate PR targeting `main`; green tests alone do not move that decision forward. Keep failing-repro and implementation commits separate within the experiment.

Use the latest main as the integration base. Preserve the native Coordinator, Keychain provider/helper, signing implementation, and `incodex-macos-update.cjs` catalog entry. Do not deploy a main-only Runtime over this experiment. Keep synchronization history, source hashes, and deployment results in private reports, not this guide.

Mode readiness uses main's official-blocker/technical-failure/active-time model, not the retired parallel total-check deadline. The experimental renderer fallback remains asynchronous and deadline-bounded; its confirmation allowance and the native allowance remain 20 polls. Window owner gating must retain actual host show intent while never promoting a never-shown prewarm window. Regression coverage is in `src/runtime/experiment-integration.test.ts`, `src/runtime-cleanup-owner.test.ts`, and `crates/incodex-cli/src/cdp_mode_tests.rs`.

The macOS recovery module uses the manifest-verified sibling loader, so existing installed Loaders cannot execute an unchecked new dependency. Provider injection keeps the original host ASAR dictionary until `sign_staged_app_with_asar_integrity` updates both metadata and the Framework digest; provider-bearing Framework helpers still join the experimental ad-hoc generation while external vendor/CUA sidecars remain preserved. Synthetic real-toolchain coverage is in `crates/incodex-macos/tests/keychain_integrity_signing.rs`.

A running process pins its verified immutable Runtime. The current hot-update controller switches the shared injector and self-contained `main-actions` factory after current-selection checks and real acknowledgements; all other verified assets must remain unchanged. The permission responder remains in the startup generation, so the CLI must speak its installed wire protocol. A new invocation of the short-lived CLI permission guide may use newly verified assets without replacing that responder. During per-window staging, the first acknowledged candidate window still calls old actions until every target acknowledges. This is not a globally atomic UI/action swap. Unmigrated assets remain explicit compatibility boundaries, not reasons to exit the App for a Runtime update. An older process cannot acquire a missing interface merely because `current.json` advances. A same-content generation ACK is publication evidence, not proof of functional hot replacement. Follow `RUNTIME.md` and report these distinctions.

Runtime-only synchronization retains the installed loader. The external main therefore repeats the exact native-open ownership bypass, allowing an older experimental loader to use the new native CLI without attaching a second Runtime owner; ordinary installed windows still arm update handoff. Regression coverage lives in `src/runtime-main-injection.test.ts`.

A successful source integration is not proof of the next real Sparkle update or permission continuity. Preserve the installed experimental Runtime until a separately validated deployment; do not patch/resign the official App to validate a merge.

### Experimental integration navigation

Rust 2021 + Bun + Electron TypeScript + macOS native helpers. Read exact versions from `Cargo.toml`, `package.json`, `.bun-version`, and `rust-toolchain.toml`.

- `crates/incodex-cli/CLAUDE.md`: native product commands and experimental update/Keychain adapters.
- `crates/incodex-macos/CLAUDE.md`: signing/integrity/Mach-O platform boundary and synthetic native regression fixtures.
- `crates/incodex-runtime-bundle/CLAUDE.md`: immutable Runtime publisher and non-blocking official bootstrap contracts.
- `crates/incodex-runtime-assets/CLAUDE.md`: cross-platform committed-asset catalog and compile-time embedding boundary.
- `src/CLAUDE.md` → `src/runtime/CLAUDE.md`: build/forensics versus Electron execution ownership.
- `tests/CLAUDE.md`: repository and publication safety contracts.
- `manual/CLAUDE.md`: independent bilingual public guide; read its toolchain from `manual/package.json`, not host Runtime dependencies.

`runtime-artifacts.json` remains the single asset catalog; `Cargo.toml` and `package.json` own exact versions.
