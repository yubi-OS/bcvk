# Software TPM 2.0 (swtpm) for CI VMs

bcvk can attach an emulated TPM 2.0 to an ephemeral VM so CI can exercise TPM2
code paths (PCR measurements, LUKS2 PCR binding, `ConditionSecurity=measured-os`)
without physical hardware.

```bash
bcvk ephemeral run --feature tpm2-swtpm dhi.io/yubi-OS/yubiOS:latest
```

## How it works

1. bcvk launches `swtpm socket` on the QEMU side with a private state dir and a
   UNIX control socket.
2. QEMU connects via `-chardev socket` + `-tpmdev emulator` and exposes an
   arch-appropriate TPM-TIS device (`tpm-tis` on x86_64, `tpm-tis-device` on
   aarch64 `virt`).
3. The guest sees a TPM at `/dev/tpm0`.

## Requirements

- `swtpm` and `swtpm-tools` installed where QEMU runs.
- Guest image with systemd >= 261 for `systemd-tpm2-swtpm.service`
  (yubiOS base `45.20260625.0` ships `systemd-261`).

## yubiOS notes

This is **test coverage only**. The production trust anchor is the YubiKey
FIDO2 (ADR-003). See yubiOS ADR-016 §Feature 1 and the guest-side drop-in in
yubiOS PR #34. Tracking: bcvk issue #3 / BLOCKER-006.
