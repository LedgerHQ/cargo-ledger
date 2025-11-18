use std::error::Error;
use std::fmt::{Display, Formatter};
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::process::Stdio;

use cargo_metadata::{Message, Package};
use clap::{Parser, Subcommand, ValueEnum};
use serde_derive::Deserialize;
use serde_json::json;

mod error;
use crate::error::LedgerError;

use setup::install_targets;
use utils::*;

mod setup;
mod utils;

#[derive(Debug, Deserialize)]
struct LedgerMetadata {
    curve: Vec<String>,
    path: Vec<String>,
    flags: Option<String>,
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DeviceMetadata {
    icon: String,
    flags: Option<String>,
}

#[derive(Parser, Debug)]
#[command(name = "cargo")]
#[command(bin_name = "cargo")]
#[clap(name = "Ledger devices build and load commands")]
#[clap(version = "0.0")]
#[clap(about = "Builds the project and emits a JSON manifest for ledgerctl.")]
enum Cli {
    Ledger(CliArgs),
}

#[derive(clap::Args, Debug)]
struct CliArgs {
    #[clap(long)]
    #[clap(value_name = "prebuilt ELF exe")]
    use_prebuilt: Option<PathBuf>,

    #[clap(subcommand)]
    command: MainCommand,
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq)]
enum Device {
    Nanox,
    Nanosplus,
    Stax,
    Flex,
    #[clap(name = "apex_p")]
    ApexP,
}

impl Display for Device {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_ref())
    }
}

impl AsRef<str> for Device {
    fn as_ref(&self) -> &str {
        match self {
            Device::Nanox => "nanox",
            Device::Nanosplus => "nanosplus",
            Device::Stax => "stax",
            Device::Flex => "flex",
            Device::ApexP => "apex_p",
        }
    }
}

#[derive(Subcommand, Debug)]
enum MainCommand {
    #[clap(about = "install custom target files")]
    Setup,
    #[clap(about = "build the project for a given device")]
    Build {
        #[clap(value_enum)]
        #[clap(help = "device to build for")]
        device: Device,
        #[clap(short, long)]
        #[clap(help = "load on a device")]
        load: bool,
        #[clap(last = true)]
        remaining_args: Vec<String>,
    },
}

fn main() {
    if let Err(e) = entrypoint() {
        eprintln!("Error: {e}");
        // Show source chain if any
        let mut src = e.source();
        while let Some(cause) = src {
            eprintln!("  caused by: {cause}");
            src = cause.source();
        }
        std::process::exit(1);
    }
}

fn entrypoint() -> Result<(), LedgerError> {
    let Cli::Ledger(cli) = Cli::parse();
    match cli.command {
        MainCommand::Setup => {
            install_targets()?;
        }
        MainCommand::Build {
            device: d,
            load: a,
            remaining_args: r,
        } => {
            build_app(d, a, cli.use_prebuilt, r)?;
        }
    }
    Ok(())
}

fn retrieve_metadata(
    device: Device,
    manifest_path: Option<&str>,
) -> Result<(Package, LedgerMetadata, DeviceMetadata), LedgerError> {
    let mut cmd = cargo_metadata::MetadataCommand::new();

    // Only used during tests
    if let Some(manifestpath) = manifest_path {
        cmd = cmd.manifest_path(manifestpath).clone();
    }

    let res = cmd.no_deps().exec()?;

    let this_pkg = res.packages.last().ok_or(LedgerError::MissingPackage)?;
    let metadata_section = this_pkg.metadata.get("ledger");

    let Some(metadatasection) = metadata_section else {
        return Err(LedgerError::MissingMetadataSection("ledger".to_string()));
    };
    let device_obj = metadatasection
        .clone()
        .get(device.as_ref())
        .ok_or(LedgerError::MissingMetadataSection(
            device.as_ref().to_string(),
        ))?
        .clone();

    let ledger_metadata: LedgerMetadata =
        serde_json::from_value(metadatasection.clone())?;
    let device_metadata: DeviceMetadata = serde_json::from_value(device_obj)?;
    Ok((this_pkg.clone(), ledger_metadata, device_metadata))
}

