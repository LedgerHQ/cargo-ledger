use cargo_metadata::Message;
use clap::Parser;
use std::process::{Command, Stdio};

use std::env;
use std::fs;
use std::io;
use std::io::Write;

use serde_json::json;

fn retrieve_data_size(file: &std::path::Path) -> Result<u64, io::Error> {
    let buffer = fs::read(&file)?;
    let elf = goblin::elf::Elf::parse(&buffer).unwrap();

    let mut nvram_data = 0;
    let mut envram_data = 0;
    for s in elf.syms.iter() {
        let symbol_name = elf.strtab.get(s.st_name);
        let name = symbol_name.unwrap().unwrap();
        match name {
            "_nvram_data" => nvram_data = s.st_value,
            "_envram_data" => envram_data = s.st_value,
            _ => (),
        }
    }
    Ok(envram_data - nvram_data)
}

fn export_binary(elf_path: &std::path::Path, dest_bin: &std::path::Path) {
    let objcopy = env::var_os("CARGO_TARGET_THUMBV6M_NONE_EABI_OBJCOPY")
        .unwrap_or("arm-none-eabi-objcopy".into());

    Command::new(objcopy)
        .arg(&elf_path)
        .arg(&dest_bin)
        .args(&["-O", "ihex"])
        .output()
        .expect("Objcopy failed");

    let size = env::var_os("CARGO_TARGET_THUMBV6M_NONE_EABI_SIZE")
        .unwrap_or("arm-none-eabi-size".into());

    // print some size info while we're here
    let out = Command::new(size)
        .arg(&elf_path)
        .output()
        .expect("Size failed");

    io::stdout().write_all(&out.stdout).unwrap();
    io::stderr().write_all(&out.stderr).unwrap();
}

fn install_with_ledgerctl(dir: &std::path::Path, app_json: &std::path::PathBuf) {
    let out = Command::new("ledgerctl")
        .current_dir(dir)
        .args(&["install", "-f", app_json.as_path().to_str().unwrap()])
        .output()
        .expect("fail");

    io::stdout().write_all(&out.stdout).unwrap();
    io::stderr().write_all(&out.stderr).unwrap();
}

use serde_derive::Deserialize;

#[derive(Debug, Deserialize)]
struct NanosMetadata {
    curve: String,
    path: String,
    flags: String,
    icon: String,
    name: Option<String>,
}

#[derive(Parser)]
#[clap(name = "Ledger NanoS load commands")]
#[clap(version = "0.0")]
#[clap(about = "Builds the project and emits a JSON manifest for ledgerctl.")]
struct Cli {
    #[clap(long)]
    #[clap(value_name = "prebuilt ELF exe")]
    use_prebuilt: Option<std::path::PathBuf>,

    #[clap(long)]
    #[clap(help = concat!(
        "Should the app.hex be placed next to the app.json, or next to the input exe?",
        " ",
        "Typically used with --use-prebuilt when the input exe is in a read-only location.",
    ))]
    hex_next_to_json: bool,

    #[clap(subcommand)]
    command: AlwaysPresentSubCommand,
}


#[derive(Parser, Debug)]
enum AlwaysPresentSubCommand {
    Ledger(SubCommandHelper),
    Load,
}

#[derive(Parser, Debug)]
struct SubCommandHelper {
    #[clap(subcommand)]
    subcommand: Option<SubCommand>,
}

#[derive(Parser, Debug)]
enum SubCommand {
    /// Load the app onto a nano
    Load,
}

fn main() {
    let cli: Cli = Cli::parse();

    let exe_path = match cli.use_prebuilt {
        None => {
            let mut cargo_cmd = Command::new("cargo")
                .args(&["build", "--release", "--message-format=json"])
                .stdout(Stdio::piped())
                .spawn()
                .unwrap();

            let mut exe_path = std::path::PathBuf::new();
            let reader =
                std::io::BufReader::new(cargo_cmd.stdout.take().unwrap());
            for message in cargo_metadata::Message::parse_stream(reader) {
                if let Message::CompilerArtifact(artifact) = message.unwrap() {
                    if let Some(n) = artifact.executable {
                        exe_path = n;
                    }
                }
            }

            let _output =
                cargo_cmd.wait().expect("Couldn't get cargo's exit status");

            exe_path
        }
        Some(prebuilt) => prebuilt,
    };

    let mut cmd = cargo_metadata::MetadataCommand::new();
    let res = cmd.no_deps().exec().unwrap();

    let this_pkg = res.packages.last().unwrap();
    let this_metadata: NanosMetadata =
        serde_json::from_value(this_pkg.metadata["nanos"].clone()).unwrap();

    let current_dir = std::path::Path::new(&this_pkg.manifest_path)
        .parent()
        .unwrap();

    let hex_file_abs = if cli.hex_next_to_json {
        current_dir
    } else {
        exe_path.parent().unwrap()
    }
    .join("app.hex");

    export_binary(&exe_path, &hex_file_abs);

    // app.json will be placed in the app's root directory
    let app_json = current_dir.join("app.json");

    // Find hex file path relative to 'app.json'
    let hex_file = hex_file_abs.strip_prefix(current_dir).unwrap();

    // Retrieve real 'dataSize' from ELF
    let data_size = retrieve_data_size(&exe_path).unwrap();

    // create manifest
    let file = fs::File::create(&app_json).unwrap();
    let json = json!({
        "name": this_metadata.name.as_ref().unwrap_or(&this_pkg.name),
        "version": &this_pkg.version,
        "icon": &this_metadata.icon,
        "targetId": "0x31100004",
        "flags": this_metadata.flags,
        "derivationPath": {
            "curves": [ this_metadata.curve ],
            "paths": [ this_metadata.path ]
        },
        "binary": hex_file,
        "dataSize": data_size
    });
    serde_json::to_writer_pretty(file, &json).unwrap();

    match cli.command {
        AlwaysPresentSubCommand::Ledger(subc) => { 
            if let Some(SubCommand::Load) = subc.subcommand {
                install_with_ledgerctl(current_dir, &app_json);
            }
        },
        AlwaysPresentSubCommand::Load => install_with_ledgerctl(current_dir, &app_json),
    }
}
