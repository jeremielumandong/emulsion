# Cloud accounts and developer registrations

Emulsion's cloud integration is experimental. The implementation has local
storage tests; live provider testing and production verification require
developer registrations. No personal client IDs or tokens ship in this source
tree. [Implementation specification](../specs/cloud-sync-spec.md) ·
[Feasibility and follow-on work](../specs/cloud-sync-plan.md).

## Open-source distribution

An OAuth client ID identifies an application and is public. Official Emulsion
releases can use registrations owned by the project; each person signs in to
their own provider account. Forks can supply their own registrations. Native
apps use PKCE and the system browser because they cannot keep a confidential
client secret. [Google native OAuth](https://developers.google.com/identity/protocols/oauth2/native-app),
[Microsoft public clients](https://learn.microsoft.com/en-us/entra/identity-platform/msal-client-applications).

Official builders can set `EMULSION_GOOGLE_CLIENT_ID`,
`EMULSION_DROPBOX_CLIENT_ID`, and `EMULSION_ONEDRIVE_CLIENT_ID` at compile time.
Google Desktop registrations that require their native client-secret field
can additionally set `EMULSION_GOOGLE_DESKTOP_CLIENT_SECRET`. These values are
extractable from the binary; this mechanism never protects a confidential
server secret. With no build values, the app starts unconfigured. Local
registration imports override build defaults. Development and production
registrations have separate credential and queue identities even for the
same signed-in user.

For development, import registration JSON locally. It is stored in the
application data directory, outside the repository. User access/refresh tokens
go into the OS credential store, with an explicit session-only fallback.
Never commit token files, service-account keys, or a server application's
confidential client secret. Google's downloaded **Desktop** credentials may
include a client-secret field; this native-client field is accepted locally
and must not be confused with a protected server credential.

## Google Drive and Google Photos

In [Google Cloud Console](https://console.cloud.google.com/), create a project,
enable **Google Drive API** and **Google Photos Picker API**, and configure
Google Auth Platform. During development, use an External audience in Testing
and add your Google account as a test user. Create a **Desktop app** OAuth
client and download the JSON file.

Open **Home → Cloud files → Manage connections… → Import app registration…** and
select that JSON. It configures both Google features, but each connects
separately. The Photos Library API is not needed for selected-photo imports.
Google registrations in Testing have provider-imposed limits; complete the
applicable publishing/verification process before distributing broadly.
[OAuth setup](https://developers.google.com/identity/protocols/oauth2/native-app),
[Photos authorization](https://developers.google.com/photos/overview/authorization).

## Troubleshooting Google connections

The browser callback acknowledges receipt of Google's response. The connection
is complete only when Emulsion shows your account instead of **Not connected**.
Errors appear above the provider rows and identify token exchange, account
lookup, or storage setup. Declining consent ends the attempt immediately;
start a new connection to try again.

- **Google API is disabled / storage setup:** enable **Google Drive API** in
  the Google Cloud project that owns the imported Desktop registration. For
  photo selection, enable **Google Photos Picker API** in that same project.
  Allow time for activation, then retry.
- **Required permission was not granted:** reconnect and select the requested
  access on Google's consent screen.
- **App registration rejected:** download the current **Desktop app** client
  JSON, import it again, and reconnect.
- **Sign-in code rejected:** begin a new connection from Emulsion instead of
  refreshing an old browser callback page.

See [Google Drive error guidance](https://developers.google.com/workspace/drive/api/guides/handle-errors)
and [Google Desktop OAuth guidance](https://developers.google.com/identity/protocols/oauth2/native-app).

Developers can check network reachability without reading or sending any
registration, account credentials, or tokens:

```sh
cargo run -p emulsion-cloud --example check_google_connection
```

The unauthenticated token and account requests deliberately expect HTTP 400
and 401. Passing this check verifies connectivity only; it does not verify
account consent or API enablement.

## Dropbox

Create a Scoped access application in the
[Dropbox App Console](https://www.dropbox.com/developers/apps), using **App
folder** content access. Enable `account_info.read`, `files.metadata.read`,
`files.content.read`, and `files.content.write`, and allow public clients/PKCE.
Register this redirect URI exactly:

```text
http://127.0.0.1:53682/callback
```

Copy the app key into the JSON example below; a Dropbox client secret is not
required for this PKCE client. [Dropbox OAuth](https://docs.dropboxapi.com/dropbox-api/docs/oauth).

## OneDrive

Create an application under **Microsoft Entra → App registrations** with
organizational and personal Microsoft accounts supported. Add the **Mobile
and desktop applications** platform and the loopback redirect above. Configure
delegated Microsoft Graph permissions `User.Read`,
`Files.ReadWrite.AppFolder`, and `offline_access`. Some console versions
require entering an HTTP loopback IP redirect through the app manifest.
Do not create a confidential-client secret. Corporate tenant policies can
require administrator consent even for delegated access.
[Microsoft native redirects](https://learn.microsoft.com/en-us/entra/identity-platform/reply-url),
[OneDrive app folder](https://learn.microsoft.com/en-us/graph/onedrive-sharepoint-appfolder).

## Registration JSON for Dropbox and Microsoft

Save a local file such as `emulsion-cloud-clients.json`, replace the placeholders
with your public application identifiers, and import it through **Home → Cloud files → Manage connections…**:

```json
{
  "clients": {
    "dropbox": { "client_id": "YOUR_DROPBOX_APP_KEY" },
    "onedrive": { "client_id": "YOUR_MICROSOFT_APPLICATION_ID" }
  }
}
```

Import merges these entries with existing registrations. You can also supply
`google_drive` and `google_photos` entries in this format, though importing
Google's downloaded Desktop JSON is easier. Never import a web-server or
service-account credential file.

## Using the integration

1. Open **Home → Cloud files** in the sidebar (or **Cloud & Photos** above local files).
   Use **Manage connections…** to connect accounts.
2. Connect the desired provider and finish sign-in in your system browser.
3. On Home or inside a project, use **Sync to Google Drive** directly beneath
   a saved photo/file card (also available in list view). If multiple storage
   providers are connected, **Sync to cloud…** offers a destination menu.
   This enables automatic sync and starts uploading immediately.
   Native projects include their referenced RAW originals, including references
   retained by native history. A missing/mismatched original prevents queueing
   a misleadingly incomplete portable revision.
4. Each file shows a cloud-status icon and its provider, such as **Google Drive
   · Synced**. A green cloud check means its saved snapshot has uploaded.
   Syncing, queued, paused, retry-needed, disconnected, and unsaved-edit states
   have distinct icons and labels. **Local only** means sync has not been
   enabled for that file. **Sync now** on a card retries its saved copy;
   paused files offer **Resume sync**. Status describes Emulsion's saved
   snapshots; files changed externally are checked when next queued.
   **Refresh / retry** in Home’s cloud browser uploads pending revisions and refreshes the file list.
   Subsequent successful saves automatically queue snapshots; the running app
   also checks periodically. The file’s right-click or three-dot menu has **Pause sync**
   and **Cloud version history…**. Pausing retains queued work.
5. On another installation using the same app registration and provider
   account, connect and refresh **Cloud files**, select **Download copy**, then
   **Open downloaded copy**. Each cloud file appears once, with provider filters,
   filename search, and 48 files per page. **Version history…** opens only that
   file’s revisions, also paginated. Every download gets a separate local directory.
   Conflicts require choosing a version in history; both remain available.
   Storyboards can instead merge the other version into the open file; see
   [Shared storyboards](#shared-storyboards).
6. Use **Library → Import from Google Photos…** or the Photos button under **Manage connections…**
   to select images in Google's Picker. Imports are local creative copies;
   Google may omit location metadata. Unsupported media and failed downloads
   are counted separately. They are not continuously synchronized with Photos.

Cloud storage uses immutable `.emulsion` revision objects containing a header
and a portable payload, rather than a mutable `.ora`/`.emu` at a fixed remote
path. Old versions consume provider storage; automatic pruning is not enabled.
This format preserves concurrent edits without provider-specific overwrite
races. A project is synchronized to one provider at a time.

Google Drive stores uploads inside its dedicated **Emulsion** folder. New
snapshots with Home organization use **Emulsion / project name / original
filename / revision.emulsion**; unassigned files use **Unfiled**. The filename
entry is a folder containing immutable versions, not a directly viewable JPEG.
Use **Home → Cloud files → Open Drive folder** to open the storage location.
Previously uploaded snapshots remain discoverable in their existing locations.

Each synced file now records its Home project identity/name, assigned display
name, original filename, and classification. Downloading a copy restores those
details and groups files from the same project together. Separate projects with
identical names stay separate. Name or membership changes queue a new snapshot
on the next sync check, even when the saved image bytes have not changed.
Google Drive moves the file's version folder when its project changes. Dropbox
and OneDrive retain their provider app-folder layout, with the same portable
Home metadata in their snapshots.

This preserves the membership of files individually enabled for sync. It does
not enable uploads for other project members, save empty Home projects, or
replace downloading individual files with a whole-project restore action.

Disconnect removes the saved OAuth credential and stops that provider's work;
local documents, imports, and pending revisions remain. Reconnecting the same
account resumes its work. Connecting a different account does not transfer
the old account's jobs. Local Home trash never deletes cloud objects.

## Storage and current limits

Cloud state lives under `<Emulsion data directory>/cloud/`:

| Path | Purpose |
| --- | --- |
| `clients.json` | Local developer registrations |
| `index.json` | Account metadata, path bindings, revisions, and pending work; no OAuth tokens |
| `outbox/` | Immutable pending upload objects |
| `downloads/` | Verified, durable local project copies and originals |
| `revisions/` | Verified read-only copies of revisions fetched for storyboard merges |
| `remote/` | The last cloud listing of each open shared storyboard, for offline status and MCP |
| `photo-imports/` | Durable selected images and source attribution metadata |

The index is atomically replaced under a process lock. Transfers use a separate
lock so they do not hold the save/index transaction open. File contents and
imports are normal local artwork files, protected by filesystem permissions;
this version does not implement a separate encrypted media vault. Use an
encrypted local volume for sensitive artwork. Provider data policy and
encrypted-at-rest requirements need deployment review before a public Photos
release. [Photos data policy](https://developers.google.com/photos/support/api-policy).

Limits include 4 GiB portable cloud payloads, 1,000 bundle files, 20,000 listed
cloud revision objects, 2,000 selected images per Photos import, and 512 MiB
per imported image. Existing native document limits still apply. Full-file
revisions are uploaded, not binary deltas. Uploads use provider chunk sessions;
after a failed session or process restart, a retry may restart the transfer.
Completed but unacknowledged uploads are discovered and verified on retry.

Scope of this implementation: app-managed cloud revisions, manual version
selection, offline queued saves, native RAW portability, selected Photos
imports, and three-way merges of concurrent storyboard saves. Existing
arbitrary Drive/Dropbox/OneDrive folder browsing, library metadata sync,
merging other kinds of files, background sync while Emulsion is closed,
remote version deletion, and Photos export remain follow-on work.

## Shared storyboards

A synced storyboard is a shared project: teammates sign in to the same
provider account, each downloads the file once from **Cloud files**, and
works on their copy. Every save uploads as usual. When two people save from
the same version, both revisions are kept as two heads. The storyboard then
says that another artist's save is waiting; **File → Shared Project… →
Review and merge…** downloads that head and the common ancestor (the newest
revision both descend from, found from the revisions' parents, never from
clocks), verifies them, keeps them under `cloud/revisions/` for reuse, and
merges the boards three ways in the open editor. See the
[Storyboard guide](storyboard.md#shared-projects) for what merges and how
conflicts are chosen.

Saving the merge uploads a **merge revision**: an ordinary immutable
revision whose header names a second parent (`merged`), the head it took
in. A merge revision supersedes both of its parents, so the file has one
head again. Revision headers also carry the saving artist's name from
**Settings › Storyboard › Your name** (`author`), shown as collaborators.
Both fields are optional: older revisions without them read unchanged, and
an older Emulsion reads a merge revision too (it ignores the second parent
and shows the merged head as a separate version, as before). Nothing is
merged automatically and nothing is replaced in place; a conflict nobody
chose keeps the merging artist's version, and the other save stays in the
cloud.

Storyboard revisions travel without a Home classification, so older
Emulsion releases, which reject that kind in a header, can still list the
account.

Scene claims (who is working on which scenes) are saved in the storyboard
and travel with its revisions. There is no live presence: claims and other
artists' saves appear after a sync.

## Organizing Home

Open the three-dot menu on a project card or its sidebar entry for **Rename
project…** and **Delete project · keep files**. Right-click opens the same menu.
Inside a project, **Project actions** is available above the files, including in
narrow windows. Deleting a project removes the Home grouping and returns its
files to Unfiled; it does not delete the source files or cloud objects.

Right-click a photo/file card, or open its three-dot menu, for **Move to project…**,
**Classify as…**, rename, trash/restore, and cloud history. Moves update Home’s
project membership; the source path and its cloud binding stay intact. Explicit
classification survives reopening and saving. Photo classification no longer
follows the editor’s Paint workspace preference.

Local collections render 48 files per page and load thumbnails only for that
page. Cloud browsing groups revisions into files before paging. The provider
adapters still fetch the full remote metadata listing on refresh; this UI change
does not implement incremental provider indexing. Clicking the Emulsion brand
in the editor returns straight to Home and preserves open tabs.
