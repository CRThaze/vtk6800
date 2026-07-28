//! `vtk6800`: command-line configuration tool for the Vortex PC66/PC68.

mod layout;

use std::io::IsTerminal;

use vtk6800_config::config;
#[cfg(target_os = "linux")]
use vtk6800_config::udev;

use anyhow::{anyhow, bail, Context, Result};
use clap::{Parser, Subcommand};

use vtk6800_core::{
    device, keycode,
    model::{Entry, LayerId, Macro, MacroEvent, MouseAction, MouseButton},
    protocol, Device, DryRunTransport, FeatureTransport, FnMode, Keymap, Variant,
};

/// Configure a Vortex PC66/PC68 (VTK-6800) keyboard on Linux.
#[derive(Parser)]
#[command(name = "vtk6800", version, about, long_about = None)]
struct Cli {
    /// Board variant (ansi68, iso68, ansi66, iso66). Defaults to the saved
    /// default variant (see the `default` command), or ansi68.
    #[arg(long, global = true)]
    variant: Option<String>,

    /// Path to the config file (defaults to the platform config dir).
    #[arg(long, global = true)]
    config: Option<std::path::PathBuf>,

    /// Increase verbosity (-v, -vv).
    #[arg(short, long, global = true, action = clap::ArgAction::Count)]
    verbose: u8,

    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// List connected keyboard HID interfaces.
    Devices,
    /// Manage the Linux udev rule that grants access to the keyboard.
    #[cfg(target_os = "linux")]
    Udev {
        #[command(subcommand)]
        action: UdevAction,
    },
    /// Verify the keyboard is connected and responding. Read-only.
    ConnCheck,
    /// Show or set the default board variant (used when --variant is omitted).
    Default {
        /// Variant to set as default; omit to show the current default.
        variant: Option<String>,
    },
    /// Manage the local keymap (show, set keys, import, diff, apply).
    Keymap {
        #[command(subcommand)]
        action: KeymapAction,
    },
}

#[cfg(target_os = "linux")]
#[derive(Subcommand)]
enum UdevAction {
    /// Report whether the udev rule is installed and the device is accessible.
    Check,
    /// Install the udev rule (writes to /etc/udev/rules.d; needs root).
    Install {
        /// Print the rule to stdout instead of installing it (pipe to a file).
        #[arg(long)]
        print: bool,
    },
}

#[derive(Subcommand)]
enum KeymapAction {
    /// Print the keymap file path.
    Path,
    /// Show the current keymap.
    Show {
        /// Output format.
        #[arg(long, value_enum, default_value_t = ShowFormat::Layout)]
        format: ShowFormat,
        /// In the layout view, use 3-letter mnemonics instead of Unicode glyphs.
        #[arg(long)]
        mnemonics: bool,
        /// In the layout view, also print a slot-number table (the `slot:N`
        /// values) after each layer's bindings.
        #[arg(long)]
        slots: bool,
    },
    /// Stage a key assignment on a layer. Only edits the local keymap (the source
    /// of truth); run `apply` to write to the keyboard; `diff` reviews changes.
    ///
    /// The KEY names a *physical* key by its factory layout (not its current
    /// binding): `capslock` always means that key regardless of what it's remapped
    /// to. To re-change an already-remapped key, use its original name or `slot:N`.
    /// E.g. `keymap set base capslock lctrl`.
    Set {
        /// List every valid ACTION (key names with their hid: codes, media names,
        /// and the raw hid: range) and exit, ignoring the positional arguments.
        #[arg(long)]
        valid_actions: bool,
        /// Layer: base | fn.
        #[arg(required_unless_present = "valid_actions")]
        layer: Option<String>,
        /// Physical key by its factory name (from the default layout) or `slot:N`.
        #[arg(required_unless_present = "valid_actions")]
        key: Option<String>,
        /// Action: a key name, `media:<name>`, `mouse:<action>`, `macro:N`,
        /// `hid:0xNN`, or `none` (see --valid-actions).
        #[arg(required_unless_present = "valid_actions")]
        action: Option<String>,
    },
    /// Reset the keymap to the variant's factory default.
    Reset,
    /// Set how the Fn key behaves: `momentary` (hold, factory default) or
    /// `latch` (tap to latch the Fn layer on/off).
    ///
    /// Stages the setting in the keymap file; `apply` writes it alongside the
    /// keys. Pass --apply-immediately to write just this setting to the keyboard now.
    SetFnMode {
        /// Mode: momentary | latch.
        mode: String,
        /// After staging, write the setting to the keyboard immediately.
        #[arg(long)]
        apply_immediately: bool,
        /// With --apply-immediately, skip the confirmation prompt (non-interactive).
        #[arg(long)]
        yes: bool,
        /// Print the reports that --apply-immediately would send, without writing them.
        #[arg(long)]
        dry_run: bool,
    },
    /// Save the current keymap as a named preset (per variant).
    ///
    /// Presets are copies of the current keymap (not the applied snapshot),
    /// stored under `<config-dir>/presets/`. Overwriting one asks first.
    SavePreset {
        /// Preset name (letters, digits, '-', '_', '.').
        name: String,
        /// Overwrite an existing preset without confirming.
        #[arg(long)]
        yes: bool,
    },
    /// Load a named preset, overwriting the current keymap.
    ///
    /// Review with `diff` and write with `apply` afterwards.
    LoadPreset {
        /// Preset name to load.
        name: String,
    },
    /// List the saved presets for the variant.
    ListPresets,
    /// Delete a named preset (asks first).
    DeletePreset {
        /// Preset name to delete.
        name: String,
        /// Delete without confirming.
        #[arg(long)]
        yes: bool,
    },
    /// Import a vendor GUI profile export (.xml) into the local keymap.
    ///
    /// The keyboard's Windows tool exports profiles as XML; this converts one
    /// into the local keymap (replacing it) so you can review with `diff` and
    /// write with `apply`. The variant is auto-detected from the profile's keys
    /// when unambiguous; otherwise pass `--variant`.
    Import {
        /// Path to the exported `.xml` profile.
        file: std::path::PathBuf,
        /// Import into a named preset instead of the current keymap.
        #[arg(long)]
        to_preset: Option<String>,
        /// With --to-preset, overwrite an existing preset without confirming.
        #[arg(long)]
        yes: bool,
    },
    /// Show pending changes vs the last applied snapshot.
    Diff,
    /// Flash the config to the keyboard. Checks connectivity, then asks before
    /// writing (unless --commit/--yes).
    Apply {
        /// Print the encoded reports and exit without touching the keyboard.
        #[arg(long)]
        dry_run: bool,
        /// Apply without the confirmation prompt (non-interactive).
        #[arg(long)]
        commit: bool,
        /// Apply without the confirmation prompt (alias of --commit).
        #[arg(long)]
        yes: bool,
        /// Flash the keymap only; do not upload macro bodies (leaves any
        /// already-stored macros intact). For debugging the macro path.
        #[arg(long)]
        skip_macros: bool,
    },
}

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
enum ShowFormat {
    /// Unicode drawing of the keyboard (default).
    Layout,
    /// Per-layer list of slots and bindings.
    Text,
    /// The raw keymap file (YAML).
    Yaml,
}

