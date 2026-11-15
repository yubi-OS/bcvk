# Software U2F / FIDO2 (swu2f) for CI VMs

bcvk can give an ephemeral VM a software FIDO2/U2F authenticator so yubiOS can
exercise `pam-u2f` and `systemd-cryptenroll --fido2` code paths in CI **without
a physical YubiKey**.

> yubiOS production trust anchor remains the **YubiKey FIDO2** device
> (yubiOS ADR-003). swu2f is **test-only**. Tracks yubi-OS/yubiOS#25.

FIDO2/U2F splits into two layers, and each yubiOS consumer needs a different
one. bcvk implements the in-guest CTAP2 path (Layer 2) because that is the layer
the LUKS2 FIDO2 e2e test (#33/#20) depends on; the host-side CTAP1 path (Layer 1)
is documented as a complementary option for pam-u2f-only coverage.

## The CTAP1 vs CTAP2 split (read this first)

| Consumer                       | Protocol needed             | swu2f layer |
|--------------------------------|-----------------------------|-------------|
| `pam-u2f` login                | U2F / CTAP1                 | Layer 1 (QEMU `u2f-emulated`) |
| `systemd-cryptenroll --fido2`  | FIDO2 / CTAP2 `hmac-secret` | Layer 2 (in-guest uhid CTAP2) |

QEMU's `u2f-emulated` is backed by [libu2f-emu], which implements the **U2F
(CTAP1)** protocol only. It is fine for pam-u2f but cannot satisfy
`systemd-cryptenroll --fido2`, which requires the CTAP2 `hmac-secret` extension
to derive the LUKS2 key. Don't conflate the two.

[libu2f-emu]: https://github.com/Agnoctopus/libu2f-emu

## Layer 2 — in-guest uhid CTAP2 authenticator (FIDO2, systemd-cryptenroll) — IMPLEMENTED

`systemd-cryptenroll --fido2-device=auto` needs CTAP2 `hmac-secret`, which
libu2f-emu (and therefore QEMU `u2f-emulated`) does not provide. The portable
way to get a CTAP2 authenticator in a VM with no hardware is a **software
authenticator that creates a virtual HID device via `/dev/uhid`** inside the
guest, then enroll against it.

Unlike swtpm, this cannot be a host/QEMU-provided device — the authenticator
must run inside the guest. bcvk's only host-side responsibility is to make sure
the `uhid` kernel module is available early, via a `modules-load=` karg. That is
exactly what the `--swu2f` flag does and all that `crates/bcvk-qemu/src/swu2f.rs`
builds (it is a pure karg builder; no process/IO):

```bash
bcvk ephemeral run --swu2f quay.io/fedora/fedora-bootc:45
```

emits the guest kernel cmdline addition:

```
modules-load=uhid
```

The CTAP2 software authenticator binary itself (e.g. `fidorium` or `passless`)
must be present in the test image — image-side work, analogous to shipping
`swtpm`/`swtpm-tools` for the swtpm feature. Inside the VM:

```bash
modprobe uhid                                   # ensured by modules-load=uhid
fidorium &                                      # software CTAP2 authenticator -> /dev/uhid
fido2-token -L                                  # libfido2 enumerates the virtual token
systemd-cryptenroll --fido2-device=auto --fido2-with-client-pin=no /tmp/test.luks
```

A full e2e (#33/#20) still needs:

1. a test fixture image (`fixtures/Dockerfile.swu2f`) carrying a CTAP2 software
   authenticator + `libfido2` + `systemd-cryptenroll`;
2. the `uhid` module loaded early (provided by `--swu2f`);
3. an integration test that registers the virtual authenticator, runs
   `systemd-cryptenroll --fido2`, then reboots and unlocks.

The `bcvk-virtualization` skill also notes `virtual-fido` (Go, USB/IP via
`vhci-hcd`) as an alternative CTAP2 emulator; uhid keeps the device fully
in-guest with no USB/IP host kernel modules. Pick one in the fixture follow-up.

## Layer 1 — QEMU emulated U2F (CTAP1, pam-u2f) — NOT YET WIRED

A completely software USB HID U2F token, provided by QEMU, deterministic and
host-side exactly like swtpm; no in-guest service required. It covers the
CTAP1/pam-u2f case but, being CTAP1-only, **cannot** drive
`systemd-cryptenroll --fido2`.

Proposed QEMU args (a future host-side option, mirroring swtpm; not built by the
current `swu2f.rs`):

```
-usb -device u2f-emulated                 # ephemeral identity (default)
-usb -device u2f-emulated,dir=<setup-dir> # stable identity from a setup dir
```

A libu2f-emu *setup directory* contains `certificate.pem`, `private-key.pem`,
`counter`, and `entropy` (48 bytes). With no directory the device runs in
**ephemeral** mode and mints a single-use identity for the VM lifetime — fine
for a register+authenticate pam-u2f smoke test. Inside the VM it appears as
`/dev/hidraw*` and is usable by `fido2-token -L` / `pamu2fcfg`.

### Runtime dependency

If Layer 1 is added later, the bcvk runner environment must provide libu2f-emu
so QEMU's `u2f-emulated` device is available (QEMU built `--enable-u2f`, which
Fedora's qemu packages are):

```bash
dnf install -y libu2f-emu   # Fedora
```

## Why split it this way

bcvk ephemeral boot uses **direct kernel boot** and runs QEMU inside a
privileged podman container. The CTAP2/FIDO2 enrollment path that #33 needs is
unavoidably in-guest (no host QEMU device speaks CTAP2 `hmac-secret`), so that is
the layer bcvk wires first. The host-provided QEMU CTAP1 device (`u2f-emulated`)
is a lighter, optional add-on for pam-u2f-only coverage and is deferred so the
FIDO2 enrollment path isn't blocked on it.

## Status

Layer 2 (implemented on `feat/swtpm-ci`): `--swu2f` flag on `CommonVmOpts`,
`crates/bcvk-qemu/src/swu2f.rs` (pure `modules-load=uhid` karg builder +
`Swu2fConfig` naming the image authenticator), and the `run_ephemeral` wiring
that appends the karg. Needs `cargo build` + `cargo nextest -p bcvk-qemu` +
human Signed-off-by before merge. The guest fixture + e2e test, and the optional
Layer 1 QEMU device, are separate follow-ups. Tracks yubi-OS/yubiOS#25.
