//! How an app is built for a device: either with the device's custom target
//! spec (installed by `cargo ledger setup`), or with the built-in Rust triple
//! of its CPU plus the settings that the custom spec carries.
//!
//! In both cases the device is also exposed to the code as the
//! `ledger_device` cfg, so that apps can stop relying on `target_os`.

use std::process::Command;

use cargo_metadata::camino::{Utf8Path, Utf8PathBuf};

use crate::Device;

impl Device {
    /// Built-in Rust triple matching the device's CPU.
    pub fn builtin_triple(self) -> &'static str {
        match self {
            Device::Nanox => "thumbv6m-none-eabi",
            Device::Nanosplus | Device::Stax | Device::Flex | Device::ApexP => {
                "thumbv8m.main-none-eabi"
            }
        }
    }
}

/// Where and how to build an app for a device.
pub struct BuildTarget {
    /// Value of `--target`.
    pub target: String,
    /// Value of `--target-dir`, when the default one cannot be used.
    pub target_dir: Option<Utf8PathBuf>,
    /// Directory where the ELF is expected by the rest of the tooling
    /// (`<target-dir>/<device>/release`), when Cargo puts it elsewhere.
    pub release_dir: Option<Utf8PathBuf>,
    /// Rustflags of the build, to pass as `CARGO_ENCODED_RUSTFLAGS`.
    pub rustflags: Vec<String>,
}

impl BuildTarget {
    pub fn new(device: Device, builtin: bool, target_dir: &Utf8Path) -> Self {
        let mut rustflags = user_rustflags(device);
        if builtin {
            // Settings of the custom target spec that differ from the
            // built-in triple.
            rustflags.extend(
                [
                    "-Crelocation-model=ropi-rwpi",
                    "-Clinker=link_wrap.sh",
                    &format!("-Clink-arg=-T{device}_layout.ld"),
                    "-Clink-arg=-Tlink.ld",
                    "-Clink-arg=--wrap=__stack_chk_fail",
                    "-Clink-arg=--wrap=__stack_chk_init",
                ]
                .map(String::from),
            );
        }
        rustflags.push(format!("--cfg=ledger_device=\"{device}\""));

        if builtin {
            // Devices sharing a triple would overwrite each other's
            // artifacts in a shared target directory.
            BuildTarget {
                target: device.builtin_triple().to_string(),
                target_dir: Some(
                    target_dir.join("builtin").join(device.as_ref()),
                ),
                release_dir: Some(
                    target_dir.join(device.as_ref()).join("release"),
                ),
                rustflags,
            }
        } else {
            BuildTarget {
                target: device.to_string(),
                target_dir: None,
                release_dir: None,
                rustflags,
            }
        }
    }
}

/// The rustflags Cargo would use for the custom target of `device`, from
/// the first source that sets them: `CARGO_ENCODED_RUSTFLAGS`, `RUSTFLAGS`,
/// `target.<device>.rustflags`, `build.rustflags`.
///
/// The build passes its own flags through `CARGO_ENCODED_RUSTFLAGS`, which
/// overrides all of these, so they are carried over here. This keeps the
/// `[target.<device>]` sections of the apps' Cargo config working with the
/// built-in triples.
fn user_rustflags(device: Device) -> Vec<String> {
    if let Ok(flags) = std::env::var("CARGO_ENCODED_RUSTFLAGS")
        && !flags.is_empty()
    {
        return flags.split('\x1f').map(String::from).collect();
    }
    if let Ok(flags) = std::env::var("RUSTFLAGS") {
        return flags.split_whitespace().map(String::from).collect();
    }
    config_rustflags(&format!("target.{device}.rustflags"))
        .or_else(|| config_rustflags("build.rustflags"))
        .unwrap_or_default()
}

/// Reads a rustflags value from the Cargo configuration (all config files
/// merged, as Cargo sees them from the current directory).
fn config_rustflags(key: &str) -> Option<Vec<String>> {
    let output = Command::new("cargo")
        .args(["-Zunstable-options", "config", "get", "--format=json-value"])
        .arg(key)
        .output()
        .ok()?;
    if !output.status.success() {
        // Unset key (or a cargo without `config get`).
        return None;
    }
    match serde_json::from_slice(&output.stdout).ok()? {
        serde_json::Value::String(s) => {
            Some(s.split_whitespace().map(String::from).collect())
        }
        serde_json::Value::Array(values) => values
            .into_iter()
            .map(|v| v.as_str().map(String::from))
            .collect(),
        _ => None,
    }
}
