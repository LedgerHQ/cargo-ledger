use cargo_metadata::Message;
use clap::{ArgEnum, Parser, Subcommand};
use std::process::Command;

use std::env;
use std::fs;
use std::path::Path;
use std::process::Stdio;

use serde_json::json;

mod utils;

use serde_derive::Deserialize;
use utils::*;

#[derive(Debug, Deserialize)]
struct NanosMetadata {
    api_level: Option<String>,
    curve: Vec<String>,
    path: Vec<String>,
    flags: String,
    icon: String,
    icon_small: String,
    name: Option<String>,
}

#[derive(Parser, Debug)]
#[clap(name = "Ledger NanoS load commands")]
#[clap(version = "0.0")]
#[clap(about = "Builds the project and emits a JSON manifest for ledgerctl.")]
struct Cli {
    #[clap(long)]
    #[clap(value_name = "prebuilt ELF exe")]
    use_prebuilt: Option<std::path::PathBuf>,

    #[clap(long)]
    #[clap(help = concat ! (
    "Should the app.hex be placed next to the app.json, or next to the input exe?",
    " ",
    "Typically used with --use-prebuilt when the input exe is in a read-only location.",
    ))]
    hex_next_to_json: bool,

    #[clap(subcommand)]
    command: MainCommand,
}

#[derive(ArgEnum, Clone, Debug)]
enum Device {
    Nanos,
    Nanox,
    Nanosplus,
}

impl From<Device> for &str {
    fn from(device: Device) -> &'static str {
        match device {
            Device::Nanos => "nanos",
            Device::Nanox => "nanox",
            Device::Nanosplus => "nanosplus",
        }
    }
}

#[derive(Subcommand, Debug)]
enum MainCommand {
    Ledger {
        #[clap(arg_enum)]
        device: Device,
        #[clap(short, long)]
        load: bool,
        #[clap(last = true)]
        remaining_args: Vec<String>,
    },
}

fn main() {
    let cli = Cli::parse();

    let ledger_target_path = match env::var("LEDGER_TARGETS") {
        Ok(path) => path,
        Err(_) => String::new(),
    };

    let (device, is_load, remaining_args) = match cli.command {
        MainCommand::Ledger {
            device: d,
            load: a,
            remaining_args: r,
        } => (d, a, r),
    };

    let device_str = <Device as Into<&str>>::into(device.clone());
    let device_json = format!("{}.json", &device_str);
    let device_json_path = Path::new(&ledger_target_path).join(&device_json);
    let exe_path = match cli.use_prebuilt {
        None => {
            let mut cargo_cmd = Command::new("cargo")
                .args([
                    "build",
                    "--release",
                    "-Zbuild-std=core",
                    "-Zbuild-std-features=compiler-builtins-mem",
                    format!("--target={}", device_json_path.display()).as_str(),
                    "--message-format=json-diagnostic-rendered-ansi",
                ])
                .args(&remaining_args)
                .stdout(Stdio::piped())
                .spawn()
                .unwrap();

            let mut exe_path = std::path::PathBuf::new();
            let out = cargo_cmd.stdout.take().unwrap();
            let reader = std::io::BufReader::new(out);
            for message in cargo_metadata::Message::parse_stream(reader) {
                match message.as_ref().unwrap() {
                    Message::CompilerArtifact(artifact) => {
                        if let Some(n) = &artifact.executable {
                            exe_path = n.to_path_buf();
                        }
                    }
                    Message::CompilerMessage(message) => {
                        println!("{message}");
                    }
                    _ => (),
                }
            }

            cargo_cmd.wait().expect("Couldn't get cargo's exit status");

            exe_path
        }
        Some(prebuilt) => prebuilt.canonicalize().unwrap(),
    };

    // Fetch crate metadata without fetching dependencies
    let mut cmd = cargo_metadata::MetadataCommand::new();
    let res = cmd.no_deps().exec().unwrap();

    // Fetch package.metadata.nanos section
    let this_pkg = res.packages.last().unwrap();
    let metadata_value = this_pkg
        .metadata
        .get(device_str)
        .expect("package.metadata.nanos section is missing in Cargo.toml")
        .clone();
    let this_metadata: NanosMetadata =
        serde_json::from_value(metadata_value).unwrap();

    let current_dir = this_pkg.manifest_path.parent().unwrap();

    let hex_file_abs = if cli.hex_next_to_json {
        current_dir
    } else {
        exe_path.parent().unwrap()
    }
        .join("app.hex");

    export_binary(&exe_path, &hex_file_abs);

    // app.json will be placed in the app's root directory
    let app_json_name = format!("app_{}.json", &device_str);
    let app_json = current_dir.join(app_json_name);

    // Find hex file path relative to 'app.json'
    let hex_file = hex_file_abs.strip_prefix(current_dir).unwrap();

    // Retrieve real 'dataSize' from ELF
    let data_size = retrieve_data_size(&exe_path).unwrap();

    // Modify flags to enable BLE if targeting Nano X
    let flags = match device_str {
        "nanos" | "nanosplus" => this_metadata.flags,
        "nanox" => {
            let base = u32::from_str_radix(this_metadata.flags.as_str(), 16)
                .unwrap_or(0);
            format!("0x{:x}", base | 0x200)
        }
        _ => panic!("Unknown device."),
    };

    // Pick icon and targetid according to target
    let (targetid, icon) = match device_str {
        "nanos" => ("0x31100004", &this_metadata.icon),
        "nanox" => ("0x33000004", &this_metadata.icon_small),
        "nanosplus" => ("0x33100004", &this_metadata.icon_small),
        _ => panic!("Unknown device."),
    };

    // create manifest
    let file = fs::File::create(&app_json).unwrap();
    let mut json = json!({
            "name": this_metadata.name.as_ref().unwrap_or(&this_pkg.name),
            "version": &this_pkg.version,
            "icon": icon,
            "targetId": targetid,
            "flags": flags,
            "derivationPath": {
                "curves": this_metadata.curve,
                "paths": this_metadata.path
            },
            "binary": hex_file,
            "dataSize": data_size
        });
    // Ignore apiLevel for Nano S as it is unsupported for now
    match device {
        Device::Nanos => (),
        _ => json["apiLevel"] = serde_json::Value::String(this_metadata.api_level.expect("Missing field 'api_level'")),
    }
    serde_json::to_writer_pretty(file, &json).unwrap();

    if is_load {
        install_with_ledgerctl(current_dir, &app_json);
    }
}
