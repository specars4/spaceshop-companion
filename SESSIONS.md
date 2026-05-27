# Spaceshop Companion — Session log

Chronological record of build sessions on the Companion app. Companion
lives in its own repo (this one) and has its own session/backlog cadence
— separate from the larger SPACESHOP TOOLS project because it ships
independently to contractors as a `.msi` and has its own release cycle.

Cross-link to the SPACESHOP TOOLS world only happens at the **invite-format
contract** (`docs/INVITE_FORMAT.md` in both repos; canonical source is
SPACESHOP TOOLS) and the **bundled `p4.exe`** (sourced from `tools/perforce/bin/`
in SPACESHOP TOOLS).

Most-recent first.

---

## Session 10 — 2026-05-26 — v0.6.7 (parallel sync — fixes contractor onboarding WSAECONNRESET)

**Outcome:** Companion's `p4 sync` now runs with `--parallel=threads=4`
batched into 8 MB chunks. First adversarial-agent-caught release in
this session: a broken `sync -k` "resume" implementation was shipped
to the audit and rejected before publish; replaced with the correct
parallel-sync fix that addresses the actual root cause.

### What happened

Shakira (first real contractor onboarding through v0.6.5) hit
`WSAECONNRESET, An existing connection was forcibly closed by the
remote host` 25 % through her 8010-file project sync (file 2038
of 8010). Server-side investigation showed:

- Her workspace was correctly created on the server.
- Her local disk had ~2038 partial files at `C:\Vraylar`.
- Her server-side have-table was **empty** — the partial-sync
  transactions never committed when the TCP reset hit.
- TRY AGAIN would have called `force_resync` which empties the
  have-table and re-downloads from scratch — a 12 GB re-transfer
  giving Tailscale's relay another chance to time out mid-stream.

### Root cause (agent's diagnosis)

Tailscale-relayed TCP between Shakira's machine and the NAS
(no direct P2P NAT-traversal possible) routes through a DERP
relay with a multi-minute idle-TCP timeout. Companion's pre-v0.6.7
sync ran as a single long-lived stream against the full 48 GB
workspace; on a slow link or with any per-file pause, the relay
killed the connection.

### The fix (v0.6.7)

Server-side: `net.parallel.max=8` set via `p4 configure set` so
clients are allowed to request parallel sync.

Client-side: new `PARALLEL_SYNC_FLAG` constant applied to all three
streaming-sync paths (`sync_workspace`, `force_resync_inner`,
`repair_workspace` step 2):

  `--parallel=threads=4,batch=16,batchsize=8388608,min=1,minsize=1048576`

This splits the sync into 4 concurrent TCP streams, each handling
8 MB batches at a time. Each batch is short enough to clear the
DERP idle timeout, and successful batches commit their have-table
entries incrementally — so a single batch failing is a small,
cheap-to-retry loss instead of a full re-download.

### What got dropped before publish (the adversarial-agent save)

