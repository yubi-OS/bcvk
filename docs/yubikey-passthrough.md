--- run_ephemeral.rs patch ---
Add `--yubikey` flag to CommonVmOpts and wire through to QemuConfig.

In CommonVmOpts struct, add:

    /// Pass YubiKey USB devices into the VM (requires host YubiKey attached).
    /// Detected via sysfs (Yubico vendor 0x1050). Adds usb-ehci controller
    /// and usb-host device entries to QEMU args.
    #[clap(long, help = "Pass YubiKey USB device(s) into the VM")]
    pub yubikey: bool,

In run_impl() where QemuConfig is built, after existing device setup:

    if opts.common.yubikey {
        let keys = usb_passthrough::require_yubikeys()?;
        tracing::info!("Passing {} YubiKey(s) into VM", keys.len());
        for arg in usb_passthrough::qemu_usb_args(&keys) {
            qemu_config.extra_args.push(arg);
        }
    }

Import at top of file:
    use crate::usb_passthrough;
