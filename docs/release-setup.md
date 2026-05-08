# Release Setup

This document captures the one-time human steps required before the
auto-update pipeline (Phase 6.2 wiring + Phase 6.3 release workflow)
can ship signed bundles to end-user installs.

The Rust backend is already wired (`apps/desktop/src-tauri/src/ipc/updates.rs`,
`apps/desktop/src-tauri/Cargo.toml`, `apps/desktop/src-tauri/src/lib.rs`).
The release workflow already expects the secrets listed below
(`.github/workflows/release.yml`). What remains is **generating the
Tauri Updater Ed25519 keypair** and **placing the public key** in
`tauri.conf.json`. Until that happens, every signed update will fail
signature verification on the user's machine and the install step
will refuse to proceed.

## Required user action: generate the Tauri signing keypair

Tauri Updater uses a per-app Ed25519 signing keypair. We sign the
`latest-stable.json` / `latest-beta.json` manifests with the private
key in CI; the running app verifies the signature against the public
key embedded at build time.

**This is a one-time step. The same keypair signs both the stable
and beta channels (see `docs/12-security.md` "Update channel
separation").**

### 1. Generate the keypair

From a trusted developer machine:

```bash
pnpm tauri signer generate -w ~/.tauri/aseye.key
```

Choose a **non-empty** passphrase when prompted. Tauri reads
`TAURI_SIGNING_PRIVATE_KEY_PASSWORD` unconditionally, so an empty
passphrase still has to be set as the empty string in the secret.

The command emits two files:
- `~/.tauri/aseye.key`     - the encrypted private key (PEM-ish format).
- `~/.tauri/aseye.key.pub` - the public key (base64-encoded Ed25519).

### 2. Place the public key in `tauri.conf.json`

Open `apps/desktop/src-tauri/tauri.conf.json` and replace the
placeholder under `plugins.updater.pubkey`:

```json
"plugins": {
  "updater": {
    "endpoints": [...],
    "pubkey": "<contents of ~/.tauri/aseye.key.pub>",
    "dialog": false
  }
}
```

The pubkey value is a single line (base64 + a small header). Commit
this change. The public key being in source is not a leak; it is
designed to be public.

### 3. Add the private key to GitHub Actions secrets

Two secrets at the repository level:

- `TAURI_SIGNING_PRIVATE_KEY` - the **contents** of `~/.tauri/aseye.key`.
  Either paste it as-is (newlines preserved) or as a single line with
  `\n` literals. Both forms are accepted by the action.
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` - the passphrase you chose
  during keygen (set to empty string if no passphrase).

Once both secrets are present, `Release` workflow runs will pass the
"Fail fast if updater signing secret is missing" guard at the start
of every matrix leg.

### 4. Verify

Tag a beta release:

```bash
git tag v0.0.1-beta.1 -m "First beta with signed updater"
git push origin v0.0.1-beta.1
```

The `Release` workflow should:

- Build, sign, notarise, GPG-sign on each matrix leg.
- Emit `latest-beta.json` from the `publish` job.
- Attach the manifest plus all bundles to the GitHub Release.

The first install on a fresh machine and the first auto-update from
that beta to `v0.0.1-beta.2` will exercise the end-to-end path. If
the second install fails with "signature mismatch", the public key
in `tauri.conf.json` does not match the private key in CI; rotate
the public key and ship a new version.

## Rotation policy

Per `.github/workflows/release.yml` and `docs/12-security.md`, ANY
rotation of `TAURI_SIGNING_PRIVATE_KEY` forces every existing install
to fail signature verification on its next update poll. A rotation
must therefore:

1. Ship a new app version that embeds the new public key.
2. Keep both keys signing in parallel for at least one full release
   cycle so users on the previous version can still update.
3. Track the rotation in an `INV` document under
   `docs/investigations/release-pipeline/`.

Treat `TAURI_SIGNING_PRIVATE_KEY` as the highest-impact secret in the
project. The `APPLE_*`, `WINDOWS_*`, and `GPG_*` secrets are all
free to rotate; this one is not.

## Where the IPC contract lives

The frontend consumes the Phase 6.2 IPC surface:

| Command | Purpose |
|---------|---------|
| `check_for_update()` | One-shot manual check; returns `Option<UpdateAvailable>`. |
| `install_update_and_relaunch()` | Download + install + restart. |
| `get_update_channel()` / `set_update_channel(channel)` | Switch stable/beta. |
| `get_auto_check_setting()` / `set_auto_check_setting(enabled)` | Disable the daily check. |

A `update-available` Tauri event is broadcast from the daily background
poll spawned in `lib.rs::run`. Generated TS bindings:

- `apps/desktop/src-tauri/bindings/updates/UpdateAvailable.ts`
- `apps/desktop/src-tauri/bindings/updates/UpdateChannel.ts`
- `apps/desktop/src-tauri/bindings/updates/UpdateError.ts`

Re-exported from `packages/shared-types/src/index.ts`.
