# Software U2F / FIDO2 (swu2f) for CI VMs

bcvk can give an ephemeral VM a software FIDO2/U2F authenticator so yubiOS can
exercise `pam-u2f` and `systemd-cryptenroll --fido2` code paths in CI **without
a physical YubiKey**.

> yubiOS production trust anchor remains the **YubiKey FIDO2** device
> (yubiOS ADR-003). swu2f is **test-only**. Tracks yubi-OS/yubiOS#25.

This mirrors the `swtpm` feature (see `docs/swtpm.md`): a host/QEMU-provided
virtual device the guest sees as real hardware. But FIDO2/U2F splits into two
layers, and each yubiOS consumer needs a different one.

## The CTAP1 vs CTAP2 split (read this first)

| Consumer                       | Protocol needed             | swu2f layer |
|--------------------------------|-----------------------------|-------------|
| `pam-u2f` login                | U2F / CTAP1                 | Layer 1 (QEMU `u2f-emulated`) |
| `systemd-cryptenroll --fido2`  | FIDO2 / CTAP2 `hmac-secret` | Layer 2 (in-guest uhid CTAP2) |

QEMU's `u2f-emulated` is backed by [libu2f-emu], which implements the **U2F
(CTAP1)** protocol only. It is perfect for pam-u2f but cannot satisfy
`systemd-cryptenroll --fido2`, which requires the CTAP2 `hmac-secret` extension
to derive the LUKS2 key. Don't conflate the two.

[libu2f-emu]: https://github.com/Agnoctopus/libu2f-emu

## Layer 1 — QEMU emulated U2F (CTAP1, pam-u2f)

A completely software USB HID U2F token, provided by QEMU. Deterministic and
host-side, exactly like swtpm; no in-guest service required.

Proposed usage (mirrors `--swtpm`):

```bash
bcvk ephemeral run --swu2f quay.io/fedora/fedora-bootc:45
```

QEMU args emitted (see `crates/bcvk-qemu/src/swu2f.rs`):

```
-usb -device u2f-emulated                 # ephemeral identity (default)
-usb -device u2f-emulated,dir=<setup-dir> # stable identity from a setup dir
```

A libu2f-emu *setup directory* contains `certificate.pem`, `private-key.pem`,
`counter`, and `entropy` (48 bytes). With no directory the device runs in
**ephemeral** mode and mints a single-use identity for the VM lifetime — fine
for a register+authenticate pam-u2f smoke test.

Inside the VM:

```bash
ls -l /dev/hidraw*        # the emulated token appears as a HID security key
fido2-token -L            # libfido2 enumerates it
pamu2fcfg                 # register it for pam-u2f
```

### Runtime dependency

The bcvk runner environment must provide libu2f-emu so QEMU's `u2f-emulated`
device is available (QEMU must be built `--enable-u2f`, which Fedora's qemu
packages are):

```bash
dnf install -y libu2f-emu   # Fedora
```

## Layer 2 — in-guest uhid CTAP2 authenticator (FIDO2, systemd-cryptenroll)

`systemd-cryptenroll --fido2-device=auto` needs CTAP2 `hmac-secret`, which
libu2f-emu does not provide. The portable way to get a CTAP2 authenticator in a
VM with no hardware is a **software authenticator that creates a virtual HID
device via `/dev/uhid`** inside the guest, then enroll against it:

```bash
systemd-cryptenroll --fido2-device=auto --fido2-with-client-pin=no /tmp/test.luks
```

This is guest-image work, not a QEMU device, so it is intentionally **not** in
the `swu2f.rs` arg builder. It needs:

1. a test fixture image (`fixtures/Dockerfile.swu2f`) carrying a CTAP2 software
   authenticator (e.g. a uhid-based libfido2 virtual device) + `libfido2` +
   `systemd-cryptenroll`;
2. `modules-load` for `uhid`;
3. an integration test that registers the virtual authenticator, runs
   `systemd-cryptenroll --fido2`, then reboots and unlocks.

The `bcvk-virtualization` skill also notes `virtual-fido` (Go, USB/IP via
`vhci-hcd`) as an alternative CTAP2 emulator; uhid keeps the device fully
in-guest with no USB/IP host kernel modules. Pick one in the follow-up PR.

## Why split it this way

Same rationale as swtpm: bcvk ephemeral boot uses **direct kernel boot** and
runs QEMU inside a privileged podman container. A host-provided QEMU HID device
(`u2f-emulated`) is the deterministic, dependency-light path and covers the
CTAP1/pam-u2f case immediately. The CTAP2/FIDO2 enrollment path is heavier
(needs a guest authenticator) and is staged as Layer 2 so pam-u2f coverage
isn't blocked on it.

## Status

Layer 1: QEMU-arg builder + flag spec landed on `feat/swtpm-ci` (this doc +
`swu2f.rs`). The `--swu2f` CLI flag and `QemuConfig` wiring follow the exact
`--swtpm` pattern and land in the same wiring PR (needs `cargo build` +
`cargo nextest` + human Signed-off-by before merge). Layer 2 is a separate
guest-image PR. Tracks yubi-OS/yubiOS#25.
