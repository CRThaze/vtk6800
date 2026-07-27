//! Host-side udev rule management (Linux).
//!
//! The keyboard's `hidraw` nodes are root-only by default. A udev rule tagged
//! `uaccess` grants the logged-in user read/write on them. This module renders,
//! installs, and verifies that rule. The filename sorts below
//! `73-seat-late.rules` so the `uaccess` tag is honored.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};

use vtk6800_core::{device, PID, VID};

/// Canonical install location and filename for our rule.
const RULES_DIR: &str = "/etc/udev/rules.d";
const RULE_FILENAME: &str = "70-vortex-pc68.rules";

/// Every directory udev reads rules from, so `check` finds a rule no matter
/// where (or under what name) it was installed.
const SEARCH_DIRS: &[&str] = &[
    "/etc/udev/rules.d",
    "/run/udev/rules.d",
    "/usr/lib/udev/rules.d",
    "/lib/udev/rules.d",
];

/// The canonical path we install to.
pub fn target_path() -> PathBuf {
    Path::new(RULES_DIR).join(RULE_FILENAME)
}

/// The rule text for this build's VID/PID.
pub fn rule_text() -> String {
    format!(
        "# Installed by vtk6800; grants the logged-in user rw on the Vortex PC66/PC68 hidraw nodes.\n\
         # Filename sorts below 73-seat-late.rules so TAG+=\"uaccess\" is honored.\n\
         KERNEL==\"hidraw*\", ATTRS{{idVendor}}==\"{VID:04x}\", ATTRS{{idProduct}}==\"{PID:04x}\", MODE=\"0660\", TAG+=\"uaccess\"\n",
    )
}

/// A rule file already on disk that references our VID/PID.
pub struct Installed {
    pub path: PathBuf,
    /// Whether its contents match what we would write now.
    pub up_to_date: bool,
}

/// Scan udev's rule directories for a rule referencing our VID/PID. Matching on
/// the hex ids (not the filename) catches rules installed by hand or under a
/// different name.
pub fn find_installed() -> Option<Installed> {
    let vid = format!("{VID:04x}");
    let pid = format!("{PID:04x}");
    let want = rule_text();
    for dir in SEARCH_DIRS {
        let entries = match fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("rules") {
                continue;
            }
            let Ok(text) = fs::read_to_string(&path) else {
                continue;
            };
            // Case-insensitive id match; a rule may spell hex in either case.
            let lower = text.to_ascii_lowercase();
            if lower.contains(&vid) && lower.contains(&pid) {
                return Some(Installed {
                    up_to_date: text == want,
                    path,
                });
            }
        }
    }
    None
}

/// Ground-truth access test: for each matching `hidraw` node, can we open it
/// read/write? This is what actually determines whether the tool will work,
/// independent of whether a rule file exists.
pub fn access_report() -> Result<Vec<(String, bool)>> {
    let devs = device::list().context("enumerating HID devices")?;
    Ok(devs
        .into_iter()
        .map(|d| {
            let ok = fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(&d.path)
                .is_ok();
            (d.path, ok)
        })
        .collect())
}

/// Outcome of an install attempt.
pub enum InstallOutcome {
    /// An up-to-date rule was already present; nothing to do.
    AlreadyPresent(PathBuf),
    /// We wrote the rule (we had permission, i.e. ran as root).
    Wrote(PathBuf),
    /// We lack permission to write; the caller should print manual steps.
    NeedsRoot(PathBuf),
}

/// Install the rule if it is not already present and up to date. Writes
/// directly when we have permission; otherwise reports [`InstallOutcome::NeedsRoot`]
/// rather than escalating privileges itself.
pub fn install() -> Result<InstallOutcome> {
    if let Some(existing) = find_installed() {
        if existing.up_to_date {
            return Ok(InstallOutcome::AlreadyPresent(existing.path));
        }
        // A stale/differing rule exists elsewhere; fall through and (re)write
        // our canonical file so the correct rule is definitely present.
    }

    let target = target_path();
    match fs::write(&target, rule_text()) {
        Ok(()) => {
            reload_udev();
            Ok(InstallOutcome::Wrote(target))
        }
        Err(e) if e.kind() == ErrorKind::PermissionDenied => Ok(InstallOutcome::NeedsRoot(target)),
        Err(e) => Err(e).with_context(|| format!("writing {}", target.display())),
    }
}

/// Best-effort `udevadm control --reload-rules && udevadm trigger`. Only called
/// after a successful write (so we are root); a missing `udevadm` is non-fatal.
fn reload_udev() {
    let _ = Command::new("udevadm")
        .args(["control", "--reload-rules"])
        .status();
    let _ = Command::new("udevadm").arg("trigger").status();
}

#[cfg(test)]
mod tests {
    use super::rule_text;

    /// The `.deb`/`.rpm` packages ship a checked-in copy of the rule
    /// (`packaging/udev/70-vortex-pc68.rules`) so packaging never has to run the
    /// cross-built binary to emit it. This test guards that file against drift:
    /// regenerate it with `vtk6800 udev install --print` if this fails.
    #[test]
    fn packaged_rule_matches_generated() {
        let packaged = include_str!("../../../packaging/udev/70-vortex-pc68.rules");
        assert_eq!(
            packaged,
            rule_text(),
            "packaging/udev/70-vortex-pc68.rules is out of sync with rule_text(); \
             regenerate it: `cargo run -p vtk6800-cli -- udev install --print > \
             packaging/udev/70-vortex-pc68.rules`"
        );
    }
}
