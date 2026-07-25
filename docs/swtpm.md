# Software TPM (swtpm) for CI VMs

bcvk can attach a software TPM 2.0 to an ephemeral VM so that `/dev/tpm0` is
present inside the guest. This lets yubiOS exercise TPM2 code paths (PCR
measurement, LUKS2 PCR binding, `ConditionSecurity=measured-os`) in CI without
physical hardware.

> yubiOS production trust anchor remains the **YubiKey FIDO2** device
> (yubiOS ADR-003). swtpm is **test-only**. Tracks yubi-OS/bcvk#3.

## Usage

```bash
bcvk ephemeral run --swtpm quay.io/fedora/fedora-bootc:45
```

Inside the VM:

```bash
ls -l /dev/tpm0 /dev/tpmrm0
systemd-analyze condition 'ConditionSecurity=measured-os'
```

## How it works

bcvk runs QEMU inside a (privileged) podman container. With `--swtpm`, bcvk:

1. starts a `swtpm socket --tpm2` process on the host side, exposing a UNIX
   control socket;
2. adds the QEMU emulator TPM backend:
   `-chardev socket,id=chrtpm,path=<sock>`
   `-tpmdev emulator,id=tpm0,chardev=chrtpm`
   `-device tpm-crb,tpmdev=tpm0` (x86_64) / `tpm-tis-device,tpmdev=tpm0` (aarch64);
3. the guest kernel `tpm_crb`/`tpm_tis` driver then enumerates the TPM and
   creates `/dev/tpm0` automatically at boot — no guest service required.

## Why the QEMU emulator device, not `systemd-tpm2-swtpm.service`

yubiOS ADR-016 §Feature 1 describes the guest-side `systemd-tpm2-swtpm.service`
(systemd v261). That service is designed for **bare-metal hosts that lack a
TPM**: it is pulled in by `systemd-tpm2-generator` in the initrd, stores TPM
NVRAM on the **EFI System Partition**, and encrypts it with the systemd-stub
boot secret (`systemd-stub(7)`).

bcvk ephemeral boot uses **direct kernel boot** (it extracts kernel+initrd from
the UKI and boots them via `-kernel`/`-initrd`), which deliberately breaks the
systemd-stub chain (see `BootMode::DirectBoot` in `bcvk-qemu`). There is no
stub boot secret and no mounted ESP, so the guest software-fallback path is not
reliable in this environment.

Giving the VM a real (virtual) TPM via the QEMU emulator backend is the
deterministic way to get `/dev/tpm0` in a VM, and is what this feature does.

## Runtime dependency

The bcvk runner environment must provide the swtpm binaries:

```bash
dnf install -y swtpm swtpm-tools   # Fedora
```
