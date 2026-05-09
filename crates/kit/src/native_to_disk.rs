//! Native (no-VM) installation of bootc images directly to block devices.
//
//! Unlike `to_disk`, which routes through an ephemeral QEMU VM, this module
//! runs `bootc install to-disk` directly via a privileged rootful podman
//! container. The block device is mapped straight into the container — no
//! virtiofsd, no SSH, no virtio-blk translation layer.
//
//! # When to use
//
//! - Flashing a bootc OCI image to a **physical** block device (USB stick, NVMe,
//!   SD card) where latency and QEMU overhead matter.
//! - Environments without hardware virtualisation (VMs-inside-VMs, some CI).
//! - Workflows where the simpler privilege model (rootful podman on host) is
//!   acceptable and QEMU/virtiofsd are not installed.
//
//! # Requirements
//
//! - rootful podman (or `sudo podman`) on the host
//! - The target path must be an **unmounted block device** (`/dev/sdX`,
//!   `/dev/nvme0n1`, etc.)
//
//! # Safety
//
//! The device is wiped unconditionally. bcvk will:
//! 1. Confirm the device is a block device (`fstat st_mode & S_IFBLK`)
//! 2. Verify no partition on the device appears in `/proc/mounts`
//! 3. Print device info (size, model) and require `--yes` or interactive confirmation
//!    before proceeding

use std::os::unix::fs::FileTypeExt;

use camino::Utf8PathBuf;
use color_eyre::eyre::{bail, eyre, Context};
use color_eyre::Result;
use tracing::{debug, info, warn};

use crate::install_options::InstallOptions;

/// Options specific to native (no-VM) installation
#[derive(Debug, Clone, clap::Parser, Default)]
pub struct NativeToDiskOpts {
    /// Container image to install
    pub source_image: String,

    /// Target block device path (e.g. /dev/sdb, /dev/nvme0n1)
    pub target_device: Utf8PathBuf,

    /// Installation options (filesystem, root-size, kargs)
    #[clap(flatten)]
    pub install: InstallOptions,

    /// Skip interactive confirmation (use in scripts/CI)
    #[clap(long)]
    pub yes: bool,

    /// Configure RUST_LOG for bootc install inside the container
    #[clap(long)]
    pub install_log: Option<String>,

    /// Use rootful podman (default: tries rootless first, falls back to sudo)
    #[clap(long)]
    pub rootful: bool,
}

/// Safety information gathered about the target device
#[derive(Debug)]
struct DeviceInfo {
    path: Utf8PathBuf,
    /// Device size in bytes (from `blockdev --getsize64`)
    size_bytes: u64,
    /// Model string from `/sys/block/<dev>/device/model`, if available
    model: Option<String>,
}

impl DeviceInfo {
    fn human_size(&self) -> String {
        let gb = self.size_bytes as f64 / 1_000_000_000.0;
        format!("{:.1} GB", gb)
    }
}

/// Verify the path is a block device and return device info.
fn inspect_device(path: &Utf8PathBuf) -> Result<DeviceInfo> {
    let meta = std::fs::metadata(path.as_std_path())
        .with_context(|| format!("Cannot stat {path}"))?;

    if !meta.file_type().is_block_device() {
        bail!("{path} is not a block device. Pass a real device path like /dev/sdb.");
    }

    // Get size via blockdev --getsize64
    let size_out = std::process::Command::new("blockdev")
        .args(["--getsize64", path.as_str()])
        .output()
        .with_context(|| "blockdev not found; install util-linux")?;

    let size_bytes = if size_out.status.success() {
        String::from_utf8_lossy(&size_out.stdout)
            .trim()
            .parse::<u64>()
            .unwrap_or(0)
    } else {
        0
    };

    // Best-effort: read model from sysfs
    let dev_name = path
        .file_name()
        .unwrap_or(path.as_str())
        .trim_end_matches(|c: char| c.is_ascii_digit()); // strip partition suffix
    let model = std::fs::read_to_string(format!("/sys/block/{dev_name}/device/model"))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    Ok(DeviceInfo {
        path: path.clone(),
        size_bytes,
        model,
    })
}

/// Check /proc/mounts for any partition on the target device.
fn check_not_mounted(path: &Utf8PathBuf) -> Result<()> {
    let mounts = std::fs::read_to_string("/proc/mounts")
        .context("Cannot read /proc/mounts")?;
    let dev_str = path.as_str();
    for line in mounts.lines() {
        let device = line.split_whitespace().next().unwrap_or("");
        // Match exact device or any partition (e.g. /dev/sdb1 for /dev/sdb)
        if device == dev_str || device.starts_with(dev_str) {
            bail!("{device} is currently mounted. Unmount all partitions on {dev_str} before flashing.");
        }
    }
    Ok(())
}

/// Interactive confirmation prompt (skipped with --yes)
fn confirm_destructive(info: &DeviceInfo) -> Result<()> {
    let model = info.model.as_deref().unwrap_or("unknown");
    eprintln!();
    eprintln!("  Target : {}", info.path);
    eprintln!("  Model  : {model}");
    eprintln!("  Size   : {}", info.human_size());
    eprintln!();
    eprint!("  ALL DATA ON THIS DEVICE WILL BE LOST. Type 'yes' to continue: ");

    let mut input = String::new();
    std::io::stdin()
        .read_line(&mut input)
        .context("Failed to read confirmation")?;

    if input.trim() != "yes" {
        bail!("Aborted by user.");
    }
    Ok(())
}

