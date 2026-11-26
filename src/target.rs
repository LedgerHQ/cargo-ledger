//! How an app is built for a device: with the device's custom target spec
//! (installed by `cargo ledger setup`), or, when the app opts in with
//! `builtin_target = true` in `[package.metadata.ledger]`, with the built-in
//! Rust triple of its CPU plus the settings that the custom spec carries.
//!
//! In built-in mode the settings go to `build.rustflags` through `--config`.
//! Cargo merges arrays across config sources, so they are added to the
//! app's own `[build] rustflags`. Per-device flags of the app, which the
//! triple cannot select, come from `[package.metadata.ledger.<device>]
//! rustflags`. A `[target.<triple>]` or `[target.'cfg(..)']` rustflags
//! section, or `RUSTFLAGS`, would replace these flags: the SDK's build
//! script then fails, as the `ledger_device` cfg is missing.

use cargo_metadata::camino::{Utf8Path, Utf8PathBuf};
use serde_json::Value;

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
    /// Extra `cargo build` arguments (`--target-dir`, `--config`).
    pub cargo_args: Vec<String>,
    /// Directory where the ELF is expected by the rest of the tooling
    /// (`<target-dir>/<device>/release`), when Cargo puts it elsewhere.
    pub release_dir: Option<Utf8PathBuf>,
}

impl BuildTarget {
    /// `metadata` is the `package.metadata` value of the app.
    pub fn new(
        device: Device,
        metadata: &Value,
        target_dir: &Utf8Path,
    ) -> Self {
        let ledger = &metadata["ledger"];
        if ledger["builtin_target"].as_bool() != Some(true) {
            return BuildTarget {
                target: device.to_string(),
                cargo_args: vec![],
                release_dir: None,
            };
        }

        // Settings of the custom target spec that the triple lacks.
        let mut rustflags: Vec<String> = [
            "-Crelocation-model=ropi-rwpi",
            "-Clinker=link_wrap.sh",
            &format!("-Clink-arg=-T{device}_layout.ld"),
            "-Clink-arg=-Tlink.ld",
            "-Clink-arg=--wrap=__stack_chk_fail",
            "-Clink-arg=--wrap=__stack_chk_init",
            &format!("--cfg=ledger_device=\"{device}\""),
        ]
        .map(String::from)
        .into();
        if let Some(flags) = ledger[device.as_ref()]["rustflags"].as_array() {
            rustflags.extend(
                flags.iter().filter_map(|f| f.as_str().map(String::from)),
            );
        }
        let rustflags = rustflags
            .iter()
            .map(|f| toml_string(f))
            .collect::<Vec<_>>()
            .join(", ");

        // Devices sharing a triple would overwrite each other's artifacts
        // in a shared target directory.
        BuildTarget {
            target: device.builtin_triple().to_string(),
            cargo_args: vec![
                format!(
                    "--target-dir={}",
                    target_dir.join("builtin").join(device.as_ref())
                ),
                "--config".to_string(),
                format!("build.rustflags = [{rustflags}]"),
            ],
            release_dir: Some(target_dir.join(device.as_ref()).join("release")),
        }
    }
}

/// `s` as a TOML basic string.
fn toml_string(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build(device: Device, metadata: Value) -> BuildTarget {
        BuildTarget::new(device, &metadata, Utf8Path::new("/app/target"))
    }

    #[test]
    fn custom_target_by_default() {
        let t = build(Device::Stax, serde_json::json!({ "ledger": {} }));
        assert_eq!(t.target, "stax");
        assert!(t.cargo_args.is_empty());
        assert!(t.release_dir.is_none());
        let t = build(Device::Stax, Value::Null);
        assert_eq!(t.target, "stax");
    }

    #[test]
    fn builtin_target_opt_in() {
        let t = build(
            Device::Nanox,
            serde_json::json!({ "ledger": {
                "builtin_target": true,
                "nanox": { "rustflags": ["-Copt-level=s"] },
                "stax": { "rustflags": ["-Cfoo"] },
            }}),
        );
        assert_eq!(t.target, "thumbv6m-none-eabi");
        assert_eq!(t.cargo_args[0], "--target-dir=/app/target/builtin/nanox");
        assert_eq!(t.cargo_args[1], "--config");
        assert_eq!(
            t.cargo_args[2],
            "build.rustflags = [\"-Crelocation-model=ropi-rwpi\", \
             \"-Clinker=link_wrap.sh\", \"-Clink-arg=-Tnanox_layout.ld\", \
             \"-Clink-arg=-Tlink.ld\", \"-Clink-arg=--wrap=__stack_chk_fail\", \
             \"-Clink-arg=--wrap=__stack_chk_init\", \
             \"--cfg=ledger_device=\\\"nanox\\\"\", \"-Copt-level=s\"]"
        );
        assert_eq!(t.release_dir.unwrap(), "/app/target/nanox/release");
    }

    #[test]
    fn shared_triple() {
        let t = build(
            Device::ApexP,
            serde_json::json!({ "ledger": { "builtin_target": true } }),
        );
        assert_eq!(t.target, "thumbv8m.main-none-eabi");
        assert_eq!(t.cargo_args[0], "--target-dir=/app/target/builtin/apex_p");
        assert!(t.cargo_args[2].contains("-Tapex_p_layout.ld"));
    }
}
