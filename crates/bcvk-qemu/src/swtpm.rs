//! Software TPM 2.0 (swtpm) integration for ephemeral CI VMs.
//!
//! When enabled (via `--feature tpm2-swtpm`), bcvk launches an IBM `swtpm`
//! process that serves a software TPM 2.0 over a UNIX control socket, then
//! wires it into the guest QEMU as an emulated TPM-TIS device. The guest then
//! sees a hardware-like TPM at `/dev/tpm0`, which lets CI exercise TPM2 code
//! paths (PCR measurements, LUKS2 PCR binding, `ConditionSecurity=measured-os`)
//! without physical hardware.
//!
//! For yubiOS this is **test coverage only**: the production trust anchor is the
//! YubiKey FIDO2 (ADR-003). See yubiOS ADR-016 §Feature 1 and bcvk issue #3.
//!
//! # swtpm package requirement
//!
//! The host/container that runs QEMU must have `swtpm` (and `swtpm-tools`)
//! installed. [`SwtpmConfig::command`] spawns plain `swtpm`; a clear error is
//! surfaced if the binary is missing.

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use camino::{Utf8Path, Utf8PathBuf};
use color_eyre::eyre::{bail, eyre, Context};
use color_eyre::Result;
use tracing::debug;

/// QEMU chardev id used for the swtpm control socket.
pub const TPM_CHARDEV_ID: &str = "chrtpm";
/// QEMU tpmdev id used for the emulated TPM backend.
pub const TPM_DEV_ID: &str = "tpm0";

/// Configuration for a software TPM backing an ephemeral VM.
#[derive(Debug, Clone)]
pub struct SwtpmConfig {
    /// Directory holding swtpm NVRAM/state for this VM.
    pub state_dir: Utf8PathBuf,
    /// Path to the swtpm control socket QEMU connects to.
    pub socket_path: Utf8PathBuf,
}

impl SwtpmConfig {
    /// Create a config with a unique state directory + control socket under
    /// `base` (typically the system temp dir). The directory is created on disk.
    pub fn new_in(base: &Utf8Path) -> Result<Self> {
        let unique = format!("bcvk-swtpm-{}-{}", std::process::id(), monotonic_suffix());
        let state_dir = base.join(unique);
        std::fs::create_dir_all(&state_dir)
            .with_context(|| format!("creating swtpm state dir {state_dir}"))?;
        let socket_path = state_dir.join("swtpm-sock");
        Ok(Self { state_dir, socket_path })
    }

    /// Create a config rooted in the system temp directory.
    pub fn new() -> Result<Self> {
        let tmp = std::env::temp_dir();
        let tmp = Utf8PathBuf::from_path_buf(tmp)
            .map_err(|p| eyre!("non-UTF8 temp dir: {}", p.display()))?;
        Self::new_in(&tmp)
    }

    /// Build the `swtpm socket` command serving a TPM 2.0 over the control socket.
    ///
    /// `--terminate` makes swtpm exit once QEMU disconnects, keeping the process
    /// lifecycle tied to the VM.
    pub fn command(&self) -> Command {
        let mut cmd = Command::new("swtpm");
        cmd.arg("socket")
            .arg("--tpmstate")
            .arg(format!("dir={}", self.state_dir))
            .arg("--ctrl")
            .arg(format!("type=unixio,path={}", self.socket_path))
            .arg("--tpm2")
            .arg("--terminate")
            .stdout(Stdio::null())
            .stderr(Stdio::inherit());
        cmd
    }

    /// QEMU args wiring the emulated TPM into the guest for the given target arch.
    ///
    /// `arch` is typically [`std::env::consts::ARCH`] ("x86_64", "aarch64").
    pub fn qemu_args(&self, arch: &str) -> Vec<String> {
        vec![
            "-chardev".to_string(),
            format!("socket,id={TPM_CHARDEV_ID},path={}", self.socket_path),
            "-tpmdev".to_string(),
            format!("emulator,id={TPM_DEV_ID},chardev={TPM_CHARDEV_ID}"),
            "-device".to_string(),
            format!("{},tpmdev={TPM_DEV_ID}", tpm_device_for_arch(arch)),
        ]
    }
}

/// Select the QEMU TPM device model for the target architecture.
///
/// x86 uses the ISA-attached TIS device (`tpm-tis`); the aarch64 `virt` machine
/// uses the MMIO TIS device (`tpm-tis-device`).
pub fn tpm_device_for_arch(arch: &str) -> &'static str {
    match arch {
        "aarch64" | "arm" => "tpm-tis-device",
        _ => "tpm-tis",
    }
}

fn monotonic_suffix() -> u128 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

/// Block until the swtpm control socket appears, up to `timeout`.
pub fn wait_for_socket(path: &Utf8Path, timeout: Duration) -> Result<()> {
    let start = Instant::now();
    while !path.exists() {
        if start.elapsed() >= timeout {
            bail!("timed out waiting for swtpm control socket at {path} after {timeout:?}");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    debug!("swtpm control socket ready at {path}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> SwtpmConfig {
        SwtpmConfig {
            state_dir: Utf8PathBuf::from("/tmp/bcvk-swtpm-test"),
            socket_path: Utf8PathBuf::from("/tmp/bcvk-swtpm-test/swtpm-sock"),
        }
    }

    #[test]
    fn device_for_arch_table() {
        let cases = [
            ("x86_64", "tpm-tis"),
            ("aarch64", "tpm-tis-device"),
            ("arm", "tpm-tis-device"),
            ("riscv64", "tpm-tis"),
        ];
        for (arch, want) in cases {
            assert_eq!(tpm_device_for_arch(arch), want, "arch={arch}");
        }
    }

    #[test]
    fn qemu_args_structure_x86() {
        let args = cfg().qemu_args("x86_64");
        assert_eq!(
            args,
            vec![
                "-chardev".to_string(),
                "socket,id=chrtpm,path=/tmp/bcvk-swtpm-test/swtpm-sock".to_string(),
                "-tpmdev".to_string(),
                "emulator,id=tpm0,chardev=chrtpm".to_string(),
                "-device".to_string(),
                "tpm-tis,tpmdev=tpm0".to_string(),
            ]
        );
    }

    #[test]
    fn qemu_args_use_arch_device() {
        let args = cfg().qemu_args("aarch64");
        assert_eq!(args.last().unwrap(), "tpm-tis-device,tpmdev=tpm0");
    }

    #[test]
    fn command_program_is_swtpm() {
        let cmd = cfg().command();
        assert_eq!(cmd.get_program(), "swtpm");
        let args: Vec<_> = cmd.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
        assert!(args.contains(&"socket".to_string()));
        assert!(args.contains(&"--tpm2".to_string()));
        assert!(args.iter().any(|a| a.starts_with("type=unixio,path=")));
    }
}