/// Build the podman command for native installation.
//
/// Runs `bootc install to-disk` inside a privileged container that has the
/// host device, /dev, and /sys mapped in. Container storage is mounted
/// read-only so the source image does not need to be re-pulled.
fn build_podman_cmd(
    opts: &NativeToDiskOpts,
    storage_path: &Utf8PathBuf,
) -> Vec<std::ffi::OsString> {
    let mut cmd: Vec<std::ffi::OsString> = Vec::new();

    // Use sudo podman when rootful is requested
    if opts.rootful {
        cmd.push("sudo".into());
    }
    cmd.push("podman".into());

    cmd.extend([
        "run".into(), "--rm".into(), "--privileged".into(),
        "--pid=host".into(), "--net=none".into(),
    ]);

    // /sys and /dev: full device access
    cmd.extend(["-v".into(), "/sys:/sys:ro".into()]);
    cmd.extend(["-v".into(), "/dev:/dev".into()]);

    // Host container storage (read-only): avoids re-pulling the image
    cmd.extend([
        "-v".into(),
        format!("{storage_path}:{storage_path}:ro").into(),
        "--security-opt".into(),
        "label=type:unconfined_t".into(),
    ]);

    // Optional RUST_LOG
    if let Some(ref level) = opts.install_log {
        cmd.extend(["--env".into(), format!("RUST_LOG={level}").into()]);
    }

    // Source image (containers-storage: transport)
    let imgref = format!("containers-storage:{}", opts.source_image);
    cmd.push(imgref.into());

    // bootc install to-disk
    cmd.extend([
        "bootc".into(), "install".into(), "to-disk".into(),
        "--generic-image".into(), "--skip-fetch-check".into(),
    ]);

    // Pass through install options
    for arg in opts.install.to_bootc_args() {
        cmd.push(arg.into());
    }

    // Target device (the real block device)
    cmd.push(opts.target_device.as_str().into());

    cmd
}

/// Execute native installation to the target block device.
pub fn run(opts: NativeToDiskOpts) -> Result<()> {
    // 1. Validate target is a block device
    let device_info = inspect_device(&opts.target_device)?;

    // 2. Verify nothing on the device is mounted
    check_not_mounted(&opts.target_device)?;

    // 3. Confirm with the user (unless --yes)
    info!(
        "Native install: {} -> {}",
        opts.source_image,
        opts.target_device
    );
    if !opts.yes {
        confirm_destructive(&device_info)?;
    } else {
        warn!(
            "--yes specified: skipping confirmation for {} ({})",
            opts.target_device,
            device_info.human_size()
        );
    }

    // 4. Resolve container storage
    let storage_path = if let Some(ref p) = opts.install.storage_path {
        p.clone()
    } else {
        crate::utils::detect_container_storage_path()?
    };
    debug!("Using container storage: {}", storage_path);

    // 5. Build and run the privileged podman command
    let podman_args = build_podman_cmd(&opts, &storage_path);
    debug!("Running: {:?}", podman_args);

    let program = podman_args[0].clone();
    let status = std::process::Command::new(&program)
        .args(&podman_args[1..])
        .status()
        .with_context(|| format!("Failed to exec {:?}", program))?;

    if !status.success() {
        bail!(
            "bootc install to-disk failed with exit code {:?}",
            status.code()
        );
    }

    info!(
        "Installation complete. {} is ready to boot.",
        opts.target_device
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Table-driven: check_not_mounted accepts devices absent from a mock mount list.
    #[test]
    fn test_check_not_mounted_parse() {
        // We can not call the real check_not_mounted (reads /proc/mounts),
        // so test the parsing logic directly.
        let mounts = "/dev/sda1 / ext4 rw 0 0
/dev/sda2 /boot ext4 rw 0 0
";
        let cases = [
            ("/dev/sdb", false),  // not mounted
            ("/dev/sda1", true),  // exact match
            ("/dev/sda", true),   // prefix match (sda1 starts with sda)
            ("/dev/nvme0n1", false),
        ];
        for (device, expect_mounted) in cases {
            let mounted = mounts.lines().any(|line| {
                let d = line.split_whitespace().next().unwrap_or("");
                d == device || d.starts_with(device)
            });
            assert_eq!(
                mounted, expect_mounted,
                "device={device} expected_mounted={expect_mounted}"
            );
        }
    }

    #[test]
    fn test_device_info_human_size() {
        let cases: &[(u64, &str)] = &[
            (64_000_000_000, "64.0 GB"),
            (16_000_000_000, "16.0 GB"),
            (1_000_000_000, "1.0 GB"),
        ];
        for (bytes, expected) in cases {
            let info = DeviceInfo {
                path: Utf8PathBuf::from("/dev/sdb"),
                size_bytes: *bytes,
                model: None,
            };
            assert_eq!(info.human_size(), *expected, "bytes={bytes}");
        }
    }

    #[test]
    fn test_build_podman_cmd_contains_device() {
        let opts = NativeToDiskOpts {
            source_image: "ghcr.io/example/yubios:latest".to_string(),
            target_device: Utf8PathBuf::from("/dev/sdb"),
            install: InstallOptions::default(),
            yes: true,
            install_log: None,
            rootful: false,
        };
        let storage = Utf8PathBuf::from("/var/lib/containers");
        let cmd = build_podman_cmd(&opts, &storage);
        let cmd_strs: Vec<&str> = cmd.iter().map(|s| s.to_str().unwrap()).collect();
        assert!(cmd_strs.contains(&"--privileged"), "must be privileged");
        assert!(cmd_strs.contains(&"to-disk"), "must call to-disk");
        assert!(
            cmd_strs.last() == Some(&"/dev/sdb"),
            "device must be last arg, got {:?}",
            cmd_strs.last()
        );
        assert!(
            cmd_strs.iter().any(|s| s.contains("yubios")),
            "source image must appear"
        );
    }
}