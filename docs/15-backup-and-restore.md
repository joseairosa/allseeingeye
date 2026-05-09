# 15 - Backup & Restore

Status: PENDING

This phase ships an end-to-end backup + restore round-trip for every
indexed component file. The cryptography is designed once here; the
storage backend is intentionally swappable so today's localhost
"cloud" can become an S3 / R2 / GCS upload without changing the
encryption layer.

The core promise: **the storage backend never sees plaintext, ever**.
Even if the entire backup directory is stolen, the attacker reads
ciphertext and a public key. They cannot decrypt without the user's
private key, which never leaves the OS keychain.

---

## 15.1 - Threat model

| Surface | What an attacker can read | What they cannot read |
|---------|---------------------------|------------------------|
| Backup directory contents (filesystem read) | Encrypted blobs, public key, manifest metadata (paths, hashes, mtimes). | Plaintext file contents, the symmetric DEKs that protect them. |
| `index.sqlite` leak | Component metadata + the cached **public** key. | Plaintext of any backed-up file. |
| Network sniffer on a future production deployment | TLS-protected ciphertext blobs. | TLS protects transport; envelope encryption protects at-rest. |
| Keychain compromise (attacker has the device unlocked + signed in as the user) | Everything. The private key is in OS keychain so a fully-compromised user account is game over. This is intentional and consistent with how passkeys / SSH keys work. | (n/a - this is the trust boundary) |
| Cross-device restore without the original device | Nothing. Device-bound private key is not exportable in v0. | Plaintext - by design. |

### Why this shape

The user's `~/.claude/CLAUDE.md`, `AGENTS.md`, and similar files often
contain credentials, tokens, project specifics, and sensitive prose.
Treating the backup like an opaque blob protects against both
malicious storage providers and supply-chain compromises of whatever
service hosts the backups in production.

---

## 15.2 - Key custody

### Algorithm choice

- **X25519** for the device long-term keypair (used in ECDH for
  wrapping symmetric DEKs).
- **AES-256-GCM** for symmetric file encryption (DEK encrypts file
  bytes).
- **HKDF-SHA-256** to derive a wrapping key from each ECDH agreement.

X25519 + AES-GCM is the same family modern HPKE (RFC 9180) uses; we
implement a minimal subset rather than pulling in the full HPKE
crate.

### Lifecycle

```
First launch
  └─ Tauri command `ensure_backup_keypair` runs once on app boot
       ├─ If keychain entry "dev.allseeingeye.backup-key" exists:
       │     load private key, recompute public key, cache public
       │     in app_settings.backupPublicKey for fast wraps
       │
       └─ If absent:
             generate fresh X25519 keypair
             write private key to keychain
             write public key (32 bytes hex) to app_settings
```

The private key is **always** retrieved fresh from the keychain at
the moment of unwrap; we never cache it in process memory across
operations. The public key is fine to cache because compromising it
gains nothing.

### Keychain integration

Use the `keyring` Rust crate (cross-platform: macOS Keychain, Windows
Credential Manager, Linux libsecret/kwallet). Service name
`dev.allseeingeye.backup` and account name `device-key`. Stored value
is the raw 32-byte X25519 private key encoded as hex (the `keyring`
crate stores strings).

### What the user never sees

No master passphrase. No recovery code. No "re-enter your password".
The app handles everything transparently against the device-bound
key. This is the explicit trade: simplicity now, no cross-device
restore until v1 ships an opt-in recovery passphrase flow.

---

## 15.3 - Envelope encryption

Every backed-up file follows the same pattern:

```
plaintext file bytes
   │
   ├─ generate random 32-byte DEK (data encryption key)
   ├─ AES-256-GCM(DEK, plaintext, 12-byte nonce) -> ciphertext + tag
   │
   ├─ generate ephemeral X25519 keypair (eph_priv, eph_pub)
   ├─ shared = X25519-ECDH(eph_priv, device_pub)
   ├─ wrapping_key = HKDF-SHA256(shared, info="aseye-backup-wrap-v1", salt=device_pub || eph_pub)
   ├─ AES-256-GCM(wrapping_key, DEK, 12-byte nonce) -> wrapped_DEK + wrap_tag
   │
   └─ blob = magic + version + eph_pub + nonce_wrap + wrapped_DEK + wrap_tag + nonce_data + ciphertext + data_tag
```

