use std::collections::HashMap;
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::process::Command;
use std::process::Stdio;

use cargo_metadata::Message;
use cargo_metadata::camino::Utf8PathBuf;
use clap::{Parser, Subcommand, ValueEnum};

mod error;
use crate::error::LedgerError;

use setup::install_targets;
use utils::*;

mod setup;
mod utils;

#[derive(Parser, Debug)]
#[command(name = "cargo")]
#[command(bin_name = "cargo")]
#[clap(name = "Ledger devices build and load commands")]
#[clap(version = "1.13.0")]
#[clap(about = "Builds the project and emits a JSON manifest for ledgerctl.")]
enum Cli {
    Ledger(CliArgs),
}

#[derive(clap::Args, Debug)]
struct CliArgs {
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
    Setup {
        #[clap(short, long)]
        #[clap(help = "git tag or branch to use")]
        tag: Option<String>,
    },
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
        MainCommand::Setup { tag: t } => {
            install_targets(t)?;
        }
        MainCommand::Build {
            device: d,
            load: a,
            remaining_args: r,
        } => {
            build_app(d, a, r)?;
        }
    }
    Ok(())
}

fn build_app(
    device: Device,
    is_load: bool,
    remaining_args: Vec<String>,
) -> Result<(), LedgerError> {
    let elf_path = {
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

        let mut elf_path = Utf8PathBuf::new();
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
                        elf_path = n.to_path_buf();
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
        elf_path
    };

    // Retrieve package path
    let mut cmd = cargo_metadata::MetadataCommand::new();
    let res = cmd.no_deps().exec()?;
    let this_pkg = res.packages.last().ok_or(LedgerError::MissingPackage)?;
    let package_path = this_pkg
        .manifest_path
        .parent()
        .ok_or(LedgerError::MissingField("package parent path"))?;

    // Retrieve hex path and export binary
    let hex_path = elf_path.with_extension("hex");
    println!("Exporting binary from ELF {} to {}", elf_path, hex_path);
    export_binary(&elf_path, &hex_path)?;

    // Retrieve info from ELF
    let mut infos = retrieve_infos(&elf_path)?;
    println!("Retrieved ELF infos: {:?}", infos);
    infos.app_flags = match device {
        // Modify flags to enable BLE if targeting Nano X
        Device::Nanosplus => infos.app_flags,
        Device::Nanox | Device::Stax | Device::Flex | Device::ApexP => {
            let base = u32::from_str_radix(
                infos.app_flags.trim_start_matches("0x"),
                16,
            )
            .unwrap_or(0);
            format!("0x{:x}", base | 0x200)
        }
    };

    // Dump with ledgerblue and optionally install
    let mut lb_params: HashMap<String, String> = HashMap::new();
    lb_params.insert("name".to_string(), infos.app_name);
    lb_params.insert("targetId".to_string(), infos.target_id);
    lb_params.insert("apiLevel".to_string(), infos.api_level);
    lb_params.insert("flags".to_string(), infos.app_flags);
    lb_params.insert("binary".to_string(), hex_path.clone().into_string());
    lb_params.insert("dataSize".to_string(), infos.data_size.to_string());
    lb_params.insert(
        "installParamsSize".to_string(),
        infos.install_params_size.to_string(),
    );

    let apdu_path = elf_path.with_extension("apdu");
    dump_with_ledgerblue(package_path, &lb_params, &apdu_path)?;
    if is_load {
        install_with_ledgerblue(package_path, &lb_params, &apdu_path)?;
    }

    remaining_args
        .iter()
        .find(|arg| arg.starts_with("--artifact-dir="))
        .and_then(|arg| {
            let out_dir = arg.trim_start_matches("--artifact-dir=");
            let out_path =
                Utf8PathBuf::from(out_dir).join(hex_path.file_name().unwrap());
            std::fs::copy(&hex_path, &out_path).ok()?;
            let out_path =
                Utf8PathBuf::from(out_dir).join(apdu_path.file_name().unwrap());
            std::fs::copy(&apdu_path, &out_path).ok()?;
            Some(())
        });

    Ok(())
}

// #[cfg(test)]
// mod tests {
//     use super::*;

//     #[test]
//     fn valid_metadata() {
//         match retrieve_metadata(Device::Flex, Some("./tests/valid/Cargo.toml"))
//         {
//             Ok(res) => {
//                 let (_, metadata_ledger, _metadata_nanos) = res;
//                 assert_eq!(metadata_ledger.name, Some("TestApp".to_string()));
//                 assert_eq!(metadata_ledger.curve, ["secp256k1"]);
//                 assert_eq!(metadata_ledger.flags, Some(String::from("0x38")));
//                 assert_eq!(metadata_ledger.path, ["'44/123"]);
//                 assert_eq!(
//                     metadata_ledger.path_slip21,
//                     Some(vec!["LEDGER".into()])
//                 );
//             }
//             Err(e) => panic!("Failed to retrieve metadata: {}", e),
//         };
//     }

//     #[test]
//     fn valid_metadata_variant() {
//         match retrieve_metadata(
//             Device::Flex,
//             Some("./tests/valid_variant/Cargo.toml"),
//         ) {
//             Ok(res) => {
//                 let (_, metadata_ledger, _metadata_nanos) = res;
//                 assert_eq!(metadata_ledger.name, Some("TestApp".to_string()));
//                 assert_eq!(metadata_ledger.curve, ["secp256k1"]);
//                 assert_eq!(metadata_ledger.flags, Some(String::from("0x38")));
//                 assert_eq!(metadata_ledger.path, ["'44/123"]);
//                 assert_eq!(
//                     metadata_ledger.path_slip21,
//                     Some(vec!["LEDGER".into()])
//                 );
//             }
//             Err(e) => panic!("Failed to retrieve metadata: {}", e),
//         };
//     }

//     #[test]
//     fn valid_outdated_metadata() {
//         match retrieve_metadata(
//             Device::Flex,
//             Some("./tests/valid_outdated/Cargo.toml"),
//         ) {
//             Ok(res) => {
//                 let (_, metadata_ledger, _metadata_nanos) = res;
//                 assert_eq!(metadata_ledger.name, Some("TestApp".to_string()));
//                 assert_eq!(metadata_ledger.curve, ["secp256k1"]);
//                 assert_eq!(metadata_ledger.flags, Some(String::from("0x38")));
//                 assert_eq!(metadata_ledger.path, ["'44/123"]);
//                 assert_eq!(
//                     metadata_ledger.path_slip21,
//                     Some(vec!["LEDGER".into()])
//                 );
//             }
//             Err(e) => panic!("Failed to retrieve metadata: {}", e),
//         };
//     }
// }
