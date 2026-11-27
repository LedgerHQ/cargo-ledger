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

use setup::{TargetSource, install_targets};
use utils::*;

mod setup;
mod target;
mod utils;

use target::BuildTarget;

#[derive(Parser, Debug)]
#[command(name = "cargo")]
#[command(bin_name = "cargo")]
#[clap(name = "Ledger devices build and load commands")]
#[clap(version = env!("CARGO_PKG_VERSION"))]
#[clap(
    about = "Builds the project and generates the APDU file with ledgerblue."
)]
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
    #[clap(
        about = "install custom target files",
        long_about = "Install the custom target specs and link_wrap.sh into the active toolchain's sysroot.\n\nBy default they are taken from the latest release of the ledger_secure_sdk_sys crate on crates.io (checksum-verified). Use --sys-version to pin a release, or --tag to take them from a git ref of the Rust SDK repository instead."
    )]
    Setup {
        #[clap(short, long, conflicts_with = "sys_version")]
        #[clap(help = "git tag or branch of the Rust SDK repository to use")]
        tag: Option<String>,
        #[clap(long, value_name = "VERSION")]
        #[clap(
            help = "ledger_secure_sdk_sys release to use (default: latest on crates.io)"
        )]
        sys_version: Option<String>,
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
    #[clap(
        about = "run cargo clippy for a given device",
        long_about = "Run cargo clippy with the same target and flags as `cargo ledger build`: the custom target spec of the device, or its built-in triple when the app opts in with `builtin_target`.\n\nArguments after `--` go to cargo clippy, e.g. `cargo ledger clippy nanox -- --features foo -- -Dwarnings`."
    )]
    Clippy {
        #[clap(value_enum)]
        #[clap(help = "device to lint for")]
        device: Device,
        #[clap(last = true)]
        remaining_args: Vec<String>,
    },
    #[clap(
        about = "run cargo check for a given device",
        long_about = "Run cargo check with the same target and flags as `cargo ledger build`: the custom target spec of the device, or its built-in triple when the app opts in with `builtin_target`.\n\nArguments after `--` go to cargo check."
    )]
    Check {
        #[clap(value_enum)]
        #[clap(help = "device to check for")]
        device: Device,
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
        MainCommand::Setup { tag, sys_version } => {
            let source = match tag {
                Some(git_ref) => TargetSource::GitRef(git_ref),
                None => TargetSource::CratesIo(sys_version),
            };
            install_targets(source)?;
        }
        MainCommand::Build {
            device: d,
            load: a,
            remaining_args: r,
        } => {
            build_app(d, a, r)?;
        }
        MainCommand::Clippy {
            device,
            remaining_args,
        } => run_cargo("clippy", device, remaining_args)?,
        MainCommand::Check {
            device,
            remaining_args,
        } => run_cargo("check", device, remaining_args)?,
    }
    Ok(())
}

/// Target and flags for building the app of the current directory for
/// `device`, with the path of the app package.
fn app_build_target(
    device: Device,
) -> Result<(BuildTarget, Utf8PathBuf), LedgerError> {
    let mut cmd = cargo_metadata::MetadataCommand::new();
    let res = cmd.no_deps().exec()?;
    let this_pkg = res.packages.last().ok_or(LedgerError::MissingPackage)?;
    let package_path = this_pkg
        .manifest_path
        .parent()
        .ok_or(LedgerError::MissingField("package parent path"))?
        .to_path_buf();
    let build_target =
        BuildTarget::new(device, &this_pkg.metadata, &res.target_directory);
    Ok((build_target, package_path))
}

/// `cargo <subcommand>` arguments selecting the target of `build_target`.
fn cargo_args(subcommand: &str, build_target: &BuildTarget) -> Vec<String> {
    let mut args = vec![
        subcommand.to_string(),
        format!("--target={}", build_target.target),
    ];
    args.extend(build_target.cargo_args.iter().cloned());
    args
}