fn init_tracing(verbose: u8) {
    let level = match verbose {
        0 => "warn",
        1 => "info",
        2 => "debug",
        _ => "trace",
    };
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| format!("vtk6800={level},vtk6800_core={level}").into()),
        )
        .with_target(false)
        .without_time()
        .try_init();
}

fn cmd_devices() -> Result<()> {
    let devs = device::list().context("enumerating HID devices")?;
    if devs.is_empty() {
        println!(
            "No keyboard found (VID {:04x} PID {:04x}). Check the cable and hidraw permissions.",
            vtk6800_core::VID,
            vtk6800_core::PID
        );
        return Ok(());
    }
    let chosen = device::preferred_index(&devs);
    println!("Found {} interface(s):", devs.len());
    for (i, d) in devs.iter().enumerate() {
        let marker = if Some(i) == chosen {
            "  <- config interface"
        } else {
            ""
        };
        println!(
            "  iface {:>2}  usage {:#06x}/{:#04x}  {}  [{}]{}",
            d.interface,
            d.usage_page,
            d.usage,
            d.product.as_deref().unwrap_or("?"),
            d.path,
            marker
        );
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn cmd_udev(action: UdevAction) -> Result<()> {
    match action {
        UdevAction::Check => cmd_udev_check(),
        UdevAction::Install { print } => cmd_udev_install(print),
    }
}

#[cfg(target_os = "linux")]
fn cmd_udev_check() -> Result<()> {
    match udev::find_installed() {
        Some(rule) if rule.up_to_date => {
            println!("Rule installed: {} (up to date)", rule.path.display());
        }
        Some(rule) => {
            println!(
                "Rule found at {} but its contents differ from this build's.",
                rule.path.display()
            );
            println!("Re-run `vtk6800 udev install` to refresh it.");
        }
        None => {
            println!("No udev rule for this keyboard found.");
            println!("Run `vtk6800 udev install` to add one.");
        }
    }

    // Ground truth: whether we can actually open the nodes.
    let report = udev::access_report()?;
    if report.is_empty() {
        println!("\nNo matching keyboard is currently connected, so access can't be tested.");
    } else {
        println!("\nDevice access (read/write):");
        for (path, ok) in &report {
            println!("  {}  {}", if *ok { "ok  " } else { "DENIED" }, path);
        }
        if report.iter().any(|(_, ok)| !ok) {
            println!(
                "\nSome nodes are not writable. If you just installed the rule, replug the\n\
                 keyboard (or run `sudo udevadm control --reload-rules && sudo udevadm trigger`)."
            );
        }
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn cmd_udev_install(print: bool) -> Result<()> {
    if print {
        print!("{}", udev::rule_text());
        return Ok(());
    }
    match udev::install()? {
        udev::InstallOutcome::AlreadyPresent(path) => {
            println!("Already installed: {} (up to date).", path.display());
        }
        udev::InstallOutcome::Wrote(path) => {
            println!("Installed {} and reloaded udev.", path.display());
            println!("Replug the keyboard if it's currently connected.");
        }
        udev::InstallOutcome::NeedsRoot(path) => {
            println!(
                "Root is required to write {}. Run:\n\n  \
                 vtk6800 udev install --print | sudo tee {} >/dev/null\n  \
                 sudo udevadm control --reload-rules && sudo udevadm trigger\n\n\
                 Then replug the keyboard.",
                path.display(),
                path.display()
            );
        }
    }
    Ok(())
}

/// Probe the device to confirm the transport works end-to-end. Internally this
/// reads the lighting framebuffer (the one safe read the firmware exposes), but
/// that mechanism is not surfaced to the user. Reused as a fail-fast check
/// before any write.
fn conn_check_probe<T: FeatureTransport>(dev: &mut Device<T>) -> Result<()> {
    // The read-back is intermittently slow to answer (a GET_FEATURE can time out
    // even on a healthy keyboard), so retry a few times before concluding the
    // keyboard is absent; a flaky read must not abort a write.
    let mut last_err = None;
    for attempt in 0..4 {
        match dev.dump_lights() {
            Ok(_) => return Ok(()),
            Err(e) => last_err = Some(e),
        }
        if attempt < 3 {
            std::thread::sleep(std::time::Duration::from_millis(75));
        }
    }
    Err(last_err.expect("loop body ran at least once"))
        .context("connectivity check failed (keyboard did not respond after 4 attempts)")
}

fn cmd_conn_check() -> Result<()> {
    let mut dev = Device::open(None).context("opening keyboard")?;
    conn_check_probe(&mut dev)?;
    println!("Keyboard is connected and responding.");
    Ok(())
}

/// Print a stderr warning for any Fn-layer bindings that land on firmware-
/// reserved keys (Bluetooth / RGB / Win-lock combos the hardware owns). Such
/// bindings flash without error but the keyboard silently ignores them. No-op
/// when the keymap is clean, so a freshly seeded default never warns.
fn warn_reserved(km: &Keymap) {
    let conflicts = vtk6800_core::reserved::reserved_conflicts(km);
    if conflicts.is_empty() {
        return;
    }
    eprintln!(
        "warning: {} Fn-layer binding(s) fall on firmware-reserved keys and will be \
         ignored by the keyboard:",
        conflicts.len()
    );
    for c in &conflicts {
        eprintln!(
            "  Fn slot {}: '{}' -> reserved for {}",
            c.reserved.slot,
            c.entry.describe(),
            c.reserved.function
        );
    }
    eprintln!("  These Fn combos are hardwired; remap onto a non-reserved key instead.");
}

fn cmd_keymap(
    action: KeymapAction,
    variant_arg: Option<&str>,
    config_arg: Option<std::path::PathBuf>,
) -> Result<()> {
    // These run before resolving the variant/paths: import detects the variant
    // from the profile itself, and --valid-actions is a static reference.
    if let KeymapAction::Import {
        file,
        to_preset,
        yes,
    } = &action
    {
        return cmd_import(file, variant_arg, config_arg, to_preset.as_deref(), *yes);
    }
    if matches!(
        action,
        KeymapAction::Set {
            valid_actions: true,
            ..
        }
    ) {
        print_valid_actions();
        return Ok(());
    }
    let (variant, owned_paths) = resolve_variant_paths(variant_arg, config_arg)?;
    let paths = &owned_paths;
    match action {
        KeymapAction::Path => {
            println!("{}", paths.keymap.display());
            Ok(())
        }
        KeymapAction::Show {
            format,
            mnemonics,
            slots,
        } => {
            let km = config::load_or_seed(variant, paths)?;
            warn_reserved(&km);
            let labels = if mnemonics {
                layout::Labels::Mnemonic
            } else {
                layout::Labels::Glyph
            };
            match format {
                ShowFormat::Layout => {
                    let color =
                        std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none();
                    // Baseline for the "changed" highlight: last applied, else default.
                    let baseline = match config::load_applied(paths)? {
                        Some(km) => km,
                        None => variant.default_keymap()?,
                    };
                    print!("{}", layout::render(&km, &baseline, labels, color, slots));
                }
                ShowFormat::Text => print_keymap(&km),
                ShowFormat::Yaml => {
                    print!(
                        "{}",
                        vtk6800_core::document::to_yaml(&km).context("serializing keymap")?
                    )
                }
            }
            Ok(())
        }
        KeymapAction::Set {
            layer, key, action, ..
        } => {
            // clap guarantees these are present unless --valid-actions (handled
            // above), so the unwraps can't fire.
            cmd_set(
                variant,
                paths,
                &layer.expect("layer required unless --valid-actions"),
                &key.expect("key required unless --valid-actions"),
                &action.expect("action required unless --valid-actions"),
            )
        }
        KeymapAction::Reset => {
            let km = variant.default_keymap()?;
            config::save(&km, &paths.keymap)?;
            println!("Reset keymap to {} factory default.", variant.as_str());
            Ok(())
        }
        KeymapAction::SetFnMode {
            mode,
            apply_immediately,
            yes,
            dry_run,
        } => cmd_set_fn_mode(variant, paths, &mode, apply_immediately, yes, dry_run),
        KeymapAction::SavePreset { name, yes } => cmd_save_preset(variant, paths, &name, yes),
        KeymapAction::LoadPreset { name } => cmd_load_preset(variant, paths, &name),
        KeymapAction::ListPresets => cmd_list_presets(variant),
        KeymapAction::DeletePreset { name, yes } => cmd_delete_preset(variant, &name, yes),
        KeymapAction::Diff => cmd_diff(variant, paths),
        KeymapAction::Apply {
            dry_run,
            commit,
            yes,
            skip_macros,
        } => cmd_apply(variant, paths, dry_run, commit, yes, skip_macros),
        KeymapAction::Import { .. } => unreachable!("import is handled before variant resolution"),
    }
}

fn cmd_set(
    variant: Variant,
    paths: &config::Paths,
    layer: &str,
    key: &str,
    action: &str,
) -> Result<()> {
    let layer_id = LayerId::parse(layer).ok_or_else(|| anyhow!("unknown layer '{layer}'"))?;
    // The alt/raw table (`04 23`) is decompiled but its purpose is unconfirmed,
    // so it isn't offered for editing yet.
    if layer_id == LayerId::Alt {
        bail!("the 'alt' layer is not supported yet; use 'base' or 'fn'");
    }
    let slot = resolve_slot(variant, key)?;
    let entry = parse_action(action)?;

    let mut km = config::load_or_seed(variant, paths)?;
    let l = km
        .layer_mut(layer_id)
        .ok_or_else(|| anyhow!("keymap has no {layer} layer"))?;
    l.set(slot, entry);
    config::save(&km, &paths.keymap)?;
    println!(
        "Staged {layer}: slot {slot} = {} (run `apply` to write to the keyboard).",
        entry.describe()
    );
    // Immediate feedback if this slot is one the firmware owns on the Fn layer.
    if layer_id == LayerId::Fn {
        if let Some(r) = vtk6800_core::reserved::reserved_fn(slot) {
            eprintln!(
                "warning: Fn slot {slot} is firmware-reserved ({}); the keyboard will \
                 ignore this binding. Remap onto a non-reserved key instead.",
                r.function
            );
        }
    }
    Ok(())
}

fn cmd_diff(variant: Variant, paths: &config::Paths) -> Result<()> {
    let current = config::load_or_seed(variant, paths)?;
    let baseline = match config::load_applied(paths)? {
        Some(km) => km,
        None => {
            println!(
                "No prior applied keymap on record; nothing has been written to this\n\
                 keyboard by this tool yet. Comparing against the factory default.\n"
            );
            variant.default_keymap()?
        }
    };
    let mut changes = 0;
    for layer in &current.layers {
        let base_layer = baseline.layer(layer.id);
        for km in &layer.keys {
            let before = base_layer.and_then(|b| b.get(km.slot));
            if before != Some(km.entry) {
                let from = before.map(|e| e.describe()).unwrap_or_else(|| "—".into());
                println!(
                    "  {:?} slot {:>3}: {} -> {}",
                    layer.id,
                    km.slot,
                    from,
                    km.entry.describe()
                );
                changes += 1;
            }
        }
    }
    if changes == 0 {
        println!("No pending changes.");
    } else {
        println!("{changes} pending change(s). Run `vtk6800 keymap apply` to preview the writes.");
    }
    Ok(())
}

fn cmd_apply(
    variant: Variant,
    paths: &config::Paths,
    dry_run: bool,
    commit: bool,
    yes: bool,
    skip_macros: bool,
) -> Result<()> {
    let km = config::load_or_seed(variant, paths)?;
    warn_reserved(&km);

    // --dry-run: encode and print the reports; never touch the keyboard. Order
    // mirrors a real apply and the vendor: keymap, Fn-mode, then macros last.
    if dry_run {
        let mut dev = Device::new(DryRunTransport::default());
        dev.op_delay_ms = 0;
        dev.flash(&km)?;
        dev.set_fn_mode(km.fn_mode.is_momentary())?;
        if !skip_macros {
            dev.upload_macros(&km.macros)?;
        }
        let log = dev.into_transport().log;
        println!("DRY RUN: {} feature reports would be sent:\n", log.len());
        for (i, report) in log.iter().enumerate() {
            println!("  [{i:>3}] {}", hex(report));
        }
        return Ok(());
    }

    // Open and prove the transport works, then confirm before writing.
    let mut dev = Device::open(None).context("opening keyboard")?;
    conn_check_probe(&mut dev)?;
    println!("Connectivity OK.");

    if !(commit || yes || confirm("Write keymap to the keyboard now?")?) {
        println!("Not applied.");
        return Ok(());
    }

    // Vendor order: keymap, Fn-mode, then macros LAST (the macro upload
    // self-persists via its own `04 02`). `--skip-macros` leaves any
    // already-stored macro bodies untouched.
    dev.flash(&km).context("flashing keymap")?;
    config::save_applied(&km, paths)?;
    dev.set_fn_mode(km.fn_mode.is_momentary())
        .context("writing fn mode")?;
    if !skip_macros && !km.macros.is_empty() {
        dev.upload_macros(&km.macros).context("uploading macros")?;
    }
    let macro_note = if km.macros.is_empty() {
        String::new()
    } else {
        format!(", {} macro(s)", km.macros.len())
    };
    println!(
        "Applied (keymap + fn_mode = {}{macro_note}). Verify by typing; to restore \
         factory keys, run `keymap reset` then apply again.",
        km.fn_mode.as_str()
    );
    Ok(())
}

fn cmd_set_fn_mode(
    variant: Variant,
    paths: &config::Paths,
    mode: &str,
    apply_immediately: bool,
    yes: bool,
    dry_run: bool,
) -> Result<()> {
    let fn_mode = FnMode::parse(mode)
        .ok_or_else(|| anyhow!("unknown fn mode '{mode}' (expected 'momentary' or 'latch')"))?;

    let mut km = config::load_or_seed(variant, paths)?;
    km.fn_mode = fn_mode;
    config::save(&km, &paths.keymap)?;
    println!("Staged fn_mode = {}.", fn_mode.as_str());

    // --dry-run: show the `04 17` reports without writing.
    if dry_run {
        let mut dev = Device::new(DryRunTransport::default());
        dev.op_delay_ms = 0;
        dev.set_fn_mode(fn_mode.is_momentary())?;
        let log = dev.into_transport().log;
        println!("\nDRY RUN: {} feature reports would be sent:\n", log.len());
        for (i, report) in log.iter().enumerate() {
            println!("  [{i:>3}] {}", hex(report));
        }
        return Ok(());
    }

    if !apply_immediately {
        println!("Run `apply` (or re-run with --apply-immediately) to write it to the keyboard.");
        return Ok(());
    }

    let mut dev = Device::open(None).context("opening keyboard")?;
    conn_check_probe(&mut dev)?;
    println!("Connectivity OK.");
    if !yes
        && !confirm(&format!(
            "Write fn_mode = {} to the keyboard now?",
            fn_mode.as_str()
        ))?
    {
        println!("Staged but not written.");
        return Ok(());
    }
    dev.set_fn_mode(fn_mode.is_momentary())
        .context("writing fn mode")?;
    println!("Fn mode set to {}.", fn_mode.as_str());
    Ok(())
}

// --- helpers ---------------------------------------------------------------

fn resolve_slot(variant: Variant, key: &str) -> Result<u8> {
    if let Some(n) = key.strip_prefix("slot:") {
        return n.parse::<u8>().context("parsing slot number");
    }
    variant
        .slot_for_name(key)?
        .ok_or_else(|| anyhow!("no key named '{key}' in the {} layout", variant.as_str()))
}

/// Print every valid `keymap set` ACTION: named keys (with their `hid:` codes),
/// media names, `none`, and the raw `hid:` byte range including which bytes are
/// valid but unnamed.
fn print_valid_actions() {
    use std::collections::{BTreeMap, HashSet};

    println!("Named keys (use the name, or the `hid:` form):");
    let mut by_hid: BTreeMap<u8, Vec<&str>> = BTreeMap::new();
    for &(name, hid) in keycode::KEYS {
        by_hid.entry(hid).or_default().push(name);
    }
    for (&hid, names) in &by_hid {
        println!("  {:<22} hid:{hid:#04x}", names.join(" / "));
    }

    println!("\nMedia keys (`media:<name>`, or the bare name):");
    let mut by_code: BTreeMap<u8, Vec<&str>> = BTreeMap::new();
    for &(name, code) in keycode::CONSUMER {
        by_code.entry(code).or_default().push(name);
    }
    for names in by_code.values() {
        println!("  media:{}", names.join(" / "));
    }

    println!("\nmouse:<action>: a mouse button/wheel action. One of:");
    println!(
        "  {}",
        MouseAction::ALL
            .iter()
            .map(|a| a.as_str())
            .collect::<Vec<_>>()
            .join(" / ")
    );

    println!("\nmacro:N: run stored macro N (define macro bodies in the keymap YAML).");

    println!("\nnone: clear a key (leave it unassigned).");

    println!("\nRaw: `hid:0xNN` for a defined keyboard usage that has no name above:");
    let named: HashSet<u8> = by_hid.keys().copied().collect();
    let mut ranges: Vec<String> = Vec::new();
    let mut run_start: Option<u8> = None;
    for v in 0u16..=256 {
        let unnamed =
            v <= 0xff && keycode::is_writable_key_hid(v as u8) && !named.contains(&(v as u8));
        match (unnamed, run_start) {
            (true, None) => run_start = Some(v as u8),
            (false, Some(s)) => {
                let e = (v - 1) as u8;
                ranges.push(if s == e {
                    format!("{s:#04x}")
                } else {
                    format!("{s:#04x}-{e:#04x}")
                });
                run_start = None;
            }
            _ => {}
        }
    }
    if ranges.is_empty() {
        println!("  (none; every writable usage has a name)");
    } else {
        println!("  {}  (keypad-calculator block)", ranges.join(", "));
    }
    println!("`hid:` rejects media keys (0x7f-0x81, use media:) and reserved codes.");
}

fn parse_action(action: &str) -> Result<Entry> {
    let a = action.trim();
    if a.eq_ignore_ascii_case("none") {
        return Ok(Entry::Unassigned);
    }
    if let Some(name) = a.strip_prefix("media:") {
        let code = keycode::consumer_from_name(name)
            .ok_or_else(|| anyhow!("unknown media key '{name}'"))?;
        return Ok(Entry::Consumer { code });
    }
    if let Some(hex) = a.strip_prefix("hid:") {
        let hid = parse_u8(hex).context("parsing hid:")?;
        if keycode::is_media_key_hid(hid) {
            bail!(
                "hid:{hid:#04x} is a media key; use media:mute / media:volup / media:voldown instead"
            );
        }
        if !keycode::is_writable_key_hid(hid) {
            bail!("hid:{hid:#04x} is reserved / not a keyboard usage");
        }
        return Ok(Entry::Key { modifier: 0, hid });
    }
    if let Some(name) = a.strip_prefix("mouse:") {
        let action = MouseAction::parse(name).ok_or_else(|| {
            anyhow!("unknown mouse action '{name}' (see `keymap set --valid-actions`)")
        })?;
        return Ok(Entry::Mouse { action });
    }
    if let Some(idx) = a.strip_prefix("macro:") {
        let index: u8 = idx
            .trim()
            .parse()
            .with_context(|| format!("macro index in '{a}' must be 0..=255"))?;
        if index as usize >= vtk6800_core::protocol::macros::MAX_MACROS {
            bail!(
                "macro index {index} out of range (max {})",
                vtk6800_core::protocol::macros::MAX_MACROS - 1
            );
        }
        // count/repeat are derived from the referenced macro's `times` by
        // `resolve_macros` (set the play count on the macro, not the binding).
        return Ok(Entry::Macro {
            index,
            repeat: 0,
            count: 0,
        });
    }
    if let Some(hid) = keycode::hid_from_name(a) {
        return Ok(Entry::Key { modifier: 0, hid });
    }
    if let Some(code) = keycode::consumer_from_name(a) {
        return Ok(Entry::Consumer { code });
    }
    Err(anyhow!(
        "cannot parse action '{a}' (try a key name, media:<name>, mouse:<action>, macro:N, \
         hid:0xNN, or none; see `keymap set --valid-actions`)"
    ))
}

fn parse_u8(s: &str) -> Result<u8> {
    let s = s.trim();
    let v = if let Some(h) = s.strip_prefix("0x") {
        u8::from_str_radix(h, 16)?
    } else {
        s.parse::<u8>()?
    };
    Ok(v)
}

fn hex(report: &[u8; protocol::REPORT_LEN]) -> String {
    report
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn print_keymap(km: &Keymap) {
    println!("variant: {}", km.variant.as_str());
    println!("fn_mode: {}", km.fn_mode.as_str());
    for layer in &km.layers {
        println!("[{:?}]  {} keys", layer.id, layer.keys.len());
        for km in &layer.keys {
            println!("  slot {:>3}: {}", km.slot, km.entry.describe());
        }
    }
}

/// Resolve the effective variant (explicit `--variant`, else the saved default,
/// else ansi68) and its per-variant config paths. The first variant ever used
/// is captured as the default.
fn resolve_variant_paths(
    variant_arg: Option<&str>,
    config_arg: Option<std::path::PathBuf>,
) -> Result<(Variant, config::Paths)> {
    let mut settings = config::Settings::load()?;
    let variant = match variant_arg {
        Some(v) => Variant::parse(v).ok_or_else(|| anyhow!("unknown variant '{v}'"))?,
        None => settings.default_variant().unwrap_or(Variant::Ansi68),
    };
    if settings.default_variant().is_none() {
        settings.default_variant = Some(variant.as_str().to_string());
        settings.save()?;
    }
    let paths = config::Paths::resolve(config_arg, variant)?;
    Ok((variant, paths))
}

/// One `<item>` from a vendor profile export.
struct ProfileItem {
    /// Source physical key: HID code in decimal (`-1` is a sentinel, skipped).
    key_value: i32,
    /// `0` = base layer, `1` = Fn layer.
    fnlayer: u8,
    /// Assignment kind; `2` = single key (the only form imported so far).
    macro_type: u8,
    /// Assigned HID for `macro_type = 2`.
    macro_value: u8,
}

/// Extract `name="value"` from an element's attribute text. Matches the full
/// attribute name so `key_value` is not confused with `layoutkeyvalue`.
fn xml_attr<'a>(attrs: &'a str, name: &str) -> Option<&'a str> {
    let pat = format!("{name}=\"");
    let start = attrs.find(&pat)? + pat.len();
    let rest = &attrs[start..];
    rest.find('"').map(|end| &rest[..end])
}

/// Parse `<item .../>` elements from a vendor profile XML (a small, flat format,
/// hand-parsed to avoid an XML dependency).
fn parse_profile_items(xml: &str) -> Vec<ProfileItem> {
    let mut items = Vec::new();
    for chunk in xml.split("<item").skip(1) {
        let attrs = &chunk[..chunk.find('>').unwrap_or(chunk.len())];
        let Some(key_value) = xml_attr(attrs, "key_value").and_then(|v| v.parse().ok()) else {
            continue;
        };
        items.push(ProfileItem {
            key_value,
            fnlayer: xml_attr(attrs, "fnlayer")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0),
            macro_type: xml_attr(attrs, "macro_type")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0),
            macro_value: xml_attr(attrs, "macro_value")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0),
        });
    }
    items
}

