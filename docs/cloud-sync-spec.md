# Cloud integration implementation specification

Status: implementation in progress, 2026-09-27. Based on the
[feasibility plan](cloud-sync-plan.md). Live provider acceptance requires
developer registrations, which are not available yet.

## Release contract

The desktop app connects to Google Drive, Dropbox, and OneDrive using native
OAuth with PKCE. Each saved, opted-in file becomes an immutable cloud revision.
Another installation discovers revisions and downloads a selected version.
Google Photos uses explicit Picker imports into the local editing library.
All providers are optional. Missing registration is a setup state, not a
working-looking connection button.

Local saves remain authoritative. Network failure cannot undo or block a local
save. Capture a stable snapshot on the save worker before releasing the saved
generation. Persist jobs before upload; restart resumes pending work. Never
replace a cloud file in place or use device clocks to resolve conflicts.

An immutable revision contains project UUID, revision UUID, parent revision,
device UUID, SHA-256, name, and size, plus the complete file. Concurrent children
are both retained. Discovery identifies all heads; downloading/opening either
version preserves the other. No automatic destructive conflict resolution.

The first implementation uses a bounded, atomically replaced JSON index and a
process lock, rather than migrating the existing catalog to SQLite. Existing
local catalog IDs remain unchanged. The separate index maps canonical local
paths to stable project IDs and provider/account bindings. This limits the
migration surface while retaining a future database migration path.

## Components and work order

1. `emulsion-cloud`: provider types, validated revisions, durable snapshots,
   restartable queue, revision graph, and account isolation.
2. Native authorization: system browser, loopback callback, state and PKCE,
   token refresh, operating-system credential storage; explicit session-only
   fallback when the credential store is unavailable.
3. File adapters: Drive resumable uploads, Dropbox upload sessions, OneDrive
   upload sessions; paginated scoped discovery and bounded downloads. Store
   complete immutable revision objects so publication needs no cross-file
   atomic transaction. Verify hashes before opening any download.
4. Native portability: bundle `.emu`/`.ora` with RAW originals referenced by
   current documents and history; rebind verified originals on download.
5. Photos: create/cancel/poll Picker session, paginate selected images, bounded
   downloads and duplicate-safe local import. Temporary media URLs stay out of
   persistent catalogs. Skip unsupported media with an explicit result.
6. GPUI: connected accounts, configurable registrations, sync current file,
   pending status/retry/pause, remote version browser, download/open, photo
   import, and save completion integration. Network work uses background tasks.
7. Validation: deterministic provider protocol and storage tests, UI compilation
   and interaction tests, formatting and lint checks, then credentialed manual
   acceptance on two installations when registrations are available.

## Boundaries

Initially one connected account per provider and one provider per local file.
Switching accounts must not reuse another account's queue or bindings.
An explicit rebind creates a new project identity unless opening a known
remote revision. Remote-only projects are downloaded on demand, not silently
written over open editors. Removal from Home does not delete cloud data.
Pausing/disconnecting retains unsynced snapshots and local artwork.

Managed objects are immutable, so retry after an ambiguous response must look
up the revision identity and verify it before treating the job as complete.
Transient failures retain the job and use bounded backoff. Authentication and
quota failures remain visible. Provider error bodies and bearer URLs must
never be included in user-visible errors or logs.

The package format is versioned and bounded. Remote paths are never accepted
as local destinations. Extract only validated entries into a newly created
managed directory; reject traversal, duplicates, unexpected entries, excess
decoded size, bad hashes, and unsupported versions. Preserve original RAW
bytes and validate existing native archives before opening.

Whole-file revisions consume storage and bandwidth. Version pruning, shared
drives, arbitrary remote-folder browsing, collection-metadata reconciliation,
multi-provider mirrors, real-time collaboration, and closed-app background
sync remain later stages from the feasibility plan. Photos full-library
mirroring is unavailable. A Photos import is a user-selected creative import,
not an archival-original guarantee.

## Required acceptance evidence

- Two saves from the same base remain separate heads after reconciliation.
- Process restart preserves pending jobs and completed generation identity.
- Interrupted/ambiguous uploads do not lose the local file or create new
  logical revisions on retry.
- Account switches cannot upload queued work into a different account.
- Stale downloads never overwrite local edits; verified downloads open through
  the normal native file validation path.
- RAW sources and historical RAW references are portable to a different path.
- Callback forgery, unexpected redirect hosts, invalid manifests, traversal,
  oversized responses, and hash mismatches are rejected.
- Picker cancellation/expiration and partial import failures remain recoverable.
- UI distinguishes saved locally, queued, paused, synced, and failed work.
- No account credentials are written to project files or the ordinary index.

Provider production verification, privacy/disclosure review, encrypted-at-rest
handling appropriate to the deployed environment, and real-account tests on
Linux/macOS/Windows are release gates, not results implied by mocked tests.
