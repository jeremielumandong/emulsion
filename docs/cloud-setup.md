# Cloud accounts and developer registrations

Emulsion's cloud integration is experimental. The implementation has local
storage tests; live provider testing and production verification require
developer registrations. No personal client IDs or tokens ship in this source
tree. [Implementation specification](cloud-sync-spec.md) ·
[Feasibility and follow-on work](cloud-sync-plan.md).

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

Open **Settings → Cloud projects and photos → Import app registration…** and
select that JSON. It configures both Google features, but each connects
separately. The Photos Library API is not needed for selected-photo imports.
Google registrations in Testing have provider-imposed limits; complete the
applicable publishing/verification process before distributing broadly.
[OAuth setup](https://developers.google.com/identity/protocols/oauth2/native-app),
[Photos authorization](https://developers.google.com/photos/overview/authorization).

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
with your public application identifiers, and import it through Settings:

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

1. Open **Home → Cloud & Photos**, or the cloud section at the top of Settings.
2. Connect the desired provider and finish sign-in in your system browser.
3. Open and save a file, then choose **Sync current file** for its destination.
   Native projects include their referenced RAW originals, including references
   retained by native history. A missing/mismatched original prevents queueing
   a misleadingly incomplete portable revision.
4. **Sync now / retry** uploads pending revisions and lists remote versions.
   Subsequent successful saves automatically queue snapshots; the running app
   also checks periodically. Pause stops uploads but retains queued work.
5. On another installation using the same app registration and provider
   account, connect and sync, select **Download copy**, then **Open downloaded
   copy**. Every download gets a separate local directory. Conflicting heads
   are labeled and both remain available.
6. Use **Library → Import from Google Photos…** or the Photos button in Settings
   to select images in Google's Picker. Imports are local creative copies;
   Google may omit location metadata. Unsupported media and failed downloads
   are counted separately. They are not continuously synchronized with Photos.

Cloud storage uses immutable `.emulsion` revision objects containing a header
and a portable payload, rather than a mutable `.ora`/`.emu` at a fixed remote
path. Old versions consume provider storage; automatic pruning is not enabled.
This format preserves concurrent edits without provider-specific overwrite
races. A project is synchronized to one provider at a time.

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
selection, offline queued saves, native RAW portability, and selected Photos
imports. Existing arbitrary Drive/Dropbox/OneDrive folder browsing, library
metadata sync, automatic merge, background sync while Emulsion is closed,
remote version deletion, and Photos export remain follow-on work.