/// Auto-detect the variant whose factory layout contains every source key in
/// the profile. Errors (asking for `--variant`) if zero or several qualify.
fn detect_variant(src_hids: &[u8]) -> Result<Variant> {
    let mut candidates = Vec::new();
    for &v in &Variant::ALL {
        let hids: std::collections::HashSet<u8> =
            v.key_slots()?.into_iter().map(|(_, _, h)| h).collect();
        if src_hids.iter().all(|h| hids.contains(h)) {
            candidates.push(v);
        }
    }
    match candidates.as_slice() {
        [v] => Ok(*v),
        [] => bail!("the profile's keys match no known variant; pass --variant explicitly"),
        many => {
            let names: Vec<_> = many.iter().map(|v| v.as_str()).collect();
            bail!(
                "the profile matches several variants ({}); pass --variant to choose",
                names.join(", ")
            )
        }
    }
}

fn cmd_import(
    file: &std::path::Path,
    variant_arg: Option<&str>,
    config_arg: Option<std::path::PathBuf>,
    to_preset: Option<&str>,
    yes: bool,
) -> Result<()> {
    let xml =
        std::fs::read_to_string(file).with_context(|| format!("reading {}", file.display()))?;

    // A `<macro>` export defines a single macro body; route it to the macro
    // importer, which appends it to the target keymap/preset's macro list.
    if xml.contains("<macroitems") && !xml.contains("<keyitems") {
        return cmd_import_macro(&xml, file, variant_arg, config_arg, to_preset, yes);
    }

    let items = parse_profile_items(&xml);
    if items.is_empty() {
        bail!("no <item> entries found in {}", file.display());
    }

    // The physical keys a profile touches (the `-1` sentinel is dropped) drive
    // variant auto-detection.
    let src_hids: Vec<u8> = items
        .iter()
        .filter_map(|it| u8::try_from(it.key_value).ok())
        .collect();

    let variant = match variant_arg {
        Some(v) => Variant::parse(v).ok_or_else(|| anyhow!("unknown variant '{v}'"))?,
        None => detect_variant(&src_hids)?,
    };

    // Capture the variant as the default on first use, like the other commands.
    let mut settings = config::Settings::load()?;
    if settings.default_variant().is_none() {
        settings.default_variant = Some(variant.as_str().to_string());
        settings.save()?;
    }
    let paths = config::Paths::resolve(config_arg, variant)?;

    let hid_to_slot: std::collections::HashMap<u8, u8> = variant
        .key_slots()?
        .into_iter()
        .map(|(slot, _, hid)| (hid, slot))
        .collect();

    // Destination path (used both to preserve existing macros and to save).
    let dest = match to_preset {
        Some(name) => config::preset_path(variant, name)?,
        None => paths.keymap.clone(),
    };
    // Preserve macros already at the destination so the profile's `macro:`
    // bindings can resolve (import macro bodies first, then the profile).
    let existing_macros = if dest.exists() {
        config::load(&dest).map(|k| k.macros).unwrap_or_default()
    } else {
        Vec::new()
    };

    // Start from the factory default and overlay the profile's assignments: the
    // export lists only non-default keys, so this reproduces the whole profile.
    let mut km = variant.default_keymap()?;
    km.macros = existing_macros;
    let mut applied = 0usize;
    let mut unmapped: Vec<i32> = Vec::new();
    let mut unsupported = 0usize;
    let mut macro_unresolved = 0usize;
    for it in &items {
        let Ok(src) = u8::try_from(it.key_value) else {
            continue; // -1 sentinel
        };
        let Some(&slot) = hid_to_slot.get(&src) else {
            unmapped.push(it.key_value);
            continue;
        };
        // Map the vendor action category (macro_type) to a keymap entry.
        let entry = match it.macro_type {
            2 => Entry::Key {
                modifier: 0,
                hid: it.macro_value,
            },
            5 => match MouseAction::from_vendor_index(it.macro_value) {
                Some(action) => Entry::Mouse { action },
                None => {
                    unsupported += 1;
                    continue;
                }
            },
            6 => match consumer_from_vendor_index(it.macro_value) {
                Some(code) => Entry::Consumer { code },
                None => {
                    unsupported += 1;
                    continue;
                }
            },
            3 => {
                // The profile references a macro by its 1-based id; our index is
                // the 0-based position in the macro list, so subtract one. The
                // count/repeat bytes are derived from the macro's `times` by
                // `resolve_macros` on load.
                let idx = it.macro_value.saturating_sub(1);
                if (idx as usize) < km.macros.len() {
                    Entry::Macro {
                        index: idx,
                        repeat: 0,
                        count: 0,
                    }
                } else {
                    macro_unresolved += 1;
                    continue;
                }
            }
            _ => {
                unsupported += 1;
                continue;
            }
        };
        let layer_id = if it.fnlayer == 1 {
            LayerId::Fn
        } else {
            LayerId::Base
        };
        if let Some(layer) = km.layer_mut(layer_id) {
            layer.set(slot, entry);
            applied += 1;
        }
    }

    // A named preset is confirmed before overwriting; the current keymap is
    // replaced without prompting, as before.
    if let Some(name) = to_preset {
        if dest.exists()
            && !yes
            && !confirm(&format!("Preset '{name}' already exists. Overwrite?"))?
        {
            println!("Aborted.");
            return Ok(());
        }
    }
    config::save(&km, &dest)?;
    match to_preset {
        Some(name) => println!(
            "Imported {applied} assignment(s) from {} into {} preset '{name}'.",
            file.display(),
            variant.as_str()
        ),
        None => println!(
            "Imported {applied} assignment(s) from {} into the {} keymap.",
            file.display(),
            variant.as_str()
        ),
    }
    println!("  {}", dest.display());
    if !unmapped.is_empty() {
        println!(
            "  skipped {} key(s) not on {} (HID codes): {:?}",
            unmapped.len(),
            variant.as_str(),
            unmapped
        );
    }
    if macro_unresolved > 0 {
        println!(
            "  skipped {macro_unresolved} macro binding(s) with no matching macro \
             (import the macro body first, then re-import the profile)."
        );
    }
    if unsupported > 0 {
        println!("  skipped {unsupported} entr(y/ies) of an unsupported action type.");
    }
    match to_preset {
        Some(name) => println!("Load it with `vtk6800 keymap load-preset {name}`."),
        None => {
            println!("Review with `vtk6800 keymap diff`, then apply with `vtk6800 keymap apply`.")
        }
    }
    Ok(())
}