Initial v0.6.7 draft had a `resume_partial_sync` function that
ran `p4 sync -k //...` (to "repopulate the have-table from on-disk
files") followed by `sync //...` (resume). The patch even had a
multi-paragraph docstring confidently explaining the safety
argument. The adversarial-audit agent caught that **`sync -k` does
NOT verify on-disk files** — it unconditionally writes have-table
entries for the requested revspec. Result on Shakira's box would
have been: have-table claims 8010 @ HEAD, second `sync //...`
reports "file(s) up-to-date" and transfers nothing, contractor
ends silently with 25 % of files on disk + a server view saying
she's whole. Silent corruption, worse than the original bug.

Replaced entirely with the parallel-sync approach above —
addresses the root cause (DERP timeout) instead of working around
the symptom (failed sync needs resume).

Raw-byte form is mandatory: this server's p4d rejected the
`8M` / `8m` shorthand with `Usage: threads=N,batch=N,batchsize=N,...`
even though that's the format in some Perforce docs.

---

## Session 9 — 2026-05-26 — v0.6.6 (polish: close-to-tray notification, friendly p4-timeout, client -i timeout bump, error scrubs)

**Outcome:** Three operator-flagged UX nuisances + three audit findings
landed in one polish release. Notable: the headline "friendly p4
timeout" fix was caught BROKEN by three adversarial agents before
publish — the pattern didn't match the actual emitted error string.
Fix shipped only after that was corrected.

### v0.6.6 changes

- **`lib.rs::FIRST_CLOSE_NOTIFIED`** — OS notification on first
  process-lifetime window close, explaining the close-to-tray
  behavior. Uses `compare_exchange` + rollback on `show()` failure
  so a Focus-Assist-suppressed notification doesn't consume the
  one-shot gate. `Relaxed` ordering (was `AcqRel` pre-audit).

- **`errors.rs::p4_friendly`** — new p4-variant-specific friendly
  mapping. Matches `"timed out after"` → "Perforce server is slow"
  + concrete retry advice (UE Editor, other p4 apps). Matches
  `"can't be locked"` / `"already locked by"` → "File is locked by
  someone else." Falls through to existing `translate()` for
  AUTH_PHRASES, CONNECT_PHRASES, etc.

- **`errors.rs::UPDATE_NETWORK_PHRASES`** — dropped the bare
  `"timed out"` phrase. It over-matched on Perforce timeouts and was
  the original cause of the "Couldn't reach update server"
  mislabel. The more-specific `"connection timed out"` is still
  in the list and covers real network timeouts.

- **`perforce.rs::run_p4`** — (a) emits the actual elapsed budget in
  timeout errors (`"p4 {args} timed out after Ns"`) so future
  classifiers don't have to grep for variant prefixes; (b) sets
  `kill_on_drop(true)` on the spawned Command so a force-quit or
  task cancellation kills the p4 child instead of orphaning it.

- **`perforce.rs::CLIENT_SPEC_TIMEOUT = 120s`** — used by
  `create_workspace` for `p4 client -i`. The 30-second
  `CALL_TIMEOUT` was too tight under workspace lock contention
  (Workshop pre-Session-6 polling could block `client -i` for 60+ s).
  v0.6.3's polling fix should make contention rare; this is defense
  in depth.

- **`src/pages/Confirm.tsx`** — adopt-in-place "30 seconds, no
  download" copy now says "usually 30 seconds, up to 2 minutes if
  the server is busy." Truth-in-advertising for the 120s
  `CLIENT_SPEC_TIMEOUT` worst case.

### Three adversarial-agent findings caught BEFORE publish

The author wrote v0.6.6's friendly-timeout pattern (`"timed out
after"`) without sanity-checking the actual emitted string. `run_p4`
emitted `"p4 {args} timed out"` with no `"after"` suffix — the
pattern would never match, and the bug v0.6.6 claimed to fix would
have shipped unfixed. Three parallel adversarial agents ALL caught
it independently. Fix: changed `run_p4` to emit the suffix AND
removed the bare `"timed out"` from `UPDATE_NETWORK_PHRASES` as
defense in depth.

Two other agent-flagged P2s also landed in this release:
- `"locked by"` substring was too broad (would match benign p4 log
  text containing those two words) — tightened to `"can't be
  locked"` and `"already locked by"`.
- `swap()` on the notification gate consumed the one-shot even when
  `show()` failed — switched to `compare_exchange` + rollback so
  Focus-Assist users get the notification next close instead of
  losing it forever.

---

## Session 8 — 2026-05-26 — v0.6.5 (Scan for new/changed files button)

**Outcome:** Companion's Changes (Review & Submit) page now has a
"Scan for files" button that runs `p4 reconcile //...` on demand,
opening any disk file that differs from depot for the appropriate
action (add/edit/delete). Closes the v0.6.4 caveat: files dragged
into the workspace folder via Windows Explorer (e.g. dropped into
`sharedassets/`) weren't visible in Review & Submit because they
weren't in `p4 opened`. With this button the operator can scan,
let `p4 reconcile` open everything, then re-render the change list.

### v0.6.5 changes

- **`perforce.rs::reconcile_workspace`** — new public fn. Runs
  `p4 -c <ws> reconcile //...` (NOT `-n`, this actually opens files)
  with `ADOPT_RECONCILE_TIMEOUT = 10 min`. Returns the count of
  files opened. Handles the benign "no file(s) to reconcile" case
  (exit 1 + stderr message) as `Ok(0)` rather than error.
- **`perforce.rs::p4_reconcile_workspace`** — Tauri command wrapper.
- **`lib.rs`** — registered the new command.
- **`src/lib/invoke.ts::reconcileWorkspace`** — TS wrapper.
- **`src/pages/Changes.tsx`** — added a top-of-page "Missing a file
  you added outside Unreal?" call-to-action card with a "Scan for
  files" button. Separate `scanning` state from `loading` so the
  existing change list stays visible while the 30-60s scan runs.
  On success: surface "Opened N file(s)…" or "No new or changed
  files found…" in a gold-bordered notice, then re-fetch
  `listChanges` so the new files appear in the list.

### UX note

The button is intentionally framed as opt-in ("opt in to the slow
scan when you suspect Unreal missed something") rather than always-on.
A passive every-time reconcile on page load would have made the
Review & Submit page take 30-60s to render — worse UX than the
v0.6.4 "fast but blind to unmanaged files" trade-off.

---

## Session 7 — 2026-05-26 — v0.6.4 (list_changes uses `p4 opened`, fixes Review-and-Submit "no local changes" lie)

**Outcome:** Companion's "Review & Submit" page now actually shows
pending changes from the workspace. Reproducible bug it kills:
operator had one file opened for add (`NewMaterial.uasset`) in the
default changelist, opened Companion → Review & Submit, and the page
showed grayed-out "no local changes" — wrong, the file was clearly
in `p4 opened`. Forced operator to submit via Unreal's source-control
panel instead.

### Root cause

`perforce.rs::list_changes` (the function the Review & Submit page
calls) ran `p4 -c <ws> status` with the 30-second `CALL_TIMEOUT`.
Measured time on the operator's real VRAYLAR_Neymarc workspace:
**66 seconds.** So `list_changes` errored out at 30 s, the error
collapsed to an empty `Ok(Vec::new())` (the catch-all for "not
opened on this client" was matching too aggressively in the error
path), and the UI surfaced that as "no local changes."

### v0.6.4 changes

- **`perforce.rs::list_changes`** — full rewrite to use
  `p4 -ztag -c <ws> opened` instead of `p4 status`. Parses tagged
  records (depotFile / clientFile / action / etc.) directly, maps
  action verbs (`edit`/`add`/`branch`/`move/add`/`delete`/`move/delete`/
  `integrate`) to the M/A/D status codes the frontend expects.
  Sub-100 ms instead of 60+ seconds. Caveat: `p4 opened` doesn't
  catch unmanaged disk changes (files modified without `p4 edit`
  first) — that's the right trade since Unreal auto-runs `p4 edit`
  on checkout and `p4 add` on new assets. A future "Scan for
  unmanaged files" button (running `p4 reconcile -n` on demand)
  is the right home for the rarer case.

### Forensic note

This bug was masked by Session 6's tray-poll fix. Pre-v0.6.3, the
tray-poll lock storm + Companion timing-out made every Companion
view feel broken — the "Review & Submit" Failure was indistinguishable
from a hundred other things. Once v0.6.3 calmed the polls, the
specific `list_changes` timeout surfaced as a clean reproducible bug.
Worth noting because the bug had been present since `list_changes`
was first written — the operator just hadn't been able to isolate it
until the noisier issues cleared.

---

## Session 6 — 2026-05-26 — v0.6.3 (tray-poll stacking fix + cheap-query rewrite)

**Outcome:** Companion's 30-second tray-poll loop no longer stacks
concurrent `p4 status` invocations that lock out Unreal's source-control
operations. Catastrophic failure path it fixes: with v0.6.2 Companion
running in the tray, a single `p4 edit` on a `.uasset` in Unreal Editor
took **87 seconds** because three Companion `p4 status` polls were
queued behind it on the workspace's db.locks. Terminal `p4 edit`
measured at 117 s under the same conditions. After killing Companion's
stacked polls, the same `p4 edit` dropped to **0.05 s**.

### v0.6.3 changes

- **`tray_poll.rs::run_poll_cycle`** — new RAII-guarded `AtomicBool`
  stacking guard on `TrayPollState`. If a poll cycle hasn't finished
  when the next 30 s tick fires, the new cycle is SKIPPED entirely
  (rather than running concurrently, which is what produced the
  6-process pile-up). The flag clears via `Drop` on a sentinel struct,
  so a panic or early-return inside `compute_poll_result` can't leave
  it stuck `true`. A real-time tray that misses one tick is fine; a
  Companion that locks out Unreal is not.
- **`perforce.rs::change_counts`** — full rewrite to cheap metadata
  queries. Was: `p4 status` (52 s — 4 min full workspace walk) +
  `p4 sync -n //...` (another full walk). Now: `p4 opened -c <ws>`
  (~40 ms, returns the local-pending count directly from db.working)
  + `p4 changes -m 1 //<view>/...` (depot HEAD) + `p4 changes -m 1
  //<view>/...#have` (have-table HEAD). The remote_unseen output is
  now boolean (1 = remote has newer CL, 0 = up-to-date) rather than
  an exact pending-file count — precise counts cost a full sync-n
  walk and aren't worth it for the tray's green/yellow distinction.
  Total poll cost drops from minutes to <100 ms.
- **`p4_typemap.py`** (SPACESHOP TOOLS side) — dropped `+l` from
  `.psd/.tga/.tif/.tiff`. The exclusive-lock-on-every-checkout is
  necessary for `.uasset/.umap` (Unreal binary cooker can't merge)
  but is overkill for source textures (rare concurrent edits, and
  the lock storm compounds with the workspace lock contention on
  Docker-on-NAS p4d).
- Version bumps to 0.6.3 across `Cargo.toml` / `tauri.conf.json` /
  `package.json`.

**Diagnostic record.** Three parallel agents converged on the same
root cause in independent sweeps: network was fine (<1 ms RTT, no
data flow during hangs), server was healthy (no configurables
tweaked, 151 h uptime), workspace was clean. The bottleneck was
purely Companion's polling — 6 stacked `p4 status` processes from
PID 51404 at peak, oldest 273 s elapsed.

---

## Session 5 — 2026-05-26 — v0.6.2 (adopt-in-place self-onboard)

**Outcome:** Workshop's SELF-ONBOARD INVITE flow now adopts an existing
on-disk workspace instead of flushing the have-table and re-downloading.
The catastrophic case it prevents: arsen migrates a 48 GB Unreal project
via Workshop's MIGRATE TO PERFORCE, hits SELF-ONBOARD INVITE, pastes the
code into Companion — and instead of ~70 min of needless re-transfer at
the 100 Mbps line ceiling, the workspace is registered in ~30 s with the
local-vs-depot diff surfaced as the sync step's detail string.

### v0.6.2 changes

- **`src-tauri/src/commands/invite.rs`** — `InviteData` gains
  `adopt_existing: Option<bool>` + `adopt_at_cl: Option<u32>`, both
  `#[serde(default)]`. Additive on the v=1 schema; older Companion
  builds ignore the unknown fields and fall back to the contractor
  fresh-sync path (safe, just slow). `docs/INVITE_FORMAT.md` mirror
  re-synced from the canonical SPACESHOP TOOLS copy.
- **`src-tauri/src/commands/perforce.rs::adopt_in_place`** — new public
  fn. Runs `p4 sync -k //...[@N]` (have-table update, NO file transfer)
  then `p4 reconcile -n //...` (preview-only audit of disk-vs-depot).
  Returns `AdoptOutcome { have_table_files, reconcile_diff_count }`.
  Uses a dedicated `ADOPT_RECONCILE_TIMEOUT = 10 min` for the reconcile
  step (CALL_TIMEOUT's 30 s is too tight on 8000+ files).
- **`src-tauri/src/commands/onboarding.rs`** — Step 7 of `apply_invite`
  branches: when `adopt_existing == Some(true)` AND the workspace root
  is non-empty, call `adopt_in_place` instead of `initial_sync`. Empty-
  root edge case (operator picked a different folder in CHOOSE FOLDER)
  → soft-notify + fall back to `initial_sync`. Sync step detail reads
  "Adopted N files — workspace matches depot" or "Adopted N files —
  M pending changes detected".
- **`src/pages/Confirm.tsx`** — drives "What happens next" copy off a
  new `willAdoptInPlace` flag (canonical `invite.adopt_existing` with
  empty-auth-key fallback for pre-v0.6.2-Workshop invites). Adopt
  path shows "~30 seconds, no download"; fresh-sync path keeps the
  existing 2–5 min copy.
- **`src/lib/types.ts`** — `InviteData` mirror gains the two new
  optional fields; `STEP_LABELS.sync` renamed from "Downloading your
  project" to "Setting up your files" (the step's detail string carries
  the path-specific copy).
- Version bumps to 0.6.2 across `Cargo.toml` / `tauri.conf.json` /
  `package.json`. Built + published via `tools/perforce/build_companion.ps1 -Publish`.

**Carryover:** the v0.6.2 commit also rolled up the uncommitted v0.6.1
work (tailscale empty-auth-key short-circuit, errors.rs phrases parity,
projects.rs last_sync tracking, AGENTS.md) that had shipped as binaries
only without a git commit — per Session 4's note.

---

## Session 4 — 2026-05-25 — v0.6.1 (SELF-ONBOARD support + audit-followup hardening + AGENTS.md)

**Outcome:** v0.6.1 bundles four changes that ship as one release: the
empty-auth-key short-circuit needed for Workshop's new SELF-ONBOARD
INVITE flow, plus three MEDIUM-severity fixes flagged by the Session
59 Unreal+Perforce expert audit, plus the new Companion AGENTS.md so
future agents don't start conventions-blind.

### v0.6.1 changes

- **`src-tauri/src/commands/tailscale.rs::up`** — when `auth_key.is_empty()`,
  probe `status()` first. If joined (state Running / Started /
  Connected): return the current status as Ok without invoking
  `tailscale up --auth-key=""` (which errors). If not joined:
  surface a clear `CompanionError::Tailscale` telling the operator to
  supply a real key. Workshop's SELF-ONBOARD INVITE generator emits
  invites with `tailscale.auth_key = ""` because the admin's machine
  is already on the tailnet under their personal Tailscale account;
  pairing a fresh contractor auth key (`tag:contractor`) onto
  arsen's machine would be ACL-wrong.

- **`src-tauri/src/commands/errors.rs::AUTH_PHRASES`** — normalized
  to mirror `tools/perforce/p4_client.py::_AUTH_ERROR_PHRASES` in
  SPACESHOP TOOLS verbatim. Dropped the over-matching catch-all
  `"perforce password"` that would've classified benign info
  messages as auth errors. Inline comment points at the canonical
  source as the single source of truth for cross-repo lockstep.

- **`src-tauri/src/commands/projects.rs::Project`** — new
  `invite_local_root_default: String` field (`#[serde(default)]` for
  v0.6.0 back-compat). `from_invite` persists the invite's
  suggested root. New `root_diverges_from_invite()` helper
  (`#[allow(dead_code)]` for the future "verify integrity" admin
  surface) returns whether the operator's chosen `workspace_root`
  differs from the invite's default. Important for SELF-ONBOARD,
  where the depot already has a workspace bound to the invite's
  `local_root_default` and pointing Companion at a different folder
  silently breaks the no-file-transfer promise.

- **`src/pages/Confirm.tsx`** — gold-bordered drift warning banner
  shown when the operator-entered folder differs from the invite's
  default AND the invite is SELF-ONBOARD-shaped (empty
  `tailscale.auth_key`). Warns, doesn't block — operator can still
  override for non-standard layouts.

- **NEW `AGENTS.md`** at repo root — Companion-specific conventions
  (invite v=1 lock, p4tickets.txt write pattern, AUTH_PHRASES
  cross-repo lockstep, Tailscale MSI 1619 + empty-auth-key
  short-circuit, force_resync vs daily-Pull, FriendlyError pattern,
  version-bump 3-location lockstep, `#[serde(default)]` back-compat
  rule, audit/fix-pipeline conventions inherited from SPACESHOP
  TOOLS, light-vs-heavy pair-pattern scaling). Cross-links to
  SPACESHOP TOOLS AGENTS.md for shared rules.

**Backups:** `.scratch/audit_2026_05/backups/companion_empty_authkey/`
+ `auth_phrases_align/` + `companion_v0_6_1/` in the SPACESHOP TOOLS
repo (these patches were written from the SPACESHOP TOOLS session;
restore.py paths point into the Companion repo via `../spaceshop-companion/...`).

**Build:** Built + published v0.6.1 from this session via
`tools/perforce/build_companion.ps1 -Publish`.

---

## Session 3 — 2026-05-22 — marathon v0.5.4 → v0.6.0 + agent-orchestrated build process

**Outcome:** Started the session on v0.5.3 (signed, published, working).
Shipped v0.5.4 → v0.5.5 → v0.5.6 → v0.5.7 → v0.5.8 → v0.5.9 → v0.6.0
in one session, mostly via multi-agent parallel-fix dispatch with
audit-pair review between waves. Contractor's full onboarding +
auto-update path verified end-to-end on real hardware (not just the
dev box).

### v0.5.4 — bug-fix backlog from Session 2
- Parser handles `submit change <N> to ...` lines in `p4 status`
  (previously only `reconcile to ...` matched, so files already in a
  pending changelist were silently skipped from the Changes view)
- `submit_changes` calls `ensure_workspace_stream_bound` first —
  auto-heals workspaces created before stream-binding support
- Pull Latest always clickable, shows "Re-check & pull" when remote
  has no changes
- `force_resync` does `revert -k //...` → `flush //...#0` → `sync //...`
  instead of bare `sync -f` — handles stuck "open for delete" files
- `initial_sync` delegates to `force_resync` so re-onboards heal
- Update-check spam suppressed when endpoint is a placeholder
- Footer removed, YES gates on destructive actions, "SERVER DETAILS"
  label instead of "COPY THESE INTO UNREAL"
- Generic-token redaction across docs (`<SPACESHOP_TOOLS_ROOT>` etc.)
- Dropped `tauri-plugin-shell` + `tauri-plugin-opener` (dead)
- USER_GUIDE.md + CONTRACTOR_ONBOARDING_TEMPLATE.md rewritten for
  the v0.5.2 Open-in-Unreal flow

### Repo rename — `spaceshop-companion` → `onboard`
- Public URL: https://specars4.github.io/onboard/
- Repo: https://github.com/specars4/onboard
- All updater endpoints updated; GitHub's repo-rename redirect
  covers old URLs in the wild

### v0.5.5–v0.5.8 — onboarding install-path fixes from contractor reports
- v0.5.5: split UPDATE_ENDPOINT_PHRASES into NETWORK / VERIFY /
  GENERIC so Check-for-updates can report the actual cause
- v0.5.6: Tailscale msiexec `WaitForExit` (was racing `Select-Object
  -ExpandProperty ExitCode`)
- v0.5.7: copy bundled tailscale.msi to %TEMP% before msiexec to
  dodge "exit 1619 ERROR_INSTALL_PACKAGE_OPEN_FAILED" on paths with
  spaces (`C:\Program Files\Spaceshop Companion\...`); also Win32
  GetShortPathName fallback for spacey usernames
- v0.5.8: streaming-sync wall-clock timeout REMOVED — was killing
  big-file syncs that legitimately took >1h; replaced with idle-only
  semantics (later refined to no timeout + `-vnet.maxwait=300`).
  Fixed Wix `upgradeCode` for clean MajorUpgrade. 8.3 short-name
  fallback. Larger Refresh button.
- v0.5.9: backend hardening pack (10 fixes including ensure_binaries
  startup-error event, explorer.exe path escape, workspace_root
  guards on all p4 entry points, tailscale JSON parse defensive,
  p4 stderr classification, find_uproject symlink safety,
  write_ticket_file Mutex, temp MSI per-process name,
  `-vnet.maxwait=300`, errors.rs UPDATE_NETWORK/VERIFY/GENERIC split).
  Frontend: version in header, live pull-progress, error-details
  disclosure, fatal startup-error banner. Build: rustc 1.94 pin,
  Tauri 2.11 pin, NSIS dropped. Windows Sandbox smoke harness.

### v0.6.0 — feature wave (Wave 1 + Wave 2 + audit fixes)
- WiX `util:CloseApplication` fragment re-enabled (top-level
  Fragment + anchor Component pattern, `componentRefs` reference
  to satisfy Tauri bundler's link-or-drop semantics)
- "Repair workspace" button (`p4 clean` + `sync -f`) with confirm
  modal explaining orphan-deletion
- Clean uninstall feature with smart Tailscale detection
  (install-time flag + YYYYMMDD InstallDate fallback), scrubs
  p4tickets line, deletes Spaceshop data dirs, optional Tailscale
  uninstall, self-uninstalls via `msiexec /x`
- Tray polling: runtime icon tinting (luminance-weighted RGBA
  blend, no extra crate), 30s loop, per-project Sync Now menu,
  live tooltip
- Welcome background poll every 3 min with visibility-pause
- Shared `<Banner>` primitive (UpdateBanner + StartupErrorBanner)
- `invite::project_name` length + control-char validation
- `[profile.release.package.tauri] opt-level=1, codegen-units=1`
  — workaround for deterministic rustc crash on tauri lib at
  default opt-level=3

### Process changes
- **Multi-agent parallel fix dispatch** — most v0.6 work done by
  4-agent waves (each owning a self-contained slice), audited by
  a 2-agent pair (integration + security/correctness) between
  waves. Worked well; iteration time was ~30 min per wave including
  audit + integration + build.
- **VM testing infra shipped** but not yet enabled (`smoke/` folder).
  Activate by enabling Windows Sandbox on the build host.
- **Hot-fix-machine pattern called out** — earlier in session we
  kept blindly retrying rustc crashes; switched to actually pinning
  the toolchain and applying targeted opt-level overrides.

### What the contractor sees in v0.6.0
- Banner update: `v0.5.7 → v0.6.0` on next launch (or via Advanced
  → Check for updates manually)
- Header now shows running version
- Repair workspace + Clean uninstall buttons in Advanced section
- Color-coded tray icon (green / yellow / red)
- Per-project Sync from tray menu
- Live `{count} files · current filename` during pull
- Auto-update technical details disclosure if Check fails

### Still queued for v0.7
See BACKLOG.md "Up next (v0.7)" — Cancel button + Win32 I/O byte
counters, `-ztag` parser refactor, Welcome+tray poll dedup,
filesystem-watcher byte progress, File History view, installer logo,
defensive nonce on clean_uninstall, etc.

---

## Session 2 continuation — 2026-05-21 evening — daily-ops + ship infrastructure

**Outcome:** v0.5.3 shipped to GitHub (public repo, signed, auto-update verified
end-to-end). v0.5.4 attempted with critical bug fixes but build pipeline
stalled — needs fresh session to finish.

### What landed

- **GitHub repo live + public**: https://github.com/specars4/onboard
- **v0.5.3 release published with signed MSI**: https://github.com/specars4/onboard/releases/tag/v0.5.3
- **Auto-update endpoint verified** — anonymous fetch returns valid signed
  manifest, MSI URL resolves
- **GitHub Pages branch pushed** (`gh-pages`) — landing page at
  `https://specars4.github.io/onboard/` with single big "Install
  Companion" button + SmartScreen guidance + JS that auto-fetches the
  latest release. Needs ~60s after push to go live; verify when next
  session starts.
- **Signing key** moved into the operator's local SPACESHOP TOOLS
  workspace under `tools/_secrets.py` as `COMPANION_UPDATER_PRIVATE_KEY`.
  Local `.keys/` deleted from the Companion repo.
- **One-command publish helper**: `<SPACESHOP_TOOLS_ROOT>/tools/perforce/build_companion.ps1`
  — reads the key, runs `tauri build`, optionally cuts the GitHub Release.
  Had a bug (empty-string PowerShell args + Python -c indent + em-dash
  chars in source) — all fixed. Should work in a fresh shell.

### Bug fixes between v0.5.3 → intended-v0.5.4 (in git, NOT yet in a published .msi)

| Bug | Where | Fix |
|---|---|---|
| `p4 status` parser only handled `reconcile to` lines, skipped `submit change N to add` lines | `list_changes` | Now matches both formats |
| Submit fails on pre-fix workspaces with "cannot submit from non-stream client" | `submit_changes` | Calls `ensure_workspace_stream_bound` before reconcile/submit; auto-heal preserves existing spec, adds Stream binding derived from view |
| Pull Latest button greyed when no remote changes — felt broken | `Project.tsx` | Always clickable, shows "Re-check & pull" when nothing new |
| Update-check spammed ERROR every launch + every click while endpoint was placeholder | `updater.rs` | Detect placeholder, skip silently with one info log |
| Footer "ARSEN ARZUMANYAN..." unwanted | `Shell.tsx` | Removed |
| Confirm gates required typing project name | `ForceResyncConfirm`, Project Remove inline | Now require typing `YES` |
| "COPY THESE INTO UNREAL" label too prescriptive | `Project.tsx` | Renamed to "SERVER DETAILS" with softer caption |

### Server-side: contractors group Timeout was 12h → bumped to 90 days

`set_contractor_group_timeout(client, days=90)` added to
`tools/perforce/admin.py`; ran live against NAS p4d. Smoke invite now has
90-day ticket. Existing tickets minted before the change keep their old
lifetime (Perforce reuses, doesn't refresh). For full reset run
`regenerate_smoke_invite.py` which does logout-then-login.

### What broke in the last hour (root cause: trying to do too much in one Claude session)

1. **rustc STATUS_ACCESS_VIOLATION (0xc0000005)** during incremental release
   builds — intermittent rustc 1.95 / MSVC bug. Cleared
   `target/release/incremental` once, helped temporarily.
2. **Tauri `signer sign` CLI hangs** when invoked outside a fully-interactive
   shell; prompts for confirmation that never comes. Use `npm run tauri
   build` with env vars set — that path produces .sig reliably.
3. **PowerShell strips empty-string args** to native exes, so `-p ""` becomes
   `-p` with no value. Either use `[Parameter]` flag syntax (`--no-password`)
   or omit the flag and rely on the env var.
4. **My background-task timeouts** were swallowing in-flight `npm run tauri
   build` runs. The next session should run the build in a real
   foreground PowerShell terminal (not via Claude's tool subprocess) so
   nothing reaps it.

### Current artifact state

- `src-tauri/target/release/bundle/msi/Spaceshop Companion_0.5.4_x64_en-US.msi`
  exists (39 MB) but is **unsigned** (.sig missing). Don't ship this.
- v0.5.3 .msi + .sig are both fine and on the GitHub Release. Safe to
  send contractors today if necessary.

### What the next session should do (in order)

1. **Verify GitHub Pages is live** — visit
   `https://specars4.github.io/onboard/`. If 404, wait 5
   minutes; if still 404, check Pages settings via `gh api /repos/specars4/onboard/pages`.
2. **Publish v0.5.4** — in a real PowerShell terminal (not through Claude
   tools), run:
   ```powershell
   cd <SPACESHOP_TOOLS_ROOT>
   powershell -ExecutionPolicy Bypass -File .\tools\perforce\build_companion.ps1 `
     -Notes "Parser handles pending-changelist files; submit auto-heals stream binding; pull latest always clickable." `
     -Publish
   ```
   If rustc crashes, `cd <COMPANION_REPO_ROOT>\src-tauri; cargo clean`
   (full clean, not just incremental), then re-run.
3. **Smoke-test end-to-end** — install v0.5.4 .msi on Arsen's machine,
   paste the smoke invite, walk through:
   - Onboarding succeeds
   - Make a file → Review & submit → succeeds (stream auto-heal)
   - Re-check & pull → green banner
   - Auto-update banner doesn't fire (we're on latest)
4. **Ship to Arsen's contractor** — send them:
   - URL: `https://specars4.github.io/onboard/`
   - The invite code generated via Workshop's PERFORCE → INVITES panel
     (or `tools/perforce/regenerate_smoke_invite.py` for smoke testing)

---

## Session 2 — 2026-05-20 / 2026-05-21 — v0.5 build + UX iteration + v0.5.2 follow-on

### Latest pass (2026-05-21, continuation) — v0.5.2 "Open in Unreal"

Big gold **Open in Unreal** hero CTA in the top-right of the project page,
next to the title. Click → Companion writes
`<project>/Saved/Config/WindowsEditor/SourceControlSettings.ini` (with the
project's server/user/workspace) **then** shell-launches the .uproject.
Unreal opens with Perforce already configured — no Editor Preferences
detour needed. The ticket is already in `%USERPROFILE%\p4tickets.txt`, so
Unreal authenticates with zero contractor input.

Window also shrunk from 880×640 to 860×600 (min 720×520) so the daily
project surface fits without scrolling on smaller displays.

Mechanism researched against Epic's UE 5.7 docs + community forum threads:
INI section names + key names confirmed (Port / UserName / Workspace).
The .uproject finder searches the workspace root + one level deep,
skipping engine cache dirs (Saved / Intermediate / DerivedDataCache /
Binaries / Build / .git / .p4).

### Earlier this session

**Outcome:** Working .msi installer end-to-end; mocked-and-agreed v2 daily-use
UI; ready to port to React and bake the updater in for the first contractor
release.

**Built in pass 1 (Session 2, 2026-05-20):**
- Full Tauri 2.x scaffold at this repo root
- Rust commands: invite parsing (mirrors `tools/perforce/invite.py` v=1),
  Tailscale lifecycle (bundled MSI + msiexec /qn install path), Perforce
  bootstrap (info / create_workspace / initial_sync / write_ticket_file),
  full onboarding chain with step events
- React frontend: Welcome / Confirm / Connecting / Project screens with
  Spaceshop brand styling
- System tray + hide-on-close + multi-project state persistence
- Produced `Spaceshop Companion_0.5.0_x64_en-US.msi` (37 MB)
- Smoke-tested end-to-end with `.scratch/perforce_session/test_invite_for_session_2.txt`
  from SPACESHOP TOOLS — tailscale up → p4 info → ticket write → workspace
  create → 3-file sync. One bug found and fixed: `p4tickets.txt` was
  read-only on existing Perforce users; Companion now clears the read-only
  bit before writing.

**Iteration pass 2 (2026-05-21):**
- Static HTML mockups in `mockups/` for iterating UI without
  install/rebuild round-trips. Five screens agreed:
  - **Daily v2** — status row + gold "N local not sent" / muted "X on
    server" badges + button bar + credentials and Advanced collapsibles +
    "‹ All projects" header nav
  - **Changes view** (renamed from Submit) — scrollable, master + per-row
    checkboxes, "Restore from server" replaces "Discard", ︙ per-row menu
    (Reveal / Restore / Show history grayed for v0.6 / Copy path)
  - **Conflict modal** — plain-language explainer, 3 options per file
    (Use server / Keep mine / Skip), pre-selected sensible default
  - **Force re-download confirm** — destructive modal with type-project-name
    gate, lists what will be lost
  - **All Projects v2** — no per-tile Pull, status hints per tile,
    "Status from last connection" for offline projects

**Now (Session 2 continuation, 2026-05-21):**
- Port the mockup designs into actual React + Rust
- Add the new Rust commands (list_changes / submit_changes / restore_file /
  force_resync / change_counts / conflict resolution)
- Bake in **Tauri updater plugin** + Ed25519 signing keypair so v0.5.1 (the
  first build sent to a real contractor) already self-updates from GitHub
  Releases
- Ship v0.5.1 .msi by end-of-session

See [`BACKLOG.md`](BACKLOG.md) for what's queued after v0.5.1.

---

## Session 1 — N/A

Companion didn't exist. Session 51 of SPACESHOP TOOLS built the Perforce
foundation (NAS p4d, Tailscale tailnet, invite generator in Workshop) that
this app talks to.