### Blob format on disk (v1)

```
Offset  Size   Field
------  -----  -----
0       4      Magic bytes: "ASBV"  (All-Seeing-eye Backup Volume)
4       4      Version u32 LE = 1
8       32     Ephemeral X25519 public key
40      12     AES-GCM nonce for wrapped DEK
52      32     Wrapped DEK (encrypted DEK bytes)
84      16     AES-GCM tag for wrapped DEK
100     12     AES-GCM nonce for file data
112     N      Ciphertext (length matches plaintext_size)
112+N   16     AES-GCM tag for file data
```

A future format version uses a different magic value or bumps the
version field; the loader treats unknown shapes as "incompatible
version, surface a clear error" rather than guessing.

### Why fresh DEK + ephemeral key per file

- Two files with identical plaintext produce different ciphertext.
- Forward secrecy at the wrapping layer: even if the device key is
  compromised tomorrow, today's blobs stay safe against an attacker
  who only has the storage backend and not the keychain.
- Cheap: each file backup costs one X25519 keygen, one ECDH, two
  AES-GCM operations. Negligible vs. file IO.

### Symmetric for-the-record only

We do **not** sign blobs. The AES-GCM tag serves as the integrity
check; if a blob is tampered with the unwrap fails closed and the
restore path skips that file with a logged error. Adding signatures
would mean Ed25519 + a signing-key keychain entry, which is more
moving parts for protection we already have at the encryption layer.

---

## 15.4 - Local storage layout

```
~/.aseye-backup/
├── manifest.json                 # version, created_at, public_key_hex
├── blobs/
│   ├── ab/
│   │   └── abcdef0123...bin      # blob_path = <first 2 hex of blob_hash>/<rest>
│   ├── cd/
│   └── ...
└── trash/                         # optional: retired blobs await GC
```

The directory is created on first backup. `~` resolves via
`dirs::home_dir()`. The single-directory layout means production
swap = replace the writer with an S3 `PutObject` / R2 `Put`; the
key-by-blob-hash naming maps cleanly to object keys.

### What lives in `manifest.json`

A small text file (no encryption needed - it carries metadata only,
no plaintext):

```json
{
  "version": 1,
  "created_at": 1736361600,
  "device_public_key_hex": "abcdef..."
}
```

The per-file manifest lives in SQLite (`backup_manifest` table), not
on disk, so we get atomic writes for free.

### SQLite manifest table

```sql
CREATE TABLE backup_manifest (
  component_id    TEXT PRIMARY KEY,
  blob_path       TEXT NOT NULL,            -- relative to ~/.aseye-backup/blobs/
  plaintext_hash  TEXT NOT NULL,            -- SHA-256 of original file bytes
  blob_hash       TEXT NOT NULL,            -- SHA-256 of the encrypted blob
  plaintext_size  INTEGER NOT NULL,
  blob_size       INTEGER NOT NULL,
  encrypted_at    INTEGER NOT NULL,         -- unix seconds
  FOREIGN KEY (component_id) REFERENCES component(id) ON DELETE CASCADE
);

CREATE INDEX idx_backup_manifest_encrypted_at ON backup_manifest(encrypted_at);
```

### Idempotency

Re-backing-up a file whose `plaintext_hash` matches the manifest is a
no-op. The orchestrator computes the SHA-256 of the file content
before deciding whether to re-encrypt. This makes "Backup now"
cheap when nothing changed, which matters for the auto-after-edit
trigger.

---

## 15.5 - Backup orchestration

### Triggers

1. **Manual**: `Settings > Backup > Backup now` button fires
   `backup_now()` IPC. Reports per-file outcome.
2. **Auto after edit**: a debounced listener subscribes to the
   pipeline event stream. On `componentUpserted` events, schedule a
   backup for that component_id. Coalesce 5 seconds of changes into
   a single sweep so a 50-keystroke edit produces one backup, not
   fifty.

### `backup_now` flow

```
for each row in component table:
   if row.id has no manifest entry:
       encrypt + write blob + insert manifest row
   else if file SHA-256 changed since manifest.plaintext_hash:
       encrypt + write blob + update manifest row + move old blob to trash/
   else:
       skip (idempotent no-op)

emit BackupReport { total, encrypted, skipped, errors[] }
```