/// Import a vendor `<macro>` export, appending its body to the target keymap or
/// preset's macro list (macros are part of the keymap schema).
fn cmd_import_macro(
    xml: &str,
    file: &std::path::Path,
    variant_arg: Option<&str>,
    config_arg: Option<std::path::PathBuf>,
    to_preset: Option<&str>,
    yes: bool,
) -> Result<()> {
    // <macroinfo name=".." times=".." delaytime=".."/> holds the play count and
    // default inter-event gap; the <item>s are the event stream.
    let info = xml
        .split("<macroinfo")
        .nth(1)
        .map(|c| &c[..c.find('>').unwrap_or(c.len())])
        .unwrap_or("");
    let name = xml_attr(info, "name").unwrap_or("imported").to_string();
    let times: u8 = xml_attr(info, "times")
        .and_then(|v| v.parse().ok())
        .unwrap_or(1);
    let default_delay: u16 = xml_attr(info, "delaytime")
        .and_then(|v| v.parse().ok())
        .unwrap_or(10);
    let (events, skipped) = parse_macro_items(xml);
    if events.is_empty() {
        bail!("no macro events found in {}", file.display());
    }

    // A macro body has no keys, so the variant comes from --variant or the saved
    // default (the macro list lives in the per-variant keymap file).
    let mut settings = config::Settings::load()?;
    let variant = match variant_arg {
        Some(v) => Variant::parse(v).ok_or_else(|| anyhow!("unknown variant '{v}'"))?,
        None => settings.default_variant().unwrap_or(Variant::Ansi68),
    };
    if settings.default_variant().is_none() {
        settings.default_variant = Some(variant.as_str().to_string());
        settings.save()?;
    }
    let paths = config::Paths::resolve(config_arg, variant)?;

    let dest = match to_preset {
        Some(name) => config::preset_path(variant, name)?,
        None => paths.keymap.clone(),
    };
    if to_preset.is_some()
        && dest.exists()
        && !yes
        && !confirm("Append macro to the existing preset?")?
    {
        println!("Aborted.");
        return Ok(());
    }

    // Load the destination keymap (or seed a default) and append the macro.
    let mut km = if dest.exists() {
        config::load(&dest)?
    } else if to_preset.is_some() {
        variant.default_keymap()?
    } else {
        config::load_or_seed(variant, &paths)?
    };
    let index = km.macros.len();
    km.macros.push(Macro {
        events,
        times,
        default_delay,
    });
    // Validate the whole macro set (count / size limits) before writing.
    vtk6800_core::macro_table::encode_macros(&km.macros)
        .context("the imported macro exceeds the device's macro limits")?;
    config::save(&km, &dest)?;

    let where_ = match to_preset {
        Some(p) => format!("{} preset '{p}'", variant.as_str()),
        None => format!("the {} keymap", variant.as_str()),
    };
    println!(
        "Imported macro '{name}' as macro #{index} into {where_} ({} event(s)).",
        km.macros[index].events.len()
    );
    println!("  {}", dest.display());
    if skipped > 0 {
        println!("  skipped {skipped} event(s) with no key/button mapping.");
    }
    println!("Bind a key with `vtk6800 keymap set <layer> <key> macro:{index}`.");
    Ok(())
}