/// Run `cargo <subcommand>` for `device`, with the target and flags of
/// `cargo ledger build`, and exit with its status when it fails.
fn run_cargo(
    subcommand: &str,
    device: Device,
    remaining_args: Vec<String>,
) -> Result<(), LedgerError> {
    let (build_target, _) = app_build_target(device)?;
    let args = cargo_args(subcommand, &build_target);
    println!("Running cargo {}", args.join(" "));
    let status = Command::new("cargo")
        .args(args)
        .args(&remaining_args)
        .status()?;
    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }
    Ok(())
}

fn build_app(
    device: Device,
    is_load: bool,
    remaining_args: Vec<String>,
) -> Result<(), LedgerError> {
    let (build_target, package_path) = app_build_target(device)?;
    println!(
        "Building for target {} {}",
        build_target.target,
        build_target.cargo_args.join(" ")
    );

    let elf_path = {
        let mut args = cargo_args("build", &build_target);
        args.push(String::from("--release"));
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
        // Put the ELF where the rest of the tooling (Ragger, Speculos
        // commands, CI) expects it, so that the outputs derived from it
        // land there too.
        match &build_target.release_dir {
            Some(dir) => {
                let file_name = elf_path
                    .file_name()
                    .ok_or(LedgerError::MissingField("ELF file name"))?;
                std::fs::create_dir_all(dir)?;
                let dest = dir.join(file_name);
                std::fs::copy(&elf_path, &dest)?;
                dest
            }
            None => elf_path,
        }
    };

    // Retrieve hex path and export binary
    let hex_path = elf_path.with_extension("hex");
    println!("Exporting binary from ELF {} to {}", elf_path, hex_path);
    export_binary(&elf_path, &hex_path)?;

    // Retrieve info from ELF
    let infos = retrieve_infos(&elf_path)?;
    println!("Retrieved ELF infos: {:?}", infos);

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
    dump_with_ledgerblue(&package_path, &lb_params, &apdu_path)?;
    if is_load {
        install_with_ledgerblue(&package_path, &lb_params, &apdu_path)?;
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
            let sha256_path = apdu_path.with_extension("sha256");
            if sha256_path.exists() {
                let out_path = Utf8PathBuf::from(out_dir)
                    .join(sha256_path.file_name().unwrap());
                std::fs::copy(&sha256_path, &out_path).ok()?;
            }
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

#[cfg(test)]
mod cli_tests {
    use super::*;
    use cargo_metadata::camino::Utf8Path;

    fn parse(args: &[&str]) -> MainCommand {
        let Cli::Ledger(cli) =
            Cli::try_parse_from(["cargo", "ledger"].iter().chain(args.iter()))
                .unwrap();
        cli.command
    }

    #[test]
    fn clippy_args_pass_through() {
        match parse(&[
            "clippy",
            "nanosplus",
            "--",
            "--features",
            "foo",
            "--",
            "-Dwarnings",
        ]) {
            MainCommand::Clippy {
                device,
                remaining_args,
            } => {
                assert_eq!(device, Device::Nanosplus);
                assert_eq!(
                    remaining_args,
                    ["--features", "foo", "--", "-Dwarnings"]
                );
            }
            c => panic!("unexpected command {c:?}"),
        }
        match parse(&["check", "apex_p"]) {
            MainCommand::Check {
                device,
                remaining_args,
            } => {
                assert_eq!(device, Device::ApexP);
                assert!(remaining_args.is_empty());
            }
            c => panic!("unexpected command {c:?}"),
        }
    }

    #[test]
    fn cargo_args_custom_target() {
        let t = BuildTarget::new(
            Device::Stax,
            &serde_json::json!({ "ledger": {} }),
            Utf8Path::new("/app/target"),
        );
        assert_eq!(cargo_args("clippy", &t), ["clippy", "--target=stax"]);
    }

    #[test]
    fn cargo_args_builtin_target() {
        let t = BuildTarget::new(
            Device::Nanox,
            &serde_json::json!({ "ledger": { "builtin_target": true } }),
            Utf8Path::new("/app/target"),
        );
        let args = cargo_args("check", &t);
        assert_eq!(
            args[..3],
            [
                "check",
                "--target=thumbv6m-none-eabi",
                "--target-dir=/app/target/builtin/nanox"
            ]
        );
        assert_eq!(args[3], "--config");
        assert!(args[4].contains("--cfg=ledger_device=\\\"nanox\\\""));
    }
}