### Errors are non-fatal

Per-component failures (file unreadable, disk full, etc.) are
collected into `BackupReport.errors[]` and surfaced in the UI. The
sweep keeps going so one bad row does not abort the whole pass.

---

## 15.6 - Restore orchestration

### Trigger

Manual only for v0. `Settings > Backup > Restore now` opens a strong
confirm dialog because restore overwrites local files.

### `restore_now` flow

```
for each entry in backup_manifest:
   if local file at component.path is newer than manifest.encrypted_at:
       skip with reason "local_newer" (do not overwrite the user's work)
   else:
       fetch keychain -> private key
       read blob from disk
       unwrap DEK -> decrypt ciphertext
       atomic write to component.path
       record success

emit RestoreReport { total, restored, skipped_local_newer, errors[] }
```

### Safety gates

- **Local-newer check**: if the file on disk has an mtime greater
  than the backup's `encrypted_at`, skip rather than overwrite.
  Restore should not destroy work the user did since the backup.
- **Dry-run mode**: `restore_now({ dry_run: true })` reports what
  would happen without writing anything. Surfaced behind a "Preview
  what restore would do" link in the UI.
- **Atomic writes**: use the existing `safe_atomic_write_with_options`
  so a partial restore never leaves a half-written file.

### What restore does NOT do

- Restore does not re-create directories beyond what the original
  paths require. If a project root no longer exists, that file's
  restore fails with `path_unreachable` and the rest continue.
- Restore does not migrate paths. A backup taken on machine A with
  `/Users/alice/.claude/CLAUDE.md` will write back to that exact
  path on the same machine; cross-device restore is out of scope
  for v0 (see 15.2).

---

## 15.7 - IPC surface

```rust
// commands.rs
#[tauri::command]
pub async fn backup_now() -> Result<BackupReport, String>;

#[tauri::command]
pub async fn restore_now(dry_run: bool) -> Result<RestoreReport, String>;

#[tauri::command]
pub fn backup_status() -> Result<BackupStatus, String>;

#[tauri::command]
pub fn backup_set_auto(enabled: bool) -> Result<(), String>;
```

### Returned types

```rust
pub struct BackupReport {
    pub total: u32,
    pub encrypted: u32,
    pub skipped_unchanged: u32,
    pub errors: Vec<BackupError>,
    pub elapsed_ms: u64,
}

pub struct BackupError {
    pub component_id: String,
    pub kind: BackupErrorKind, // Read | Encrypt | Write | KeychainUnavailable
    pub message: String,
}

pub struct RestoreReport {
    pub total: u32,
    pub restored: u32,
    pub skipped_local_newer: u32,
    pub errors: Vec<RestoreError>,
    pub elapsed_ms: u64,
    pub dry_run: bool,
}

pub struct BackupStatus {
    pub key_present: bool,
    pub manifest_count: u32,
    pub last_backup_at: Option<i64>,
    pub auto_backup_enabled: bool,
    pub backup_dir: String,
}
```

---

## 15.8 - UI

### Settings -> Backup pane

```
┌─────────────────────────────────────────────────────────────┐
│ Backup                                                      │
├─────────────────────────────────────────────────────────────┤
│ Status                                                      │
│   Backed up: 142 / 142 components                           │
│   Last backup: 5 minutes ago                                │
│   Storage: ~/.aseye-backup/  (4.2 MB)                       │
│   Encryption: device-bound X25519, AES-256-GCM              │
│                                                             │
│ Actions                                                     │
│   [Backup now]  [Restore now...]  [Show preview]            │
│                                                             │
│ Auto-backup                                                 │
│   ☑ Backup automatically after edits                        │
│   (debounced 5s; no network in v0)                          │
│                                                             │
│ Footer                                                      │
│   Cross-device restore is not supported in v0. The private  │
│   key never leaves this Mac's keychain. See docs/15 for     │
│   the threat model.                                         │
└─────────────────────────────────────────────────────────────┘
```

### Status banner during a backup

A small toast at the top of the view: `Backing up... 23 / 142`. On
completion: `Backed up 23 files in 480ms`. On failure with errors:
`Backed up 22, 1 error - click for details`.

---

## 15.9 - Tests

### Unit (Rust)

- `envelope::encrypt_then_decrypt_roundtrip` - random plaintext,
  random DEK, full envelope round-trip with a fresh device keypair.