fn build_app(
    device: Device,
    is_load: bool,
    use_prebuilt: Option<PathBuf>,
    remaining_args: Vec<String>,
) -> Result<(), LedgerError> {
    let exe_path = match use_prebuilt {
        None => {
            let mut args: Vec<String> = vec![];

            args.push(String::from("build"));
            args.push(String::from("--release"));
            args.push(format!("--target={}", device.as_ref()));
            args.push(String::from(
                "--message-format=json-diagnostic-rendered-ansi",
            ));

            let mut cargo_cmd = Command::new("cargo")
                .args(args)
                .args(&remaining_args)
                .stdout(Stdio::piped())
                .spawn()?;

            let mut exe_path = PathBuf::new();
            let out = cargo_cmd.stdout.take().ok_or_else(|| {
                LedgerError::Other("Failed to take cargo stdout".into())
            })?;
            let reader = std::io::BufReader::new(out);
            for message in Message::parse_stream(reader) {
                match message.map_err(|e| {
                    LedgerError::Other(format!("Message stream error: {e}"))
                })? {
                    Message::CompilerArtifact(artifact) => {
                        if let Some(n) = &artifact.executable {
                            exe_path = n.to_path_buf();
                        }
                    }
                    Message::CompilerMessage(message) => {
                        println!("{message}");
                    }
                    _ => {}
                }
            }
            let status = cargo_cmd.wait()?;
            if !status.success() {
                return Err(LedgerError::CommandFailure {
                    cmd: "cargo build",
                    status: status.code(),
                    stderr: String::new(),
                });
            }

            exe_path
        }
        Some(prebuilt) => prebuilt.canonicalize()?,
    };
    let (this_pkg, metadata_ledger, metadata_device) =
        retrieve_metadata(device, None)?;

    let package_path = this_pkg
        .manifest_path
        .parent()
        .ok_or(LedgerError::MissingField("package parent path"))?;

    /* exe_path = "exe_parent" + "exe_name" */
    let exe_name = exe_path
        .file_name()
        .ok_or(LedgerError::MissingField("exe file name"))?;
    let exe_parent = exe_path
        .parent()
        .ok_or(LedgerError::MissingField("exe parent"))?;

    let hex_file_abs = exe_parent.join(exe_name).with_extension("hex");

    let hex_file = hex_file_abs
        .strip_prefix(exe_parent)
        .map_err(|e| LedgerError::Other(format!("Strip prefix error: {e}")))?;

    export_binary(&exe_path, &hex_file_abs)?;

    // app.json will be placed next to hex file
    let app_json_name = format!("app_{}.json", device.as_ref());
    let app_json = exe_parent.join(app_json_name);

    // Retrieve real data size and SDK infos from ELF
    let infos = retrieve_infos(&exe_path)?;

    let flags = match metadata_device.flags {
        Some(flags) => flags,
        None => match metadata_ledger.flags {
            Some(flags) => match device {
                // Modify flags to enable BLE if targeting Nano X
                Device::Nanosplus => flags,
                Device::Nanox | Device::Stax | Device::Flex | Device::ApexP => {
                    let base =
                        u32::from_str_radix(flags.trim_start_matches("0x"), 16)
                            .unwrap_or(0);
                    format!("0x{:x}", base | 0x200)
                }
            },
            None => String::from("0x000"),
        },
    };

    // Target ID according to target, in case it
    // is not present in the retrieved ELF infos.
    let backup_targetid: String = match device {
        Device::Nanox => String::from("0x33000004"),
        Device::Nanosplus => String::from("0x33100004"),
        Device::Stax => String::from("0x33200004"),
        Device::Flex => String::from("0x33300004"),
        Device::ApexP => String::from("0x33400004"),
    };

    // create manifest
    let file = fs::File::create(&app_json)?;
    let mut json = json!({
        "name": metadata_ledger.name.as_ref().unwrap_or(&this_pkg.name),
        "version": &this_pkg.version,
        "icon": metadata_device.icon,
        "targetId": infos.target_id.unwrap_or(backup_targetid),
        "flags": flags,
        "derivationPath": {
            "curves": metadata_ledger.curve,
            "paths": metadata_ledger.path
        },
        "binary": hex_file,
        "dataSize": infos.size
    });

    json["apiLevel"] = infos.api_level.into();
    serde_json::to_writer_pretty(file, &json)?;

    // Copy icon to the same directory as the app.json
    let icon_path = package_path.join(&metadata_device.icon);
    let icon_dest = exe_parent.join(
        metadata_device
            .icon
            .split('/')
            .last()
            .ok_or(LedgerError::MissingField("icon file name"))?,
    );
    fs::copy(icon_path, icon_dest)?;

    // Use ledgerctl to dump the APDU installation file.
    // Either dump to the location provided by the --out-dir cargo
    // argument if provided or use the default binary path.
    let output_dir: Option<PathBuf> = remaining_args
        .iter()
        .position(|arg| arg == "--out-dir" || arg.starts_with("--out-dir="))
        .and_then(|index| {
            let out_dir_arg = &remaining_args[index];
            // Extracting the value from "--out-dir=<some value>" or "--out-dir <some value>"
            if out_dir_arg.contains('=') {
                out_dir_arg.split('=').nth(1).map(|s| s.to_string())
            } else {
                remaining_args
                    .get(index + 1)
                    .map(|path_str| path_str.to_string())
            }
        })
        .map(PathBuf::from);
    let exe_filename = exe_path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or(LedgerError::MissingField("exe filename str"))?;
    let exe_parent = exe_path
        .parent()
        .ok_or(LedgerError::MissingField("exe parent"))?
        .to_path_buf();
    let apdu_file_path = output_dir
        .unwrap_or(exe_parent)
        .join(exe_filename)
        .with_extension("apdu");
    dump_with_ledgerctl(
        package_path,
        &app_json,
        apdu_file_path
            .to_str()
            .ok_or(LedgerError::MissingField("apdu path"))?,
    )?;

    if is_load {
        install_with_ledgerctl(package_path, &app_json)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_metadata() {
        match retrieve_metadata(Device::Flex, Some("./tests/valid/Cargo.toml"))
        {
            Ok(res) => {
                let (_, metadata_ledger, _metadata_nanos) = res;
                assert_eq!(metadata_ledger.name, Some("TestApp".to_string()));
                assert_eq!(metadata_ledger.curve, ["secp256k1"]);
                assert_eq!(metadata_ledger.flags, Some(String::from("0x38")));
                assert_eq!(metadata_ledger.path, ["'44/123"]);
            }
            Err(e) => panic!("Failed to retrieve metadata: {}", e),
        };
    }

    #[test]
    fn valid_metadata_variant() {
        match retrieve_metadata(
            Device::Flex,
            Some("./tests/valid_variant/Cargo.toml"),
        ) {
            Ok(res) => {
                let (_, metadata_ledger, _metadata_nanos) = res;
                assert_eq!(metadata_ledger.name, Some("TestApp".to_string()));
                assert_eq!(metadata_ledger.curve, ["secp256k1"]);
                assert_eq!(metadata_ledger.flags, Some(String::from("0x38")));
                assert_eq!(metadata_ledger.path, ["'44/123"]);
            }
            Err(e) => panic!("Failed to retrieve metadata: {}", e),
        };
    }

    #[test]
    fn valid_outdated_metadata() {
        match retrieve_metadata(
            Device::Flex,
            Some("./tests/valid_outdated/Cargo.toml"),
        ) {
            Ok(res) => {
                let (_, metadata_ledger, _metadata_nanos) = res;
                assert_eq!(metadata_ledger.name, Some("TestApp".to_string()));
                assert_eq!(metadata_ledger.curve, ["secp256k1"]);
                assert_eq!(metadata_ledger.flags, Some(String::from("0x38")));
                assert_eq!(metadata_ledger.path, ["'44/123"]);
            }
            Err(e) => panic!("Failed to retrieve metadata: {}", e),
        };
    }
}
