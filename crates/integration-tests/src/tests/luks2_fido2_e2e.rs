//! End-to-end test: LUKS2 FIDO2 unlock + systemd-homed + pam-u2f in a bcvk VM.
//!
//! Tracks yubi-OS/yubiOS#33 (closes #20), ADR-003 / ADR-009. Exercises the
//! FIDO2 trust boundary without physical hardware by combining the two CI-only
//! virtual devices added on `feat/swtpm-ci`:
//!   * `--swtpm`  -> /dev/tpm0 (measured-os paths)        — see docs/swtpm.md
//!   * `--swu2f`  -> loads `uhid` for an in-guest CTAP2 authenticator — docs/swu2f.md
//!
//! yubiOS production trust anchor stays the YubiKey FIDO2 device (ADR-003);
//! swtpm/swu2f are TEST-ONLY.
//!
//! ⚠️  INTEGRATION TEST POLICY (same as the rest of this crate):
//! NEVER "warn and continue" on failure. Fail hard with assert!/unwrap()/panic!,
//! or mark not-yet-possible work with todo!("reason").
//!
//! ## CTAP1 vs CTAP2 (why the full e2e is gated)
//! `systemd-cryptenroll --fido2-device=auto` and `homectl --fido2-device=auto`
//! both require the CTAP2 `hmac-secret` extension to derive the LUKS2 volume key.
//! QEMU's `u2f-emulated` (libu2f-emu) is CTAP1/U2F-only — fine for pam-u2f, but
//! it CANNOT drive systemd-cryptenroll. The portable CTAP2-in-a-VM answer is an
//! in-guest software authenticator over /dev/uhid (docs/swu2f.md "Layer 2"),
//! which is a guest-image follow-up that has not shipped yet. Until that fixture
//! exists, the full LUKS2/homed enrollment cannot pass, so it is a todo!() here.

use camino::Utf8Path;
use integration_tests::integration_test;
use itest::TestResult;

use std::fs;
use tempfile::TempDir;

use xshell::cmd;

use crate::{get_bck_command, get_test_image, shell, INTEGRATION_TEST_LABEL};

/// Success marker the in-guest prerequisite unit prints before poweroff.
const PREREQ_MARKER: &str = "OK-yubios-luks-fido2-prereqs";

/// Write a oneshot unit that asserts every prerequisite for the LUKS2 FIDO2 +
/// homed + pam-u2f e2e is present in the guest, prints `PREREQ_MARKER`, then
/// powers off. Any missing prerequisite makes the unit (and the test) fail hard.
fn write_prereq_unit(system_dir: &Utf8Path) -> std::io::Result<()> {
    let unit = format!(
        r#"[Unit]
Description=yubiOS LUKS2/FIDO2/homed e2e prerequisites then poweroff

[Service]
Type=oneshot
StandardOutput=journal+console
StandardError=journal+console
# swtpm: the emulated TPM must enumerate as /dev/tpm0
ExecStart=test -c /dev/tpm0
# swu2f layer 1: uhid must be loadable so an in-guest CTAP2 authenticator can open /dev/uhid
ExecStart=/bin/sh -c 'test -e /dev/uhid || modprobe uhid'
ExecStart=test -e /dev/uhid
# tooling required by the full e2e
ExecStart=/bin/sh -c 'command -v systemd-cryptenroll'
ExecStart=/bin/sh -c 'command -v homectl'
ExecStart=/bin/sh -c 'command -v cryptsetup'
ExecStart=/bin/sh -c 'find /usr/lib* /lib* -name pam_u2f.so 2>/dev/null | grep -q .'
ExecStart=echo {PREREQ_MARKER}
ExecStart=systemctl poweroff
"#
    );
    fs::write(system_dir.join("yubios-luks-fido2-prereq.service"), unit)
}

/// Runnable now: boot an ephemeral VM with both virtual security devices and
/// verify the guest has everything the full e2e needs (/dev/tpm0, /dev/uhid,
/// systemd-cryptenroll, homectl, cryptsetup, pam_u2f.so).
///
/// Requires a fixture image carrying cryptsetup + libfido2 + pam-u2f +
/// systemd-cryptsetup + systemd-homed (see fixtures/Dockerfile.luks2-fido2-e2e);
/// point BCVK_PRIMARY_IMAGE at it. Runner must provide swtpm/swtpm-tools and a
/// QEMU built with uhid support.
fn test_luks2_fido2_e2e_prereqs() -> TestResult {
    let units = TempDir::new().expect("units tempdir");
    let units_path = Utf8Path::from_path(units.path()).expect("units path utf8");
    let system_dir = units_path.join("system");
    fs::create_dir(&system_dir).expect("system dir");
    write_prereq_unit(&system_dir).expect("write prereq unit");

    let sh = shell()?;
    let bck = get_bck_command()?;
    let label = INTEGRATION_TEST_LABEL;
    let image = get_test_image();

    let stdout = cmd!(
        sh,
        "{bck} ephemeral run --rm --label {label} --console -K --swtpm --swu2f --systemd-units {units_path} --karg systemd.unit=yubios-luks-fido2-prereq.service --karg systemd.journald.forward_to_console=1 {image}"
    )
    .read()?;

    assert!(
        stdout.contains(PREREQ_MARKER),
        "guest did not confirm LUKS2/FIDO2/homed prerequisites; output:\n{stdout}"
    );
    Ok(())
}
integration_test!(test_luks2_fido2_e2e_prereqs);

/// Full e2e per yubiOS#33 — currently BLOCKED on the in-guest CTAP2 authenticator.
///
/// Intended in-guest sequence (driven by a systemd unit, image must ship a
/// uhid CTAP2 software authenticator, see docs/swu2f.md "Layer 2"):
///   1. start the in-guest CTAP2 authenticator -> /dev/uhid FIDO2 token
///   2. truncate -s 64M /tmp/test.luks; cryptsetup luksFormat --type luks2 (temp passphrase)
///   3. systemd-cryptenroll --fido2-device=auto --fido2-with-client-pin=no /tmp/test.luks
///   4. systemd-cryptsetup attach t /tmp/test.luks (FIDO2) ; assert /dev/mapper/t
///   5. homectl create e2e --storage=luks --fido2-device=auto ; homectl authenticate e2e
///   6. pamu2fcfg against the swu2f device ; assert pam-u2f auth succeeds
///   7. echo OK marker ; systemctl poweroff
///
/// Why this can't pass yet: `--swu2f` only loads `uhid`; libu2f-emu (QEMU
/// u2f-emulated) is CTAP1-only and cannot satisfy `systemd-cryptenroll --fido2`
/// (needs CTAP2 hmac-secret). The Layer 2 in-guest authenticator + its
/// fixture image are an unshipped follow-up. Also: at feat/swtpm-ci HEAD,
/// run_ephemeral.rs calls `swu2f::push_uhid_kargs` while swu2f.rs provides the
/// host `u2f-emulated` route — a build mismatch the leader must reconcile.
fn test_luks2_fido2_unlock_homed_e2e() -> TestResult {
    todo!(
        "blocked: in-guest CTAP2 /dev/uhid authenticator not shipped (docs/swu2f.md Layer 2). \
         --swu2f only loads uhid; libu2f-emu is CTAP1-only and cannot drive \
         systemd-cryptenroll --fido2 (hmac-secret). Unblock = guest-image PR adding a \
         uhid CTAP2 software authenticator + fixtures/Dockerfile.swu2f, then enroll/unlock here."
    );
}
integration_test!(test_luks2_fido2_unlock_homed_e2e);