/// Parse a `<macro>` export's `<item>` event list. Macro item `type`:
/// 1 = delay (ms), 2 = key down, 3 = key up, 4 = mouse down, 5 = mouse up. Key
/// `value`s are Windows VK codes (converted to HID); mouse `value`s are
/// 1 = left, 2 = middle, 3 = right. Returns the events and the count skipped.
fn parse_macro_items(xml: &str) -> (Vec<MacroEvent>, usize) {
    let mut events = Vec::new();
    let mut skipped = 0usize;
    for chunk in xml.split("<item").skip(1) {
        let attrs = &chunk[..chunk.find('>').unwrap_or(chunk.len())];
        let Some(ty) = xml_attr(attrs, "type").and_then(|v| v.parse::<u8>().ok()) else {
            continue;
        };
        let value: u16 = xml_attr(attrs, "value")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        let ev = match ty {
            1 => MacroEvent::Delay(value),
            2 => match keycode::hid_from_vk(value) {
                Some(hid) => MacroEvent::KeyDown(hid),
                None => {
                    skipped += 1;
                    continue;
                }
            },
            3 => match keycode::hid_from_vk(value) {
                Some(hid) => MacroEvent::KeyUp(hid),
                None => {
                    skipped += 1;
                    continue;
                }
            },
            4 => match mouse_button_from_vendor(value as u8) {
                Some(b) => MacroEvent::MouseDown(b),
                None => {
                    skipped += 1;
                    continue;
                }
            },
            5 => match mouse_button_from_vendor(value as u8) {
                Some(b) => MacroEvent::MouseUp(b),
                None => {
                    skipped += 1;
                    continue;
                }
            },
            _ => {
                skipped += 1;
                continue;
            }
        };
        events.push(ev);
    }
    (events, skipped)
}

