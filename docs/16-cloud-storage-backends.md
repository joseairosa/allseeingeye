# 16 - Cloud storage backends

Status: PENDING (spec only; no code lands here)

This phase documents the production swap from `LocalDirectoryStorage`
(today's only `BackupStorage` impl) to a cloud target. The encryption
layer from Phase 15 is unchanged; the cloud only ever sees ciphertext.
The whole point of doing the v0 envelope encryption first is that
this phase is a single new trait impl plus credential plumbing - no
crypto rework, no new threat model surface.

We are explicitly **not committing to a provider** yet. The spec is
provider-neutral; the trait and the credential model work for AWS S3,
Cloudflare R2, GCS, Backblaze B2, MinIO, and any other S3-compatible
or HTTP PUT/GET storage.

---

## 16.1 - The contract is already drawn

Phase 15 carved out the trait at `backup/storage.rs:21`:

```rust
pub trait BackupStorage: Send + Sync {
    fn put_blob(&self, relative_path: &str, bytes: &[u8]) -> Result<(), StorageError>;
    fn get_blob(&self, relative_path: &str) -> Result<Vec<u8>, StorageError>;
    fn delete_blob(&self, relative_path: &str) -> Result<(), StorageError>;
    fn root_for_display(&self) -> String;
}
```

A cloud impl ships **one new struct**. No other code changes.

```rust
pub struct S3Storage {
    bucket: String,
    prefix: String,
    client: aws_sdk_s3::Client,
}

impl BackupStorage for S3Storage {
    fn put_blob(&self, relative_path: &str, bytes: &[u8]) -> Result<(), StorageError> {
        let key = format!("{}{}", self.prefix, relative_path);
        let body = aws_sdk_s3::primitives::ByteStream::from(bytes.to_vec());
        runtime::block_on(
            self.client
                .put_object()
                .bucket(&self.bucket)
                .key(key)
                .body(body)
                .if_none_match("*")  // see 16.4 idempotency
                .send(),
        )?;
        Ok(())
    }
    // ... get / delete / root_for_display similar shape
}
```

The orchestrator's `backup_now` loop is identical. The user's keychain
private key is identical. The blob format on disk is identical. Only
the bytes-to-storage path changes.

---

## 16.2 - Credential model (the part that matters)

The user **must not** put their AWS root credentials into the app.
The app holds the **lowest-blast-radius credential** that can put +
get a single bucket prefix and nothing else.

### Recommended shape: per-bucket scoped IAM key

```
Permissions on the IAM user the app holds:
  s3:PutObject        on arn:aws:s3:::user-bucket/aseye-prefix/*
  s3:GetObject        on arn:aws:s3:::user-bucket/aseye-prefix/*
  s3:DeleteObject     on arn:aws:s3:::user-bucket/aseye-prefix/*
  s3:ListBucket       on arn:aws:s3:::user-bucket  (filtered to aseye-prefix/*)

Nothing else.
```

If the credential leaks, the blast radius is **reading or deleting
the user's already-encrypted blobs**. The blobs are still useless
without the device private key. Worst case: the attacker can rm the
backup, which is annoying but not catastrophic - the source files are
still on disk.

### Credential storage

The IAM access key + secret live in **the same OS keychain** as the
device backup key (Phase 15). New keychain entries:

| Service | Account | Value |
|---|---|---|
| `dev.allseeingeye.cloud-backup` | `aws-access-key-id` | string |
| `dev.allseeingeye.cloud-backup` | `aws-secret-access-key` | string |
| `dev.allseeingeye.cloud-backup` | `provider-config` | JSON: `{ provider, region, bucket, prefix, endpoint? }` |

`provider-config` covers the per-provider details:

```json
{
  "provider": "s3" | "r2" | "gcs" | "b2" | "custom",
  "region": "us-east-1",
  "bucket": "my-aseye-backup",
  "prefix": "user-12345/",
  "endpoint": null,            // R2/B2/MinIO override; null = AWS default
  "useTls": true
}
```

Reading the provider config does **not** require keychain access -
it's safe to cache in `app_settings`. Reading the access key/secret
does require keychain access; the cloud impl fetches them on every
`put_blob` (mirrors how `restore_now_with` fetches the device private
key fresh).

### What the user does to set this up

A new Settings -> Backup pane subsection: "Cloud destination". Two
flows:

1. **Provider preset**: dropdown of common providers (S3, R2, GCS,
   B2, MinIO). Each preset prefills `endpoint`, `region`, `useTls`.
2. **Custom**: free text for endpoint + region.

Then the user pastes their access key + secret. We do a `HEAD bucket`
sanity probe before saving, so a typo in the bucket name surfaces
immediately rather than at first backup.

The keychain entries are created on save. The user never sees the
secret again - editing reveals empty fields, with a "rotate
credentials" button that wipes both keychain entries and re-prompts.

---

## 16.3 - Multi-device key migration

Phase 15 was deliberately device-bound: the X25519 private key never
leaves the OS keychain. That works for a single-Mac user; cloud
backup makes "another device" plausible.

Three options, in increasing complexity:

### Option A: device-bound only, ship as-is (recommended for v1)

A user with two Macs has two backups in the same bucket, encrypted
with two different keys. Restore works on each device independently.
No cross-device restore. The user's mental model: "this is my
laptop's backup; that is my desktop's backup".

This is the **honest** shape and matches how Apple's iCloud Keychain,
Bitwarden, and 1Password handle device-bound keys. Ship this first.

### Option B: opt-in recovery passphrase

Add a new setting: "Recovery passphrase (optional)". When set:

1. User types a passphrase.
2. App runs Argon2id with stored salt -> 32-byte wrapping key.
3. App fetches the device X25519 private key from keychain.
4. App AES-GCM-encrypts the private key under the wrapping key.
5. The encrypted private key lands in the cloud bucket as
   `recovery/<deviceId>.bin`.

Restore on a fresh device:

1. User installs the app on the new device, signs in to the cloud
   bucket with the same access key.
2. Lists `recovery/`, picks a device, types the passphrase.
3. App derives the wrapping key, decrypts the private key, writes it
   to the new device's keychain.
4. Backup + restore from this point onwards work normally.

Properties:

- **Forgotten passphrase** = data is gone. No reset.
- The passphrase never leaves the device; the cloud sees only the
  wrapped private key.
- Argon2id parameters live in the bundle, not the wrapped blob, so
  we can ratchet them up later.

### Option C: hardware-rooted multi-device (post-v1)

iCloud Keychain or YubiKey-rooted PRF. Out of scope for the v1 cloud
spec; revisit when there's user demand.

---

## 16.4 - Idempotency on PutObject

The orchestrator's `backup_now` is already idempotent at the
component level: if `plaintext_hash` matches, no encrypt happens.
But two devices backing up the same component to the same bucket
(if the user shares credentials between machines, which is allowed
under Option A above) could race on the same blob key.

S3, R2, and GCS all support `If-None-Match: *` headers (S3 added it
in 2024). The cloud impl should use them so a concurrent PUT to the
same key fails with `412 Precondition Failed` instead of overwriting:

```rust
self.client
    .put_object()
    .bucket(&self.bucket)
    .key(key)
    .body(body)
    .if_none_match("*")  // strict-create
    .send()
```

When the orchestrator decides to re-encrypt (file changed), it picks
a new blob path (the path is `<2-hex-of-blob-hash>/<rest>.bin`, and
the blob hash is over the ciphertext, which differs every encrypt
because of the random ephemeral key + nonce). So same-device
re-encrypts always go to a fresh key; only cross-device same-content
collisions hit the precondition, which is fine - the existing blob
is already correct.

---

## 16.5 - Retry policy

Network failures are normal. Three classes:

| Error | Retry? | Strategy |
|---|---|---|
| Transient (5xx, throttling, timeout, connection reset) | Yes | Exponential backoff with jitter: 250ms, 500ms, 1s, 2s, 4s, 8s. Max 6 attempts (~16s wall-clock). |
| Auth error (401, 403, expired token) | No - surface immediately | Push to `BackupErrorEntry` with `keychainUnavailable` kind so the UI can prompt for re-auth. |
| Logical (404 on get, 409, 412 precondition failed) | No - report | These are "the world disagrees with the orchestrator's model"; surface as a typed error so the UI can explain. |

The retry loop lives in `S3Storage` (or per-provider impl), not in
the orchestrator. The orchestrator sees a single put_blob call that
either succeeds or fails after the retry budget.

Per-pass time budget cap: 30 seconds per blob. Beyond that, fail the
component with a `Write` error and move on - the next backup pass
will re-attempt.

---

## 16.6 - Telemetry shape

Cloud backup needs three pieces of telemetry to be useful operationally:

1. **Per-pass summary** (already shipped via `BackupReport`).
   - `total`, `encrypted`, `skipped_unchanged`, `errors[]`.
   - Cloud impl extends `BackupErrorEntry.message` with HTTP status.
2. **Bytes uploaded / downloaded** (new).
   - `BackupReport.bytes_uploaded: u64`.
   - `RestoreReport.bytes_downloaded: u64`.
   - Drives a "this month: 1.2 GB / 5 GB free tier" pill in Settings.
3. **Per-provider latency p50 / p99** (new).
   - In-memory ring of last 100 PUT durations.
   - Surfaced in DiagnosticsPanel, not the headline UI.

No logs leave the device. The user opted out of telemetry per
docs/12; cloud telemetry is **also** local-only.

---

## 16.7 - What the production cloud config looks like in app_settings

```sql
-- existing keys (unchanged)
backupPublicKey       = "<hex>"
backupAutoEnabled     = true
backupLastRun         = 1736361600
projectMemoryRoots    = ["~/Development", "~"]

-- new keys (Phase 16)
cloudBackupEnabled    = true
cloudBackupProvider   = "r2"
cloudBackupBucket     = "user-aseye-backup"
cloudBackupPrefix     = "user-12345/"
cloudBackupRegion     = "auto"
cloudBackupEndpoint   = "https://abc123.r2.cloudflarestorage.com"
cloudBackupUseTls     = true
cloudBackupLastSync   = 1736362400
```

Access key + secret are **NOT** in `app_settings`. They live in the
keychain. Reading `cloudBackupEnabled = true` without matching
keychain entries is treated as "credentials missing" - the UI shows
"Cloud sync paused: re-enter credentials".

---

## 16.8 - Test plan

### Unit (against MinIO running locally as a docker container)

- `S3Storage::put_blob_then_get_round_trip` - byte-equal.
- `S3Storage::if_none_match_blocks_overwrite` - second PUT to the
  same key returns precondition failed.
- `S3Storage::retry_on_transient` - inject a 503, second attempt
  succeeds.
- `S3Storage::no_retry_on_403` - first failure surfaces immediately.

### Integration (against the developer's actual cloud)

Behind an env var (`ASEYE_CLOUD_TEST=1`) so CI doesn't need cloud
credentials:

- Backup the developer's real index to a test bucket prefix.
- Wipe the local manifest.
- Restore from cloud, byte-compare every file.
- Clean up: delete every blob under the test prefix.

### Manual (release candidate gate)

- One-button credential setup in Settings.
- Disconnect WiFi mid-backup; verify retry semantics.
- Rotate credentials; verify the old keychain entries are wiped.

---

## 16.9 - What ships in the v1 cloud release

Minimum viable scope, in order of ship priority:

1. `S3Storage` impl (covers AWS S3 + R2 + B2 + MinIO via endpoint
   override).
2. Settings -> Backup -> Cloud destination subsection.
3. Per-bucket scoped IAM credential storage in keychain.
4. Idempotent uploads with `If-None-Match: *`.
5. Bandwidth tracking in `BackupReport`.
6. Recovery passphrase (Option B from §16.3) - opt-in toggle.

Out of scope for v1, deferred:

- GCS impl (AWS SDK doesn't cover; needs `google-cloud-storage`
  crate).
- Multi-bucket support ("backup to S3 *and* R2").
- Server-side replication policies.
- Bucket lifecycle / tiering hints.
- "Restore from a teammate's backup" flow.

---

## 16.10 - Risks

1. **Provider lock-in via convenience** - If we ship S3-only, users
   depending on the app effectively depend on AWS. Mitigated by the
   provider preset list including R2/B2/MinIO from day one - the
   cost of porting the bucket between providers is `s3 sync`.
2. **Credential leak in support bundles** - DiagnosticsPanel must
   never include the cloud secret. Sanitiser already covers
   `*-key`, `*-secret`, `password`, `token`; add explicit coverage
   of `cloudBackup*` keys before shipping.
3. **Bandwidth runaway on a misconfigured auto-backup** - If a user
   enables auto-backup on a 1 GB/s connection and rapidly edits a
   100 MB file, they could burn cloud egress. Mitigated by the 5s
   debouncer (Phase 15) and a per-pass byte cap (proposed: 100 MB
   per file, 1 GB per pass) that surfaces a warning rather than
   silently uploading.
4. **Clock skew breaking idempotency** - The local-newer guard on
   restore (Phase 15) compares local mtime against backup
   `encrypted_at`. With cloud backup the `encrypted_at` reflects
   the device that uploaded, not the device restoring. Document the
   ambiguity; consider switching to a per-blob "version" counter in
   v2.
5. **Provider TOS** - Some providers prohibit using their bucket as
   a sync target without specific tier upgrades. Mitigated by
   shipping the provider list with documented "verified compatible
   with this product" entries; users picking "custom" are on their
   own.

---

## 16.11 - Summary

Phase 16 is a single new struct (`S3Storage`), three new keychain
entries (provider config + access key + secret), one new Settings
subsection, and an opt-in recovery passphrase. The encryption layer,
the orchestrator, the manifest schema, and the UI all stay
unchanged. The cost of the v1 cloud release is bounded because Phase
15 carved the trait correctly.
