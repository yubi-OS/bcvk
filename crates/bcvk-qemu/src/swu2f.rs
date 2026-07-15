//! Software FIDO2/U2F authenticator (swu2f) wiring for ephemeral CI VMs.
//!
//! Unlike [`crate::swtpm`], a software FIDO2 token cannot be provided by QEMU:
//! QEMU only ships `u2f-emulated`, which speaks CTAP1/U2F and lacks the CTAP2
//! `hmac-secret` extension that `systemd-cryptenroll --fido2-device` and LUKS2
//! FIDO2 unlock require. The authenticator therefore runs *inside* the guest as
//! a userspace daemon (e.g. `fidorium` or `passless`) that registers a virtual
//! FIDO HID device through `/dev/uhid`. libfido2 (`fido2-token -L`),
//! `systemd-cryptenroll`, and `pam-u2f` then see it as a real token.
//!
//! bcvk's only responsibility on the host side is to make sure the guest kernel
//! has the `uhid` module available early via a `modules-load=` karg. The
//! authenticator binary itself must be present in the test image (image side,
//! analogous to shipping `swtpm`/`swtpm-tools` for [`crate::swtpm`]).
//!
//! Test-only: yubiOS production trust stays on the physical YubiKey FIDO2 device
//! (ADR-003). swu2f exists purely to cover enrollment / PAM code paths in CI
//! without hardware. See `docs/swu2f.md` and yubiOS issue #25.

/// Feature name used in user-facing flags / docs.
pub const SWU2F_FEATURE: &str = "fido2-swu2f";

/// Kernel module that backs userspace HID devices (`/dev/uhid`).
pub const UHID_MODULE: &str = "uhid";

/// Device node a uhid-based authenticator opens to register itself.
pub const UHID_DEVICE_PATH: &str = "/dev/uhid";

/// The `modules-load=` kernel argument that ensures `uhid` is loaded.
pub fn uhid_karg() -> String {
    format!("modules-load={UHID_MODULE}")
}

/// Append the `uhid` `modules-load=` karg to a kernel cmdline if not already present.
///
/// Idempotent: a second call with the same cmdline is a no-op.
pub fn push_uhid_kargs(cmdline: &mut Vec<String>) {
    let karg = uhid_karg();
    if !cmdline.iter().any(|a| a == &karg) {
        cmdline.push(karg);
    }
}

/// Configuration for the in-guest software FIDO2/U2F authenticator.
///
/// Currently this only records which authenticator binary the test image is
/// expected to provide; the daemon lifecycle is owned by the guest (a systemd
/// unit in the image), not by bcvk on the host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Swu2fConfig {
    /// Name of the userspace authenticator binary shipped in the test image.
    pub authenticator: String,
}

impl Default for Swu2fConfig {
    fn default() -> Self {
        Self { authenticator: "fidorium".to_string() }
    }
}

impl Swu2fConfig {
    /// Build a config naming a specific authenticator binary.
    pub fn with_authenticator(authenticator: impl Into<String>) -> Self {
        Self { authenticator: authenticator.into() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uhid_karg_is_modules_load() {
        assert_eq!(uhid_karg(), "modules-load=uhid");
    }

    #[test]
    fn push_uhid_kargs_appends_once() {
        let mut c = vec!["selinux=0".to_string()];
        push_uhid_kargs(&mut c);
        push_uhid_kargs(&mut c);
        assert_eq!(c.iter().filter(|a| a.as_str() == "modules-load=uhid").count(), 1);
        assert!(c.contains(&"selinux=0".to_string()));
    }

    #[test]
    fn push_uhid_kargs_preserves_order_tail() {
        let mut c = vec!["a".to_string(), "b".to_string()];
        push_uhid_kargs(&mut c);
        assert_eq!(c.last().unwrap(), "modules-load=uhid");
    }

    #[test]
    fn default_authenticator() {
        assert_eq!(Swu2fConfig::default().authenticator, "fidorium");
    }

    #[test]
    fn with_authenticator_overrides() {
        assert_eq!(Swu2fConfig::with_authenticator("passless").authenticator, "passless");
    }

    #[test]
    fn device_path_is_uhid() {
        assert_eq!(UHID_DEVICE_PATH, "/dev/uhid");
        assert_eq!(SWU2F_FEATURE, "fido2-swu2f");
    }
}