/// Vendor multimedia sub-action index (`macro_type = 6`) to HID consumer code.
fn consumer_from_vendor_index(v: u8) -> Option<u8> {
    Some(match v {
        1 => 0xcd, // play / pause
        2 => 0xb7, // stop
        3 => 0xb6, // previous
        4 => 0xb5, // next
        5 => 0xe9, // volume up
        6 => 0xea, // volume down
        7 => 0xe2, // mute
        _ => return None,
    })
}

/// Vendor macro mouse-button value (1 = left, 2 = middle, 3 = right).
fn mouse_button_from_vendor(v: u8) -> Option<MouseButton> {
    Some(match v {
        1 => MouseButton::Left,
        2 => MouseButton::Middle,
        3 => MouseButton::Right,
        _ => return None,
    })
}

/// Interactive yes/no prompt, defaulting to "no". Without an interactive
/// terminal it answers "no" rather than failing, so previews stay non-destructive
/// in scripts (use `--commit`/`--yes` to act non-interactively).
fn confirm(prompt: &str) -> Result<bool> {
    if !std::io::stdin().is_terminal() {
        return Ok(false);
    }
    Ok(dialoguer::Confirm::new()
        .with_prompt(prompt)
        .default(false)
        .interact()?)
}

fn cmd_save_preset(variant: Variant, paths: &config::Paths, name: &str, yes: bool) -> Result<()> {
    let dest = config::preset_path(variant, name)?;
    let existed = dest.exists();
    if existed && !yes && !confirm(&format!("Preset '{name}' already exists. Overwrite?"))? {
        println!("Aborted.");
        return Ok(());
    }
    let km = config::load_or_seed(variant, paths)?;
    config::save(&km, &dest)?;
    println!(
        "{} current {} keymap as preset '{name}'.",
        if existed { "Overwrote" } else { "Saved" },
        variant.as_str()
    );
    println!("  {}", dest.display());
    Ok(())
}

