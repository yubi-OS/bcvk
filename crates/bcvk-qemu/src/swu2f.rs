//! Software U2F / FIDO2 device for bcvk ephemeral VMs (test-only).
//!
//! Mirrors the `swtpm` module: a YubiKey-free way to exercise FIDO2/U2F code
//! paths in CI VMs without physical hardware. There are two distinct layers,
//! because the standards split the same way:
//!
//! 1. **QEMU emulated U2F** (`-device u2f-emulated`, backed by libu2f-emu): a
//!    fully software USB HID token implementing the **U2F (CTAP1)** protocol.
//!    Host/QEMU-provided and deterministic, exactly like swtpm — the guest sees
//!    a real USB HID security key with no in-guest service. This covers
//!    `pam-u2f` login tests. It does NOT implement CTAP2/FIDO2 `hmac-secret`,
//!    so it cannot drive `systemd-cryptenroll --fido2` (see docs/swu2f.md).
//!
//! 2. **In-guest uhid CTAP2 authenticator** (FIDO2 `hmac-secret`): a software
//!    authenticator that creates a virtual HID FIDO2 device via `/dev/uhid`
//!    inside the guest, for `systemd-cryptenroll --fido2` LUKS2 unlock tests.
//!    That layer is guest-image work (a fixture + a software authenticator),
//!    documented in docs/swu2f.md; it is out of scope for this QEMU-arg builder.
//!
//! yubiOS production trust anchor remains the **YubiKey FIDO2** device
//! (yubiOS ADR-003); swu2f is strictly for test coverage. Tracks
//! yubi-OS/yubiOS#25.
//!
//! This module is pure: it only builds argument vectors. Any process/IO lives
//! in `qemu.rs`, per the bcvk REVIEW.md rule of splitting builders from I/O.

/// Configuration for the QEMU emulated U2F (CTAP1) USB token.
#[derive(Debug, Clone, Default)]
pub struct Swu2fConfig {
    /// Optional libu2f-emu setup directory. When present it must contain
    /// `certificate.pem`, `private-key.pem`, `counter`, and `entropy` (48
    /// bytes), giving the token a stable identity across runs. When `None`,
    /// the device runs in libu2f-emu **ephemeral** mode and generates a
    /// single-use identity for the lifetime of the VM.
    pub setup_dir: Option<String>,
}

/// Build the QEMU arguments wiring an emulated USB U2F token into the VM.
///
/// Matches the QEMU documented invocation:
/// - ephemeral:    `-usb -device u2f-emulated`
/// - setup dir:    `-usb -device u2f-emulated,dir=<dir>`
///
/// `-usb` attaches the machine's default USB controller so the HID token can
/// enumerate. The guest `u2f`/`hid-generic` driver then exposes it as a
/// `/dev/hidrawN` security key — usable by libfido2 and pam-u2f.
pub fn qemu_u2f_args(config: &Swu2fConfig) -> Vec<String> {
    let mut device = "u2f-emulated".to_string();
    if let Some(dir) = &config.setup_dir {
        device.push_str(&format!(",dir={dir}"));
    }
    vec!["-usb".to_string(), "-device".to_string(), device]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_u2f_args_ephemeral() {
        let args = qemu_u2f_args(&Swu2fConfig::default());
        assert_eq!(args, vec!["-usb", "-device", "u2f-emulated"]);
    }

    #[test]
    fn test_u2f_args_setup_dir() {
        let cfg = Swu2fConfig {
            setup_dir: Some("/run/u2f-setup".to_string()),
        };
        let args = qemu_u2f_args(&cfg);
        assert_eq!(args[0], "-usb");
        assert_eq!(args[1], "-device");
        assert_eq!(args[2], "u2f-emulated,dir=/run/u2f-setup");
    }

    #[test]
    fn test_u2f_args_arg_count() {
        assert_eq!(qemu_u2f_args(&Swu2fConfig::default()).len(), 3);
    }
}
