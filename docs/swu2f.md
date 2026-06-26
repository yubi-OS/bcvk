# swu2f — in-guest software FIDO2/U2F for CI VMs

For [yubiOS issue #25](https://github.com/yubi-OS/yubiOS/issues/25) (post-launch):
a software FIDO2/U2F authenticator so enrollment and PAM tests run without a
physical YubiKey. Pairs with the host-side `--swtpm` support on this branch.

## Why in-guest (not a QEMU device)

swtpm can be wired host-side because QEMU has a real TPM emulator backend
(`-tpmdev emulator`). FIDO2 has no equivalent: QEMU only offers `u2f-emulated`,
which speaks **CTAP1/U2F** and does **not** implement the CTAP2 `hmac-secret`
extension. `systemd-cryptenroll --fido2-device` and LUKS2 FIDO2 unlock *require*
`hmac-secret`, so a host-emulated U2F device is unusable for the enrollment path
we need to cover.

The working approach is a userspace CTAP2 authenticator running **inside the
guest** that registers a virtual FIDO HID device via the kernel `uhid`
interface (`/dev/uhid`). libfido2 then sees it as a normal token:

- `fido2-token -L` lists it
- `systemd-cryptenroll --fido2-device=auto <luks>` enrolls against it
- `pam-u2f` authenticates against it

Known implementations: [`fidorium`](https://github.com/grawity/fidorium) and
[`passless`](https://github.com/pando85/passless) (both `/dev/uhid` + CTAP2 +
`hmac-secret`).

## Split of responsibilities

| Layer | Owns |
|---|---|
| **bcvk** (`--swu2f`) | adds `modules-load=uhid` to the guest kernel cmdline so `/dev/uhid` exists early |
| **test image** | ships the authenticator binary + a systemd unit that starts it (image side, like `swtpm`/`swtpm-tools` for swtpm) |
| **udev** | `KERNEL=="uhid", GROUP="input", MODE="0660"` so the daemon can open the node |

## Usage

```bash
bcvk ephemeral run --swu2f dhi.io/yubi-OS/yubiOS:latest
```

The guest then exposes a software FIDO2 token; tests can enroll and authenticate.

## Boot-time unlock caveat

For unlocking the **root** LUKS2 volume the authenticator must run in the
initramfs (before the disk is decrypted). For CI we target **post-boot**
enrollment/auth against a scratch LUKS2 container, which avoids the initramfs
ordering hazard. Root-unlock-in-initramfs coverage is a separate, later step.

## Scope

Test-only. Production root of trust remains the physical YubiKey FIDO2 device
(ADR-003). swu2f exists solely for hardware-free CI coverage.