fn cmd_list_presets(variant: Variant) -> Result<()> {
    let presets = config::list_presets(variant)?;
    if presets.is_empty() {
        println!("No presets saved for {}.", variant.as_str());
    } else {
        println!("{} presets:", variant.as_str());
        for p in &presets {
            println!("  {p}");
        }
    }
    Ok(())
}

fn cmd_delete_preset(variant: Variant, name: &str, yes: bool) -> Result<()> {
    let path = config::preset_path(variant, name)?;
    if !path.exists() {
        let available = config::list_presets(variant)?;
        let hint = if available.is_empty() {
            format!("no presets saved for {}", variant.as_str())
        } else {
            format!("available: {}", available.join(", "))
        };
        bail!("no preset '{name}' for {} ({hint})", variant.as_str());
    }
    if !yes && !confirm(&format!("Delete preset '{name}' for {}?", variant.as_str()))? {
        println!("Aborted.");
        return Ok(());
    }
    std::fs::remove_file(&path).with_context(|| format!("deleting {}", path.display()))?;
    println!("Deleted preset '{name}' ({}).", variant.as_str());
    Ok(())
}

fn cmd_load_preset(variant: Variant, paths: &config::Paths, name: &str) -> Result<()> {
    let src = config::preset_path(variant, name)?;
    if !src.exists() {
        let available = config::list_presets(variant)?;
        let hint = if available.is_empty() {
            format!("no presets saved for {}", variant.as_str())
        } else {
            format!(
                "available {} presets: {}",
                variant.as_str(),
                available.join(", ")
            )
        };
        bail!("no preset '{name}' for {} ({hint})", variant.as_str());
    }
    let km = config::load(&src).with_context(|| format!("loading preset '{name}'"))?;
    if km.variant != variant {
        bail!(
            "preset '{name}' is for {}, not {}",
            km.variant.as_str(),
            variant.as_str()
        );
    }
    config::save(&km, &paths.keymap)?;
    println!(
        "Loaded preset '{name}' into the current {} keymap.",
        variant.as_str()
    );
    println!("Review with `vtk6800 keymap diff`, then apply with `vtk6800 keymap apply`.");
    Ok(())
}