- `envelope::tamper_with_ciphertext_fails_open` - flip a single byte
  in the ciphertext, assert decrypt errors.
- `envelope::tamper_with_wrap_fails_open` - same for the wrapped DEK.
- `envelope::wrong_device_key_fails_open` - decrypt with a different
  X25519 key, assert decrypt errors.
- `keychain::ensure_keypair_idempotent` - calling `ensure_keypair`
  twice returns the same public key (gated on a CI flag because the
  test mutates a real keychain entry).
- `manifest::idempotent_backup` - run backup twice with no file
  changes; second run reports `encrypted=0, skipped_unchanged=N`.
- `restore::skips_local_newer` - touch a local file post-backup,
  run restore, assert the file is not overwritten.

### Integration (against real keychain + real home)

- `backup_then_restore_roundtrip` - back up the developer's actual
  indexed components into a tempdir, then restore into another
  tempdir, byte-compare every file. Gated on
  `~/Library/Application Support/AllSeeingEye/index.sqlite` existing.

### Frontend (vitest)

- BackupPane renders status when `backup_status` returns counts.
- "Backup now" disables while the mutation is in flight; status
  updates after success.
- "Restore now" surfaces the confirm dialog before firing the IPC.

---

## 15.10 - Production migration path

The encryption layer is the contract. Replacing local storage with a
real cloud target = swapping one trait implementation:

```rust
trait BackupStorage {
    fn put_blob(&self, blob_hash: &str, bytes: &[u8]) -> Result<()>;
    fn get_blob(&self, blob_hash: &str) -> Result<Vec<u8>>;
    fn list_blobs(&self) -> Result<Vec<String>>;
    fn delete_blob(&self, blob_hash: &str) -> Result<()>;
}
```

v0 implementation: `LocalDirectoryStorage` (filesystem under
`~/.aseye-backup/`). v1 candidate: `S3Storage` or `R2Storage`. The
manifest stays in SQLite either way (cheap, atomic, queryable).

Production also needs:

- Multi-device key migration (recovery passphrase opt-in).
- Server-side rate-limiting on PUT to prevent a malicious app from
  burning the user's bandwidth.
- Per-user usage quotas surfaced in the UI.
- A "delete my backups" flow with cryptographic proof of intent.

All of this is post-v0 work. The v0 spike proves the use case
without committing to any of those decisions.

---

## 15.11 - Risks

1. **Keychain unavailability** - Linux without libsecret / kwallet.
   Mitigated by detection + a clear error when keychain access
   fails; the backup pane disables the buttons with copy explaining
   the missing dependency.
2. **Free space exhaustion** - encrypted blobs are roughly the same
   size as plaintext + 80 bytes of crypto header. We cap auto-backup
   at the `~/.aseye-backup/` directory's free space; the manual
   button surfaces a clear error if a single file write fails.
3. **Concurrency** - two backup passes running at once would race on
   the manifest. A single mutex on the backup module serialises
   passes; the auto-debouncer naturally coalesces concurrent
   triggers.
4. **Clock skew on local-newer check** - `mtime` is compared against
   `encrypted_at`. A user with a clock that jumped backward could
   skip restoring a file they wanted. Documented; the dry-run mode
   gives them a way to see and fix it before committing.

---

## 15.12 - Out of scope (filed as follow-ups)

- Cross-device restore (recovery passphrase, key export).
- Differential backups (only changed bytes uploaded).
- Versioning (keeping the last N backups of a file).
- Server-side dedup beyond the local idempotency check.
- Backup of the SQLite index itself (re-deriving via re-scan is
  cheaper than encrypting + uploading the DB).
- Backup of session transcripts under `~/.claude/projects/`.

---

## 15.13 - Specialist dispatch

| Phase | Owner | Depends on |
|-------|-------|------------|
| 15.A backend (envelope, keychain, manifest) | aseye-rust-backend | none |
| 15.B IPC + auto-debouncer | aseye-rust-backend | 15.A |
| 15.C Settings BackupPane UI | aseye-frontend-features | 15.B |
| 15.D Integration test against real home | aseye-rust-backend | 15.A + 15.B |

15.A and 15.C can run in parallel against a stub IPC; final wiring
lands when 15.B exposes the real commands.
