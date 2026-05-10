//! YubiKey USB passthrough detection and QEMU device argument generation.
//!
//! Detects YubiKey 5 series devices on the host via the Linux udev/sysfs
//! interface (Yubico USB vendor ID 0x1050) and generates the QEMU
//! `-device usb-host` arguments needed to pass them into an ephemeral VM.
//!
//! # Security note
//!
//! USB passthrough gives the VM direct access to the YubiKey. Only use
//! with trusted images (e.g. yubiOS) where this is intentional.

use std::path::PathBuf;

use color_eyre::eyre::{bail, Context};
use color_eyre::Result;
use tracing::debug;

/// Yubico USB vendor ID.
pub const YUBICO_VENDOR_ID: u16 = 0x1050;

/// A USB host device to pass through to QEMU.
#[derive(Debug, Clone)]
pub struct UsbHostDevice {
    /// USB vendor ID (hex, e.g. 0x1050 for Yubico).
    pub vendor_id: u16,
    /// USB product ID. None = match any product from this vendor.
    pub product_id: Option<u16>,
    /// Human-readable label for logging.
    pub label: String,
}

impl UsbHostDevice {
    /// Match any YubiKey (any product ID under vendor 0x1050).
    pub fn any_yubikey() -> Self {
        Self {
            vendor_id: YUBICO_VENDOR_ID,
            product_id: None,
            label: "YubiKey (any model)".to_string(),
        }
    }

    /// Build QEMU `-device usb-host` argument string.
    ///
    /// Example: `usb-host,vendorid=0x1050,productid=0x0407`
    pub fn qemu_device_arg(&self) -> String {
        let mut arg = format!("usb-host,vendorid=0x{:04x}", self.vendor_id);
        if let Some(pid) = self.product_id {
            arg.push_str(&format!(",productid=0x{:04x}", pid));
        }
        arg
    }
}

/// Probe the host for attached YubiKey devices via sysfs.
///
/// Walks /sys/bus/usb/devices/ and matches on idVendor == 0x1050.
/// Returns one `UsbHostDevice` per unique (vendor, product) pair found.
pub fn detect_yubikeys() -> Result<Vec<UsbHostDevice>> {
    let mut found = Vec::new();
    let base = PathBuf::from("/sys/bus/usb/devices");

    if !base.exists() {
        debug!("sysfs not available — skipping YubiKey detection");
        return Ok(found);
    }

    for entry in std::fs::read_dir(&base).context("reading /sys/bus/usb/devices")? {
        let entry = entry?;
        let vendor_path = entry.path().join("idVendor");
        let product_path = entry.path().join("idProduct");
        if !vendor_path.exists() {
            continue;
        }
        let vendor_str = std::fs::read_to_string(&vendor_path)
            .unwrap_or_default()
            .trim()
            .to_lowercase();
        if vendor_str != format!("{:04x}", YUBICO_VENDOR_ID) {
            continue;
        }
        let product_id = std::fs::read_to_string(&product_path)
            .ok()
            .and_then(|s| u16::from_str_radix(s.trim(), 16).ok());
        debug!(
            "Found YubiKey: vendor={} product={:?} at {}",
            vendor_str,
            product_id,
            entry.path().display()
        );
        found.push(UsbHostDevice {
            vendor_id: YUBICO_VENDOR_ID,
            product_id,
            label: format!(
                "YubiKey (0x1050:{:04x})",
                product_id.unwrap_or(0)
            ),
        });
    }
    Ok(found)
}

/// Detect YubiKeys and fail with a clear message if none found.
pub fn require_yubikeys() -> Result<Vec<UsbHostDevice>> {
    let keys = detect_yubikeys()?;
    if keys.is_empty() {
        bail!(
            "--yubikey requested but no YubiKey detected.
             Insert a YubiKey (Yubico vendor 0x1050) and retry."
        );
    }
    Ok(keys)
}

/// Build the QEMU USB controller + host-device arguments for a list of devices.
///
/// Adds a USB 2.0 EHCI controller (one per call) then one usb-host device per
/// YubiKey. Returns a flat list of QEMU argument strings.
pub fn qemu_usb_args(devices: &[UsbHostDevice]) -> Vec<String> {
    if devices.is_empty() {
        return Vec::new();
    }
    let mut args = vec![
        // USB 2.0 EHCI controller — required for usb-host on x86 and arm64
        "-device".to_string(),
        "usb-ehci,id=yubikey-ehci".to_string(),
    ];
    for dev in devices {
        args.push("-device".to_string());
        let mut dev_arg = dev.qemu_device_arg();
        dev_arg.push_str(",bus=yubikey-ehci.0");
        args.push(dev_arg);
    }
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_qemu_device_arg_vendor_only() {
        let dev = UsbHostDevice::any_yubikey();
        assert_eq!(dev.qemu_device_arg(), "usb-host,vendorid=0x1050");
    }

    #[test]
    fn test_qemu_device_arg_with_product() {
        let dev = UsbHostDevice {
            vendor_id: 0x1050,
            product_id: Some(0x0407),
            label: "YubiKey 5 NFC".to_string(),
        };
        assert_eq!(
            dev.qemu_device_arg(),
            "usb-host,vendorid=0x1050,productid=0x0407"
        );
    }

    #[test]
    fn test_qemu_usb_args_empty() {
        assert!(qemu_usb_args(&[]).is_empty());
    }

    #[test]
    fn test_qemu_usb_args_adds_controller() {
        let devs = vec![UsbHostDevice::any_yubikey()];
        let args = qemu_usb_args(&devs);
        assert!(args.contains(&"usb-ehci,id=yubikey-ehci".to_string()));
        assert!(args.iter().any(|a| a.contains("usb-host")));
        assert!(args.iter().any(|a| a.contains("yubikey-ehci.0")));
    }

    #[test]
    fn test_any_yubikey_vendor() {
        let dev = UsbHostDevice::any_yubikey();
        assert_eq!(dev.vendor_id, 0x1050);
        assert!(dev.product_id.is_none());
    }
}