fn cmd_default(variant: Option<&str>) -> Result<()> {
    let mut settings = config::Settings::load()?;
    match variant {
        None => match settings.default_variant() {
            Some(v) => println!("Default variant: {}", v.as_str()),
            None => println!("No default variant set (falls back to ansi68 until one is used)."),
        },
        Some(v) => {
            let variant = Variant::parse(v).ok_or_else(|| anyhow!("unknown variant '{v}'"))?;
            settings.default_variant = Some(variant.as_str().to_string());
            settings.save()?;
            println!("Default variant set to {}.", variant.as_str());
        }
    }
    Ok(())
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    init_tracing(cli.verbose);

    let variant_arg = cli.variant.clone();
    let config_arg = cli.config.clone();

    match cli.cmd {
        Cmd::Devices => cmd_devices(),
        #[cfg(target_os = "linux")]
        Cmd::Udev { action } => cmd_udev(action),
        Cmd::ConnCheck => cmd_conn_check(),
        Cmd::Default { variant } => cmd_default(variant.as_deref()),
        Cmd::Keymap { action } => cmd_keymap(action, variant_arg.as_deref(), config_arg),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xml_attr_matches_full_name_not_substring() {
        let attrs =
            r#"key_value="41" fnlayer="0" layoutkeyvalue="0" macro_type="2" macro_value="175""#;
        // `key_value` must not be confused with `layoutkeyvalue`.
        assert_eq!(xml_attr(attrs, "key_value"), Some("41"));
        assert_eq!(xml_attr(attrs, "fnlayer"), Some("0"));
        assert_eq!(xml_attr(attrs, "macro_type"), Some("2"));
        assert_eq!(xml_attr(attrs, "macro_value"), Some("175"));
        assert_eq!(xml_attr(attrs, "missing"), None);
    }

    #[test]
    fn parses_items_and_skips_sentinel() {
        let xml = r#"<profile><keyitems>
        <item key_value="-1" fnlayer="0" macro_type="2" macro_value="175"/>
        <item key_value="57" fnlayer="0" macro_type="2" macro_value="41"/>
        <item key_value="24" fnlayer="1" macro_type="2" macro_value="36"/>
        </keyitems></profile>"#;
        let items = parse_profile_items(xml);
        assert_eq!(items.len(), 3);
        // The -1 sentinel is parsed but dropped when mapping to keys.
        let real: Vec<_> = items
            .iter()
            .filter(|i| u8::try_from(i.key_value).is_ok())
            .collect();
        assert_eq!(real.len(), 2);
        assert_eq!(real[0].key_value, 57);
        assert_eq!(real[0].fnlayer, 0);
        assert_eq!(real[0].macro_value, 41);
        assert_eq!(real[1].fnlayer, 1);
    }
}
