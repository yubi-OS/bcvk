//! Software TPM (swtpm) configuration for the QEMU emulator TPM backend.
//!
//! Provides a test-only virtual TPM 2.0 for bcvk ephemeral VMs so that
//! `/dev/tpm0` is present and TPM2 code paths (PCR measurement, LUKS2 PCR
//! binding) can be exercised in CI without physical hardware.
//!
//! yubiOS production trust anchor remains the YubiKey FIDO2 device; swtpm is
//! strictly for test coverage (see yubi-OS/bcvk#3, yubiOS ADR-016).
//!
//! This module is pure: it only builds argument vectors. Process spawning and
//! socket I/O live in `qemu.rs` (`spawn_swtpm`), per the bcvk REVIEW.md rule of
//! splitting parsers/builders from I/O.

/// Configuration for a software TPM backed by `swtpm socket`.
#[derive(Debug, Clone)]
pub struct SwtpmConfig {
    /// Path to the swtpm UNIX control socket QEMU connects to.
    pub socket_path: String,
    /// Directory holding swtpm TPM state (NVRAM).
    pub state_dir: String,
}

/// Build the `swtpm socket` command (program + args) for a TPM 2.0 emulator.
///
/// QEMU connects to the control socket via `-chardev socket,path=<socket>`.
pub fn swtpm_socket_command(config: &SwtpmConfig) -> (String, Vec<String>) {
    let args = vec![
        "socket".to_string(),
        "--tpm2".to_string(),
        "--tpmstate".to_string(),
        format!("dir={}", config.state_dir),
        "--ctrl".to_string(),
        format!("type=unixio,path={}", config.socket_path),
        "--flags".to_string(),
        "startup-clear".to_string(),
    ];
    ("swtpm".to_string(), args)
}

/// Build the QEMU arguments wiring the swtpm emulator backend into the VM.
///
/// Returns a flat list of alternating flag/value arguments:
/// `-chardev socket,...  -tpmdev emulator,...  -device <model>,tpmdev=tpm0`.
///
/// The TPM device model is architecture-dependent:
/// - x86_64 (and default): `tpm-crb` (TPM2-only CRB interface on q35)
/// - aarch64: `tpm-tis-device` (MMIO TIS on the `virt` machine; CRB is x86-only)
pub fn qemu_tpm_args(socket_path: &str, arch: &str) -> Vec<String> {
    let device_model = match arch {
        "aarch64" => "tpm-tis-device",
        _ => "tpm-crb",
    };
    vec![
        "-chardev".to_string(),
        format!("socket,id=chrtpm,path={socket_path}"),
        "-tpmdev".to_string(),
        "emulator,id=tpm0,chardev=chrtpm".to_string(),
        "-device".to_string(),
        format!("{device_model},tpmdev=tpm0"),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> SwtpmConfig {
        SwtpmConfig {
            socket_path: "/run/inner-shared/swtpm.sock".to_string(),
            state_dir: "/run/swtpm-state".to_string(),
        }
    }

    #[test]
    fn test_swtpm_socket_command() {
        let (program, args) = swtpm_socket_command(&cfg());
        assert_eq!(program, "swtpm");
        assert_eq!(args[0], "socket");
        assert!(args.contains(&"--tpm2".to_string()));
        assert!(args.contains(&"type=unixio,path=/run/inner-shared/swtpm.sock".to_string()));
        assert!(args.contains(&"dir=/run/swtpm-state".to_string()));
    }

    #[test]
    fn test_qemu_tpm_args_device_model_per_arch() {
        let cases = [
            ("x86_64", "tpm-crb,tpmdev=tpm0"),
            ("aarch64", "tpm-tis-device,tpmdev=tpm0"),
            ("powerpc64", "tpm-crb,tpmdev=tpm0"),
        ];
        for (arch, expected_device) in cases {
            let args = qemu_tpm_args("/tmp/swtpm.sock", arch);
            assert_eq!(args[0], "-chardev");
            assert_eq!(args[1], "socket,id=chrtpm,path=/tmp/swtpm.sock");
            assert_eq!(args[2], "-tpmdev");
            assert_eq!(args[3], "emulator,id=tpm0,chardev=chrtpm");
            assert_eq!(args[4], "-device");
            assert_eq!(args[5], expected_device);
        }
    }

    #[test]
    fn test_qemu_tpm_args_arg_count() {
        assert_eq!(qemu_tpm_args("/x.sock", "x86_64").len(), 6);
    }
}
