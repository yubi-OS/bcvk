# Releasing (yubios fork)

The `yubios` branch is the yubi-OS fork line of bcvk: a pinned upstream base
plus the yubiOS patch set (swtpm/swu2f support, native-to-disk installer,
YubiKey USB passthrough, `--extra-qemu-arg`, ephemeral SSH/vsock fixes).
Its source of truth is the bcvk row in yubi-OS/yubiOS `PINNED.md`.

## Tag scheme (OMN-104)

`v<UPSTREAM_BASE>-yubios.<N>` -- e.g. `v0.18.0-yubios.1`.

- `<UPSTREAM_BASE>` is the upstream bcvk release the branch is based on
  (upstream tags plain `vX.Y.Z`), so a fork tag never masquerades as an
  upstream release: the `-yubios.<N>` pre-release suffix (SemVer 2.0.0
  section 9) always marks it as the fork line.
- `<N>` increments for each fork release cut on the same upstream base.
- Tags point at the exact commit recorded as "Pinned source commit" in
  yubi-OS/yubiOS `PINNED.md` at cut time.

## Cutting a release

1. Confirm the `yubios` branch HEAD equals the PINNED.md pinned source
   commit and that yubiOS VM e2e evidence exists for it.
2. Create the tag at that commit (unsigned until the org sets up a fork
   release signing key -- gap noted in OMN-105).
3. Dispatch `.github/workflows/yubios-release.yml` with the tag name. It
   builds linux/amd64 + linux/arm64 on per-arch hosted runners and publishes
   `bcvk-amd64`, `bcvk-arm64`, a compatibility `bcvk` (amd64) asset, and
   `SHA256SUMS` on the GitHub Release.
4. Update the yubi-OS/yubiOS `PINNED.md` bcvk row with the release tag
   (OMN-107) so consumers pin download + checksum against it.

Upstream's `release.yml` is intentionally left untouched ("keep in sync"
file); it cannot fire on this fork -- it needs upstream's GitHub App and GPG
secrets and a merged-PR-with-`release`-label event.
